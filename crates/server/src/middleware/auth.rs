//! Authentication middleware for K8s multi-user deployment.
//!
//! This module provides JWT-based authentication for the server when running
//! in Kubernetes deployment mode. It validates incoming requests and extracts
//! user context for downstream handlers.

use axum::{
    Json,
    body::Body,
    extract::{FromRequestParts, Request},
    http::{StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use thiserror::Error;
use utils::response::ApiResponse;
use uuid::Uuid;

/// User context extracted from a validated JWT token.
///
/// This struct is inserted into request extensions after successful authentication
/// and can be extracted by route handlers to access user information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserContext {
    /// Unique identifier for the authenticated user.
    pub user_id: Uuid,
    /// Optional email address of the user (may not be present in all tokens).
    pub email: Option<String>,
}

impl UserContext {
    /// Creates a new UserContext with the given user ID and optional email.
    pub fn new(user_id: Uuid, email: Option<String>) -> Self {
        Self { user_id, email }
    }
}

/// Errors that can occur during authentication.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The Authorization header is missing from the request.
    #[error("Missing authorization header")]
    MissingAuthHeader,

    /// The provided JWT token is invalid (malformed, bad signature, etc.).
    #[error("Invalid token")]
    InvalidToken,

    /// The JWT token has expired.
    #[error("Token expired")]
    ExpiredToken,

    /// A required claim is missing from the JWT token.
    #[error("Missing required claim: {0}")]
    MissingClaim(String),

    /// The JWT secret is not configured or invalid.
    #[error("JWT secret not configured")]
    SecretNotConfigured,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let timestamp = chrono::Utc::now().to_rfc3339();

        let (status_code, error_type) = match &self {
            AuthError::MissingAuthHeader => (StatusCode::UNAUTHORIZED, "MissingAuthHeader"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "InvalidToken"),
            AuthError::ExpiredToken => (StatusCode::UNAUTHORIZED, "ExpiredToken"),
            AuthError::MissingClaim(_) => (StatusCode::BAD_REQUEST, "MissingClaim"),
            AuthError::SecretNotConfigured => {
                (StatusCode::INTERNAL_SERVER_ERROR, "SecretNotConfigured")
            }
        };

        let error_message = match &self {
            AuthError::MissingAuthHeader => {
                "Authorization header is required. Please provide a valid Bearer token.".to_string()
            }
            AuthError::InvalidToken => {
                "The provided token is invalid. Please sign in again.".to_string()
            }
            AuthError::ExpiredToken => {
                "Your session has expired. Please sign in again.".to_string()
            }
            AuthError::MissingClaim(claim) => {
                format!("Token is missing required claim: {}. Please sign in again.", claim)
            }
            AuthError::SecretNotConfigured => {
                "Authentication is not properly configured. Please contact support.".to_string()
            }
        };

        // Structured logging for security audit
        // All auth failures are logged with consistent fields for analysis
        tracing::warn!(
            action = "auth_failure",
            error_type = error_type,
            error_message = %error_message,
            status_code = status_code.as_u16(),
            timestamp = %timestamp,
            security_event = true,
            "Authentication error"
        );

        let response = ApiResponse::<()>::error(&error_message);
        (status_code, Json(response)).into_response()
    }
}

/// JWT claims structure for token validation.
///
/// This struct represents the expected claims in the JWT token.
/// The `sub` (subject) claim is used as the user_id.
#[derive(Debug, Clone, Deserialize)]
pub struct JwtClaims {
    /// Subject claim - used as user_id (UUID format expected).
    pub sub: String,
    /// Optional email claim.
    pub email: Option<String>,
    /// Expiration time (Unix timestamp).
    pub exp: i64,
    /// Issued at time (Unix timestamp).
    #[serde(default)]
    pub iat: Option<i64>,
}

/// Retrieves the JWT secret from the environment.
///
/// The secret is loaded once from the `JWT_SECRET` environment variable
/// and cached for subsequent calls.
fn get_jwt_secret() -> Option<&'static [u8]> {
    static JWT_SECRET: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    JWT_SECRET
        .get_or_init(|| std::env::var("JWT_SECRET").ok().map(|s| s.into_bytes()))
        .as_ref()
        .map(|v| v.as_slice())
}

/// Extracts the Bearer token from the Authorization header.
///
/// # Arguments
///
/// * `request` - The HTTP request to extract the token from.
///
/// # Returns
///
/// Returns `Ok(token)` if a valid Bearer token is found, or `Err(AuthError)` otherwise.
///
/// # Example
///
/// ```ignore
/// // Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
/// let token = extract_bearer_token(&request)?;
/// ```
pub fn extract_bearer_token(request: &Request<Body>) -> Result<&str, AuthError> {
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or(AuthError::MissingAuthHeader)?
        .to_str()
        .map_err(|_| AuthError::InvalidToken)?;

    auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .ok_or(AuthError::InvalidToken)
}

/// Extracts the token from query parameter (for WebSocket connections).
///
/// WebSocket connections cannot send custom headers from browsers,
/// so we support passing the token as a query parameter `?token=...`
///
/// # Arguments
///
/// * `request` - The HTTP request to extract the token from.
///
/// # Returns
///
/// Returns `Some(token)` if found in query params, `None` otherwise.
fn extract_token_from_query(request: &Request<Body>) -> Option<String> {
    request
        .uri()
        .query()
        .and_then(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "token")
                .map(|(_, value)| value.into_owned())
        })
}

/// Verifies a JWT token and extracts the claims.
///
/// This function validates the token's signature, expiration, and extracts
/// the user claims from the payload.
///
/// # Arguments
///
/// * `token` - The JWT token string to verify.
/// * `secret` - The secret key used to sign the token.
///
/// # Returns
///
/// Returns `Ok(UserContext)` with the extracted user information on success,
/// or `Err(AuthError)` on validation failure.
///
/// # Errors
///
/// * `AuthError::InvalidToken` - Token signature is invalid or malformed.
/// * `AuthError::ExpiredToken` - Token has expired.
/// * `AuthError::MissingClaim` - Required claim (sub) is missing or invalid.
pub fn verify_jwt(token: &str, secret: &[u8]) -> Result<UserContext, AuthError> {
    let decoding_key = DecodingKey::from_secret(secret);

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.required_spec_claims.clear();
    validation.required_spec_claims.insert("sub".to_string());
    validation.required_spec_claims.insert("exp".to_string());

    let token_data = decode::<JwtClaims>(token, &decoding_key, &validation).map_err(|err| {
        match err.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
            jsonwebtoken::errors::ErrorKind::InvalidSignature
            | jsonwebtoken::errors::ErrorKind::InvalidToken => AuthError::InvalidToken,
            jsonwebtoken::errors::ErrorKind::MissingRequiredClaim(claim) => {
                AuthError::MissingClaim(claim.to_string())
            }
            _ => AuthError::InvalidToken,
        }
    })?;

    let claims = token_data.claims;

    // Parse the subject as a UUID for user_id
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AuthError::MissingClaim("sub (invalid UUID format)".to_string()))?;

    Ok(UserContext::new(user_id, claims.email))
}

/// Axum middleware that requires authentication for protected routes.
///
/// This middleware extracts the JWT token from the Authorization header,
/// validates it, and inserts the `UserContext` into request extensions
/// for use by downstream handlers.
///
/// # Example
///
/// ```ignore
/// use axum::{Router, middleware};
/// use server::middleware::auth::require_user;
///
/// let protected_routes = Router::new()
///     .route("/api/projects", get(list_projects))
///     .layer(middleware::from_fn(require_user));
/// ```
pub async fn require_user(mut request: Request<Body>, next: Next) -> Result<Response, AuthError> {
    let secret = get_jwt_secret().ok_or(AuthError::SecretNotConfigured)?;
    let timestamp = chrono::Utc::now().to_rfc3339();

    // Try Authorization header first, then fall back to query parameter (for WebSocket)
    let token: String = match extract_bearer_token(&request) {
        Ok(t) => t.to_string(),
        Err(AuthError::MissingAuthHeader) => {
            // For WebSocket connections, check query parameter
            extract_token_from_query(&request).ok_or(AuthError::MissingAuthHeader)?
        }
        Err(e) => return Err(e),
    };
    let user_context = verify_jwt(&token, secret)?;

    // Structured logging for successful authentication (security audit)
    tracing::debug!(
        action = "auth_success",
        user_id = %user_context.user_id,
        email = ?user_context.email,
        timestamp = %timestamp,
        security_event = true,
        "User authenticated successfully"
    );

    // Insert UserContext into request extensions for downstream handlers
    request.extensions_mut().insert(user_context);

    Ok(next.run(request).await)
}

/// Axum extractor for `UserContext` from request extensions.
///
/// This extractor retrieves the `UserContext` that was inserted by the
/// `require_user` middleware. It should only be used on routes that are
/// protected by the auth middleware.
///
/// # Example
///
/// ```ignore
/// use server::middleware::auth::UserContextExt;
///
/// async fn list_projects(
///     UserContextExt(user): UserContextExt,
/// ) -> impl IntoResponse {
///     // user is the authenticated UserContext
///     let projects = get_projects_for_user(user.user_id).await;
///     Json(projects)
/// }
/// ```
#[derive(Debug, Clone)]
pub struct UserContextExt(pub UserContext);

impl<S> FromRequestParts<S> for UserContextExt
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<UserContext>()
            .cloned()
            .map(UserContextExt)
            .ok_or(AuthError::MissingAuthHeader)
    }
}

/// Optional user context extractor for backward compatibility with desktop mode.
///
/// This extractor returns `Some(UserContext)` if authentication middleware has
/// inserted a user context, or `None` if running in desktop mode without auth.
///
/// Use this in route handlers that need to support both authenticated multi-user
/// mode and unauthenticated desktop mode.
///
/// # Example
///
/// ```ignore
/// use server::middleware::auth::OptionalUserContext;
///
/// async fn list_projects(
///     OptionalUserContext(user): OptionalUserContext,
/// ) -> impl IntoResponse {
///     let user_id = user.map(|u| u.user_id);
///     // Use user_id for filtering if available, otherwise return all
/// }
/// ```
#[derive(Debug, Clone)]
pub struct OptionalUserContext(pub Option<UserContext>);

impl<S> FromRequestParts<S> for OptionalUserContext
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalUserContext(
            parts.extensions.get::<UserContext>().cloned(),
        ))
    }
}

impl OptionalUserContext {
    /// Get the user ID if available.
    #[inline]
    pub fn user_id(&self) -> Option<uuid::Uuid> {
        self.0.as_ref().map(|ctx| ctx.user_id)
    }

    /// Check if a user context is present (authenticated request).
    #[inline]
    pub fn is_authenticated(&self) -> bool {
        self.0.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use jsonwebtoken::{EncodingKey, Header, encode};

    /// Test secret for JWT signing/verification in tests.
    const TEST_SECRET: &[u8] = b"test-secret-for-jwt-signing-32-bytes";

    /// Helper to create a valid JWT token for testing.
    fn create_test_jwt(user_id: &Uuid, email: Option<&str>, exp_offset_secs: i64) -> String {
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": user_id.to_string(),
            "email": email,
            "exp": now + exp_offset_secs,
            "iat": now,
        });

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET),
        )
        .expect("encoding should succeed")
    }

    /// Helper to create a request with optional Authorization header.
    fn make_request_with_auth(auth_header: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().uri("/test").method("GET");
        if let Some(auth) = auth_header {
            builder = builder.header(header::AUTHORIZATION, auth);
        }
        builder.body(Body::empty()).unwrap()
    }

    // ========== UserContext Tests ==========

    #[test]
    fn test_user_context_new() {
        let user_id = Uuid::new_v4();
        let email = Some("test@example.com".to_string());
        let ctx = UserContext::new(user_id, email.clone());

        assert_eq!(ctx.user_id, user_id);
        assert_eq!(ctx.email, email);
    }

    #[test]
    fn test_user_context_without_email() {
        let user_id = Uuid::new_v4();
        let ctx = UserContext::new(user_id, None);

        assert_eq!(ctx.user_id, user_id);
        assert!(ctx.email.is_none());
    }

    #[test]
    fn test_user_context_serialization() {
        let user_id = Uuid::new_v4();
        let email = Some("test@example.com".to_string());
        let ctx = UserContext::new(user_id, email);

        let json = serde_json::to_string(&ctx).expect("serialization should succeed");
        let deserialized: UserContext =
            serde_json::from_str(&json).expect("deserialization should succeed");

        assert_eq!(deserialized.user_id, ctx.user_id);
        assert_eq!(deserialized.email, ctx.email);
    }

    // ========== AuthError Tests ==========

    #[tokio::test]
    async fn test_auth_error_missing_header_response() {
        let error = AuthError::MissingAuthHeader;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_error_invalid_token_response() {
        let error = AuthError::InvalidToken;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_error_expired_token_response() {
        let error = AuthError::ExpiredToken;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_error_missing_claim_response() {
        let error = AuthError::MissingClaim("sub".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_auth_error_secret_not_configured_response() {
        let error = AuthError::SecretNotConfigured;
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_auth_error_response_body_contains_error_message() {
        let error = AuthError::InvalidToken;
        let response = error.into_response();

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body_str = String::from_utf8_lossy(&body);

        assert!(body_str.contains("invalid"));
    }

    // ========== Bearer Token Extraction Tests (AUTH-05, AUTH-06) ==========

    #[test]
    fn test_extract_bearer_token_valid() {
        let request = make_request_with_auth(Some("Bearer valid_token_here"));
        let result = extract_bearer_token(&request);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "valid_token_here");
    }

    #[test]
    fn test_extract_bearer_token_lowercase_bearer() {
        let request = make_request_with_auth(Some("bearer lowercase_token"));
        let result = extract_bearer_token(&request);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "lowercase_token");
    }

    #[test]
    fn test_extract_bearer_token_missing_header() {
        let request = make_request_with_auth(None);
        let result = extract_bearer_token(&request);

        assert!(matches!(result, Err(AuthError::MissingAuthHeader)));
    }

    #[test]
    fn test_extract_bearer_token_no_bearer_prefix() {
        let request = make_request_with_auth(Some("Basic dXNlcjpwYXNz"));
        let result = extract_bearer_token(&request);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_extract_bearer_token_empty_token() {
        let request = make_request_with_auth(Some("Bearer "));
        let result = extract_bearer_token(&request);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_extract_bearer_token_malformed_header() {
        let request = make_request_with_auth(Some("BearerNoSpace"));
        let result = extract_bearer_token(&request);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    // ========== JWT Validation Tests (AUTH-01, AUTH-02, AUTH-03, AUTH-04) ==========

    #[test]
    fn test_verify_jwt_valid_token() {
        let user_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let token = create_test_jwt(&user_id, Some("test@example.com"), 3600);

        let result = verify_jwt(&token, TEST_SECRET);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert_eq!(ctx.user_id, user_id);
        assert_eq!(ctx.email, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_verify_jwt_valid_token_without_email() {
        let user_id = Uuid::new_v4();
        let token = create_test_jwt(&user_id, None, 3600);

        let result = verify_jwt(&token, TEST_SECRET);

        assert!(result.is_ok());
        let ctx = result.unwrap();
        assert_eq!(ctx.user_id, user_id);
        assert!(ctx.email.is_none());
    }

    #[test]
    fn test_verify_jwt_expired_token() {
        let user_id = Uuid::new_v4();
        // Create token that expired 1 hour ago
        let token = create_test_jwt(&user_id, Some("test@example.com"), -3600);

        let result = verify_jwt(&token, TEST_SECRET);

        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[test]
    fn test_verify_jwt_invalid_signature() {
        let user_id = Uuid::new_v4();
        let token = create_test_jwt(&user_id, Some("test@example.com"), 3600);

        // Verify with a different secret
        let wrong_secret = b"wrong-secret-key-for-validation";
        let result = verify_jwt(&token, wrong_secret);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_jwt_missing_sub_claim() {
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "email": "test@example.com",
            "exp": now + 3600,
            "iat": now,
        });

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET),
        )
        .expect("encoding should succeed");

        let result = verify_jwt(&token, TEST_SECRET);

        // Should fail - either as MissingClaim or InvalidToken depending on jsonwebtoken version
        assert!(
            matches!(
                result,
                Err(AuthError::MissingClaim(_)) | Err(AuthError::InvalidToken)
            ),
            "Expected MissingClaim or InvalidToken error, got {:?}",
            result
        );
    }

    #[test]
    fn test_verify_jwt_invalid_uuid_in_sub() {
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "sub": "not-a-valid-uuid",
            "exp": now + 3600,
            "iat": now,
        });

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET),
        )
        .expect("encoding should succeed");

        let result = verify_jwt(&token, TEST_SECRET);

        assert!(matches!(result, Err(AuthError::MissingClaim(_))));
        if let Err(AuthError::MissingClaim(msg)) = result {
            assert!(msg.contains("invalid UUID"));
        }
    }

    #[test]
    fn test_verify_jwt_malformed_token() {
        let result = verify_jwt("not.a.valid.jwt.token", TEST_SECRET);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_jwt_empty_token() {
        let result = verify_jwt("", TEST_SECRET);

        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_verify_jwt_missing_exp_claim() {
        let now = chrono::Utc::now().timestamp();
        let user_id = Uuid::new_v4();
        let claims = serde_json::json!({
            "sub": user_id.to_string(),
            "email": "test@example.com",
            "iat": now,
        });

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(TEST_SECRET),
        )
        .expect("encoding should succeed");

        let result = verify_jwt(&token, TEST_SECRET);

        // Should fail - either as MissingClaim or InvalidToken depending on jsonwebtoken version
        assert!(
            matches!(
                result,
                Err(AuthError::MissingClaim(_)) | Err(AuthError::InvalidToken)
            ),
            "Expected MissingClaim or InvalidToken error, got {:?}",
            result
        );
    }

    // ========== JwtClaims Tests ==========

    #[test]
    fn test_jwt_claims_deserialization() {
        let claims_json = r#"{
            "sub": "550e8400-e29b-41d4-a716-446655440000",
            "email": "user@example.com",
            "exp": 1700000000,
            "iat": 1699996400
        }"#;

        let claims: JwtClaims = serde_json::from_str(claims_json).expect("should deserialize");

        assert_eq!(claims.sub, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(claims.email, Some("user@example.com".to_string()));
        assert_eq!(claims.exp, 1700000000);
        assert_eq!(claims.iat, Some(1699996400));
    }

    #[test]
    fn test_jwt_claims_deserialization_minimal() {
        let claims_json = r#"{
            "sub": "550e8400-e29b-41d4-a716-446655440000",
            "exp": 1700000000
        }"#;

        let claims: JwtClaims = serde_json::from_str(claims_json).expect("should deserialize");

        assert_eq!(claims.sub, "550e8400-e29b-41d4-a716-446655440000");
        assert!(claims.email.is_none());
        assert_eq!(claims.exp, 1700000000);
        assert!(claims.iat.is_none());
    }
}
