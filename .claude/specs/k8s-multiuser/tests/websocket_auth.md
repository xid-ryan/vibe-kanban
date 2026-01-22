# WebSocket Authentication Unit Test Cases

## Test File

`crates/server/tests/websocket_auth.rs`

## Test Purpose

This module tests WebSocket authentication for terminal connections in Kubernetes multi-user mode. It validates that:

1. WebSocket connections require valid JWT tokens in K8s mode
2. Invalid/expired/missing tokens are properly rejected
3. PTY session operations validate user ownership
4. Cross-user access attempts return appropriate errors
5. Desktop mode continues to work without authentication

## Test Cases Overview

| Case ID | Feature Description | Test Type |
|---------|-------------------|-----------|
| WS-01 | WebSocket connection without token in K8s mode | Error Test |
| WS-02 | WebSocket connection with valid JWT token | Positive Test |
| WS-03 | WebSocket connection with expired JWT token | Error Test |
| WS-04 | WebSocket connection with invalid JWT signature | Error Test |
| WS-05 | WebSocket connection with malformed JWT token | Error Test |
| WS-06 | PTY session ownership validation - valid user | Positive Test |
| WS-07 | PTY session ownership validation - invalid user | Error Test |
| WS-08 | PTY write operation validates ownership | Positive Test |
| WS-09 | PTY resize operation validates ownership | Positive Test |
| WS-10 | PTY close operation validates ownership | Positive Test |
| WS-11 | Cross-user PTY access returns SessionNotFound | Security Test |
| WS-12 | Desktop mode allows unauthenticated access | Positive Test |
| WS-13 | validate_ws_auth returns UserContext in K8s mode | Positive Test |
| WS-14 | validate_ws_auth returns None in Desktop mode | Positive Test |
| WS-15 | JWT token with invalid UUID in sub claim | Error Test |
| WS-16 | List user sessions returns only owned sessions | Positive Test |
| WS-17 | Session exists check with wrong user returns false | Security Test |

## Detailed Test Steps

### WS-01: WebSocket connection without token in K8s mode

**Test Purpose**: Verify that WebSocket connections are rejected when no authentication token is provided in Kubernetes mode.

**Test Data Preparation**:
- Set `DEPLOYMENT_MODE=kubernetes` environment variable
- Set `JWT_SECRET` environment variable

**Test Steps**:
1. Set up Kubernetes deployment mode
2. Call `validate_ws_auth(None)`
3. Verify the function returns `Err(ApiError::Unauthorized)`

**Expected Results**:
- Function returns `ApiError::Unauthorized`
- Connection is denied before WebSocket upgrade

---

### WS-02: WebSocket connection with valid JWT token

**Test Purpose**: Verify that WebSocket connections succeed with a valid JWT token.

**Test Data Preparation**:
- Create valid JWT token with user_id and non-expired timestamp
- Set `DEPLOYMENT_MODE=kubernetes`
- Set `JWT_SECRET` matching the token's signing secret

**Test Steps**:
1. Generate a valid JWT token with known user_id
2. Call `validate_ws_auth(Some(&token))`
3. Verify the returned UserContext contains correct user_id

**Expected Results**:
- Function returns `Ok(Some(UserContext))`
- UserContext.user_id matches the token's sub claim

---

### WS-03: WebSocket connection with expired JWT token

**Test Purpose**: Verify that expired JWT tokens are rejected.

**Test Data Preparation**:
- Create JWT token with `exp` timestamp in the past

**Test Steps**:
1. Generate a JWT token that expired 1 hour ago
2. Call `validate_ws_auth(Some(&token))`
3. Verify the function returns `Err(ApiError::Unauthorized)`

**Expected Results**:
- Function returns `ApiError::Unauthorized`
- Token validation fails due to expiration

---

### WS-04: WebSocket connection with invalid JWT signature

**Test Purpose**: Verify that tokens signed with wrong secret are rejected.

**Test Data Preparation**:
- Create JWT token signed with a different secret

**Test Steps**:
1. Generate a JWT token with a different signing secret
2. Call `validate_ws_auth(Some(&token))`
3. Verify signature validation fails

**Expected Results**:
- Function returns `ApiError::Unauthorized`
- Invalid signature is detected and rejected

---

### WS-05: WebSocket connection with malformed JWT token

**Test Purpose**: Verify that malformed tokens are rejected.

**Test Data Preparation**:
- Use a string that is not a valid JWT format

**Test Steps**:
1. Use "not.a.valid.jwt.token" as token
2. Call `validate_ws_auth(Some(&token))`
3. Verify the function returns an error

**Expected Results**:
- Function returns `ApiError::Unauthorized`
- Malformed token is properly handled

---

### WS-06: PTY session ownership validation - valid user

**Test Purpose**: Verify that session ownership validation succeeds for the session owner.

**Test Data Preparation**:
- Create a PtyService instance
- Create a session with a known user_id

**Test Steps**:
1. Create a PTY session for user A
2. Call `session_exists_for_user(session_id, user_a_id)`
3. Verify the function returns true

**Expected Results**:
- Function returns `true`
- Session is accessible by owner

---

### WS-07: PTY session ownership validation - invalid user

**Test Purpose**: Verify that session ownership validation fails for non-owners.

**Test Data Preparation**:
- Create a PtyService instance
- Create a session with user A

**Test Steps**:
1. Create a PTY session for user A
2. Call `session_exists_for_user(session_id, user_b_id)`
3. Verify the function returns false

**Expected Results**:
- Function returns `false`
- Session is not accessible by other users

---

### WS-08: PTY write operation validates ownership

**Test Purpose**: Verify that PTY write operations require ownership.

**Test Data Preparation**:
- Create a PTY session with user A

**Test Steps**:
1. Create a PTY session for user A
2. Attempt to write with user B's ID
3. Verify write is rejected

**Expected Results**:
- Write returns `PtyError::SessionNotFound`
- Data is not written to the session

---

### WS-09: PTY resize operation validates ownership

**Test Purpose**: Verify that PTY resize operations require ownership.

**Test Data Preparation**:
- Create a PTY session with user A

**Test Steps**:
1. Create a PTY session for user A
2. Attempt to resize with user B's ID
3. Verify resize is rejected

**Expected Results**:
- Resize returns `PtyError::SessionNotFound`
- Session size is not modified

---

### WS-10: PTY close operation validates ownership

**Test Purpose**: Verify that PTY close operations require ownership.

**Test Data Preparation**:
- Create a PTY session with user A

**Test Steps**:
1. Create a PTY session for user A
2. Attempt to close with user B's ID
3. Verify close is rejected

**Expected Results**:
- Close returns `PtyError::SessionNotFound`
- Session remains open

---

### WS-11: Cross-user PTY access returns SessionNotFound

**Test Purpose**: Verify that cross-user access returns SessionNotFound (not Unauthorized) to avoid information leakage.

**Test Data Preparation**:
- Create sessions for multiple users

**Test Steps**:
1. Create PTY session for user A
2. Attempt to access with user B
3. Verify error is SessionNotFound, not Unauthorized

**Expected Results**:
- Error is `PtyError::SessionNotFound`
- No information leaked about session existence

---

### WS-12: Desktop mode allows unauthenticated access

**Test Purpose**: Verify that Desktop mode does not require authentication.

**Test Data Preparation**:
- Set `DEPLOYMENT_MODE=desktop`
- Remove `JWT_SECRET`

**Test Steps**:
1. Set up Desktop deployment mode
2. Call `validate_ws_auth(None)`
3. Verify function returns `Ok(None)`

**Expected Results**:
- Function returns `Ok(None)`
- No authentication required in desktop mode

---

### WS-13: validate_ws_auth returns UserContext in K8s mode

**Test Purpose**: Verify that valid authentication in K8s mode returns proper UserContext.

**Test Data Preparation**:
- Set `DEPLOYMENT_MODE=kubernetes`
- Create valid JWT token with email claim

**Test Steps**:
1. Create JWT with user_id and email
2. Call `validate_ws_auth(Some(&token))`
3. Verify UserContext has correct fields

**Expected Results**:
- UserContext.user_id matches token sub claim
- UserContext.email matches token email claim

---

### WS-14: validate_ws_auth returns None in Desktop mode

**Test Purpose**: Verify that Desktop mode returns None (not UserContext).

**Test Data Preparation**:
- Set `DEPLOYMENT_MODE=desktop`

**Test Steps**:
1. Set Desktop mode
2. Call `validate_ws_auth(None)`
3. Verify returns `Ok(None)`

**Expected Results**:
- Function returns `Ok(None)`, not a UserContext
- Backward compatibility maintained

---

### WS-15: JWT token with invalid UUID in sub claim

**Test Purpose**: Verify that tokens with non-UUID sub claims are rejected.

**Test Data Preparation**:
- Create JWT with `sub: "not-a-uuid"`

**Test Steps**:
1. Create JWT with invalid sub format
2. Call `validate_ws_auth(Some(&token))`
3. Verify validation fails

**Expected Results**:
- Function returns error
- Invalid UUID format is caught

---

### WS-16: List user sessions returns only owned sessions

**Test Purpose**: Verify that listing sessions returns only the requesting user's sessions.

**Test Data Preparation**:
- Create sessions for user A and user B

**Test Steps**:
1. Create 2 sessions for user A
2. Create 1 session for user B
3. Call `list_user_sessions(user_a_id)`
4. Verify only 2 sessions returned

**Expected Results**:
- Returns exactly 2 session IDs
- All returned sessions belong to user A

---

### WS-17: Session exists check with wrong user returns false

**Test Purpose**: Verify that session_exists_for_user returns false for non-owners.

**Test Data Preparation**:
- Create session for user A

**Test Steps**:
1. Create session for user A
2. Call `session_exists_for_user(session_id, user_b_id)`
3. Verify returns false

**Expected Results**:
- Function returns `false`
- No information about session existence leaked

---

## Test Considerations

### Mock Strategy

- JWT tokens are created using `jsonwebtoken::encode` with a test secret
- Environment variables (`DEPLOYMENT_MODE`, `JWT_SECRET`) are mocked for tests
- PtyService is tested in isolation without actual PTY processes

### Boundary Conditions

- Token expiration at exact boundary (exp = now)
- Empty token string
- Token with missing required claims
- Very long token strings

### Asynchronous Operations

- WebSocket upgrade is async but validation is sync
- PTY operations use async/await but internally block on mutex
- Tests use `tokio::test` for async test execution

### Security Considerations

- Cross-user access must return SessionNotFound (not Unauthorized) to prevent enumeration
- Invalid tokens should not reveal whether the user exists
- All authentication failures should be logged for audit
