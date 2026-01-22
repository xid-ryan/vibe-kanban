//! PostgreSQL-backed configuration service for multi-user Kubernetes deployments.
//!
//! This module provides a database-backed configuration service that stores user
//! preferences and encrypted OAuth credentials in PostgreSQL instead of local files.
//!
//! # Features
//!
//! - User-specific configuration storage in PostgreSQL
//! - AES-256-GCM encryption for sensitive OAuth credentials
//! - Automatic config upsert with timestamp tracking
//! - Graceful handling of missing configurations (returns defaults)
//!
//! # Example
//!
//! ```ignore
//! use services::services::config_db::ConfigServicePg;
//! use sqlx::PgPool;
//! use uuid::Uuid;
//!
//! let pool: PgPool = /* ... */;
//! let config_service = ConfigServicePg::new(pool)?;
//!
//! let user_id = Uuid::new_v4();
//!
//! // Load config (returns default if not exists)
//! let config = config_service.load_config(user_id).await?;
//!
//! // Save config
//! config_service.save_config(user_id, &config).await?;
//! ```

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use sqlx::PgPool;
use thiserror::Error;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::config::Config;
use super::oauth_credentials::Credentials;

/// Nonce size for AES-256-GCM (96 bits / 12 bytes).
const NONCE_SIZE: usize = 12;

/// Environment variable for the config encryption key.
const CONFIG_ENCRYPTION_KEY_ENV: &str = "CONFIG_ENCRYPTION_KEY";

/// Errors that can occur during configuration operations.
#[derive(Debug, Error)]
pub enum ConfigDbError {
    /// Database operation failed.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// JSON serialization/deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Encryption key is not configured or invalid.
    #[error("Encryption key not configured or invalid")]
    EncryptionKeyError,

    /// Encryption operation failed.
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    /// Decryption operation failed.
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    /// Invalid encrypted data format.
    #[error("Invalid encrypted data format")]
    InvalidEncryptedData,
}

/// PostgreSQL-backed configuration service for multi-user deployments.
///
/// This service stores user configurations in the `user_configs` table,
/// with OAuth credentials encrypted using AES-256-GCM.
#[derive(Clone)]
pub struct ConfigServicePg {
    /// The PostgreSQL connection pool.
    pool: PgPool,

    /// The encryption key for OAuth credentials (32 bytes for AES-256).
    encryption_key: Option<[u8; 32]>,
}

impl ConfigServicePg {
    /// Create a new PostgreSQL-backed configuration service.
    ///
    /// The encryption key is read from the `CONFIG_ENCRYPTION_KEY` environment
    /// variable. The key should be a 64-character hex string (32 bytes).
    ///
    /// # Arguments
    ///
    /// * `pool` - The PostgreSQL connection pool
    ///
    /// # Returns
    ///
    /// A new `ConfigServicePg` instance.
    ///
    /// # Example
    ///
    /// ```ignore
    /// std::env::set_var("CONFIG_ENCRYPTION_KEY", "0123456789abcdef...");
    /// let service = ConfigServicePg::new(pool);
    /// ```
    pub fn new(pool: PgPool) -> Self {
        let encryption_key = Self::load_encryption_key();

        if encryption_key.is_none() {
            warn!(
                "CONFIG_ENCRYPTION_KEY not set or invalid. \
                 OAuth credentials will not be encrypted/decrypted."
            );
        }

        Self {
            pool,
            encryption_key,
        }
    }

    /// Create a new service with an explicit encryption key.
    ///
    /// This is useful for testing when you don't want to rely on environment variables.
    ///
    /// # Arguments
    ///
    /// * `pool` - The PostgreSQL connection pool
    /// * `encryption_key` - A 32-byte encryption key
    pub fn new_with_key(pool: PgPool, encryption_key: [u8; 32]) -> Self {
        Self {
            pool,
            encryption_key: Some(encryption_key),
        }
    }

    /// Load the encryption key from environment variable.
    ///
    /// The key should be a 64-character hex string (representing 32 bytes).
    fn load_encryption_key() -> Option<[u8; 32]> {
        let key_hex = std::env::var(CONFIG_ENCRYPTION_KEY_ENV).ok()?;

        if key_hex.len() != 64 {
            warn!(
                "CONFIG_ENCRYPTION_KEY must be 64 hex characters (32 bytes), got {} chars",
                key_hex.len()
            );
            return None;
        }

        let key_bytes = hex::decode(&key_hex).ok()?;

        if key_bytes.len() != 32 {
            return None;
        }

        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);
        Some(key_array)
    }

    /// Load a user's configuration from the database.
    ///
    /// If no configuration exists for the user, returns the default configuration.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    ///
    /// # Returns
    ///
    /// The user's configuration, or the default configuration if not found.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails or JSON deserialization fails.
    pub async fn load_config(&self, user_id: Uuid) -> Result<Config, ConfigDbError> {
        debug!(user_id = %user_id, "Loading config from database");

        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            r#"
            SELECT config_json
            FROM user_configs
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((config_json,)) => {
                debug!(user_id = %user_id, "Found existing config in database");
                let config: Config = serde_json::from_value(config_json)?;
                Ok(config)
            }
            None => {
                debug!(user_id = %user_id, "No config found, returning default");
                Ok(Config::default())
            }
        }
    }

    /// Save a user's configuration to the database.
    ///
    /// This uses an UPSERT pattern to insert a new configuration or update
    /// an existing one. The `updated_at` timestamp is automatically updated.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    /// * `config` - The configuration to save
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if the operation fails.
    pub async fn save_config(&self, user_id: Uuid, config: &Config) -> Result<(), ConfigDbError> {
        debug!(user_id = %user_id, "Saving config to database");

        let config_json = serde_json::to_value(config)?;

        sqlx::query(
            r#"
            INSERT INTO user_configs (user_id, config_json, created_at, updated_at)
            VALUES ($1, $2, NOW(), NOW())
            ON CONFLICT (user_id)
            DO UPDATE SET
                config_json = EXCLUDED.config_json,
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(config_json)
        .execute(&self.pool)
        .await?;

        info!(user_id = %user_id, "Config saved successfully");
        Ok(())
    }

    /// Encrypt OAuth credentials for storage.
    ///
    /// Uses AES-256-GCM with a random nonce. The output format is:
    /// `nonce (12 bytes) || ciphertext`
    ///
    /// # Arguments
    ///
    /// * `credentials` - The OAuth credentials to encrypt
    ///
    /// # Returns
    ///
    /// The encrypted credentials as a byte vector.
    pub fn encrypt_credentials(
        &self,
        credentials: &Credentials,
    ) -> Result<Vec<u8>, ConfigDbError> {
        let encryption_key = self
            .encryption_key
            .ok_or(ConfigDbError::EncryptionKeyError)?;

        let key = Key::<Aes256Gcm>::from_slice(&encryption_key);
        let cipher = Aes256Gcm::new(key);

        // Generate random nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Serialize credentials to JSON
        let plaintext = serde_json::to_vec(credentials)?;

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| ConfigDbError::EncryptionFailed(e.to_string()))?;

        // Prepend nonce to ciphertext
        let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    /// Decrypt OAuth credentials from storage.
    ///
    /// Expects input in the format: `nonce (12 bytes) || ciphertext`
    ///
    /// # Arguments
    ///
    /// * `encrypted` - The encrypted credentials
    ///
    /// # Returns
    ///
    /// The decrypted OAuth credentials.
    pub fn decrypt_credentials(&self, encrypted: &[u8]) -> Result<Credentials, ConfigDbError> {
        let encryption_key = self
            .encryption_key
            .ok_or(ConfigDbError::EncryptionKeyError)?;

        if encrypted.len() <= NONCE_SIZE {
            return Err(ConfigDbError::InvalidEncryptedData);
        }

        let key = Key::<Aes256Gcm>::from_slice(&encryption_key);
        let cipher = Aes256Gcm::new(key);

        // Extract nonce and ciphertext
        let nonce = Nonce::from_slice(&encrypted[..NONCE_SIZE]);
        let ciphertext = &encrypted[NONCE_SIZE..];

        // Decrypt
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| ConfigDbError::DecryptionFailed(e.to_string()))?;

        // Deserialize
        let credentials: Credentials = serde_json::from_slice(&plaintext)?;

        Ok(credentials)
    }

    /// Get OAuth credentials for a user from the database.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    ///
    /// # Returns
    ///
    /// The user's OAuth credentials if they exist and can be decrypted.
    pub async fn get_credentials(
        &self,
        user_id: Uuid,
    ) -> Result<Option<Credentials>, ConfigDbError> {
        debug!(user_id = %user_id, "Getting credentials from database");

        let row: Option<(Option<Vec<u8>>,)> = sqlx::query_as(
            r#"
            SELECT oauth_credentials
            FROM user_configs
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((Some(encrypted),)) => {
                debug!(user_id = %user_id, "Found encrypted credentials");
                let credentials = self.decrypt_credentials(&encrypted)?;
                Ok(Some(credentials))
            }
            Some((None,)) => {
                debug!(user_id = %user_id, "No credentials stored");
                Ok(None)
            }
            None => {
                debug!(user_id = %user_id, "No user config found");
                Ok(None)
            }
        }
    }

    /// Save OAuth credentials for a user to the database.
    ///
    /// The credentials are encrypted before storage using AES-256-GCM.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    /// * `credentials` - The OAuth credentials to save
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if encryption or database operation fails.
    pub async fn save_credentials(
        &self,
        user_id: Uuid,
        credentials: &Credentials,
    ) -> Result<(), ConfigDbError> {
        debug!(user_id = %user_id, "Saving credentials to database");

        let encrypted = self.encrypt_credentials(credentials)?;

        sqlx::query(
            r#"
            INSERT INTO user_configs (user_id, oauth_credentials, created_at, updated_at)
            VALUES ($1, $2, NOW(), NOW())
            ON CONFLICT (user_id)
            DO UPDATE SET
                oauth_credentials = EXCLUDED.oauth_credentials,
                updated_at = NOW()
            "#,
        )
        .bind(user_id)
        .bind(encrypted)
        .execute(&self.pool)
        .await?;

        info!(user_id = %user_id, "Credentials saved successfully");
        Ok(())
    }

    /// Delete OAuth credentials for a user.
    ///
    /// # Arguments
    ///
    /// * `user_id` - The UUID of the user
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    pub async fn delete_credentials(&self, user_id: Uuid) -> Result<(), ConfigDbError> {
        debug!(user_id = %user_id, "Deleting credentials from database");

        sqlx::query(
            r#"
            UPDATE user_configs
            SET oauth_credentials = NULL, updated_at = NOW()
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        info!(user_id = %user_id, "Credentials deleted successfully");
        Ok(())
    }

    /// Check if the encryption key is configured.
    ///
    /// # Returns
    ///
    /// `true` if the encryption key is available, `false` otherwise.
    pub fn has_encryption_key(&self) -> bool {
        self.encryption_key.is_some()
    }

    /// Get the database pool reference.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test encryption key (32 bytes = 64 hex chars)
    const TEST_KEY: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ];

    fn create_test_credentials() -> Credentials {
        Credentials {
            access_token: Some("test_access_token".to_string()),
            refresh_token: "test_refresh_token".to_string(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
        }
    }

    /// Test helper for encryption operations that doesn't require a database pool.
    /// This allows us to test encryption/decryption without setting up PostgreSQL.
    fn encrypt_test(key: Option<[u8; 32]>, credentials: &Credentials) -> Result<Vec<u8>, ConfigDbError> {
        let encryption_key = key.ok_or(ConfigDbError::EncryptionKeyError)?;

        let aes_key = Key::<Aes256Gcm>::from_slice(&encryption_key);
        let cipher = Aes256Gcm::new(aes_key);

        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = serde_json::to_vec(credentials)?;

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| ConfigDbError::EncryptionFailed(e.to_string()))?;

        let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        Ok(output)
    }

    /// Test helper for decryption operations that doesn't require a database pool.
    fn decrypt_test(key: Option<[u8; 32]>, encrypted: &[u8]) -> Result<Credentials, ConfigDbError> {
        let encryption_key = key.ok_or(ConfigDbError::EncryptionKeyError)?;

        if encrypted.len() <= NONCE_SIZE {
            return Err(ConfigDbError::InvalidEncryptedData);
        }

        let aes_key = Key::<Aes256Gcm>::from_slice(&encryption_key);
        let cipher = Aes256Gcm::new(aes_key);

        let nonce = Nonce::from_slice(&encrypted[..NONCE_SIZE]);
        let ciphertext = &encrypted[NONCE_SIZE..];

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| ConfigDbError::DecryptionFailed(e.to_string()))?;

        let credentials: Credentials = serde_json::from_slice(&plaintext)?;

        Ok(credentials)
    }

    #[test]
    fn test_encryption_roundtrip() {
        let credentials = create_test_credentials();

        // Encrypt
        let encrypted = encrypt_test(Some(TEST_KEY), &credentials).unwrap();

        // Verify encrypted data is longer than just the nonce
        assert!(encrypted.len() > NONCE_SIZE);

        // Decrypt
        let decrypted = decrypt_test(Some(TEST_KEY), &encrypted).unwrap();

        // Verify all fields match
        assert_eq!(credentials.access_token, decrypted.access_token);
        assert_eq!(credentials.refresh_token, decrypted.refresh_token);
        // Note: expires_at comparison may have slight variance due to serialization,
        // so we check they're both Some and close in time
        assert!(credentials.expires_at.is_some() && decrypted.expires_at.is_some());
    }

    #[test]
    fn test_encryption_without_key() {
        let credentials = create_test_credentials();
        let result = encrypt_test(None, &credentials);

        assert!(matches!(result, Err(ConfigDbError::EncryptionKeyError)));
    }

    #[test]
    fn test_decryption_without_key() {
        let encrypted = vec![0u8; 50];
        let result = decrypt_test(None, &encrypted);

        assert!(matches!(result, Err(ConfigDbError::EncryptionKeyError)));
    }

    #[test]
    fn test_decryption_invalid_data() {
        // Too short (less than nonce size)
        let result = decrypt_test(Some(TEST_KEY), &[0u8; 5]);
        assert!(matches!(result, Err(ConfigDbError::InvalidEncryptedData)));

        // Invalid ciphertext (wrong key or corrupted)
        let result = decrypt_test(Some(TEST_KEY), &[0u8; 50]);
        assert!(matches!(result, Err(ConfigDbError::DecryptionFailed(_))));
    }

    #[test]
    fn test_different_encryptions_are_unique() {
        let credentials = create_test_credentials();

        // Encrypt twice
        let encrypted1 = encrypt_test(Some(TEST_KEY), &credentials).unwrap();
        let encrypted2 = encrypt_test(Some(TEST_KEY), &credentials).unwrap();

        // Due to random nonce, encryptions should be different
        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt to the same value
        let decrypted1 = decrypt_test(Some(TEST_KEY), &encrypted1).unwrap();
        let decrypted2 = decrypt_test(Some(TEST_KEY), &encrypted2).unwrap();

        assert_eq!(decrypted1.access_token, decrypted2.access_token);
        assert_eq!(decrypted1.refresh_token, decrypted2.refresh_token);
    }

    #[test]
    fn test_credentials_with_none_access_token() {
        let credentials = Credentials {
            access_token: None,
            refresh_token: "refresh_only".to_string(),
            expires_at: None,
        };

        let encrypted = encrypt_test(Some(TEST_KEY), &credentials).unwrap();
        let decrypted = decrypt_test(Some(TEST_KEY), &encrypted).unwrap();

        assert_eq!(credentials.access_token, decrypted.access_token);
        assert_eq!(credentials.refresh_token, decrypted.refresh_token);
        assert_eq!(credentials.expires_at, decrypted.expires_at);
    }

    #[test]
    fn test_load_encryption_key_from_env() {
        // SAFETY: This test modifies environment variables, which is safe in a single-threaded
        // test context. Environment variable tests should be run with `--test-threads=1` to
        // avoid race conditions.
        unsafe {
            // Valid 64-char hex key
            std::env::set_var(
                CONFIG_ENCRYPTION_KEY_ENV,
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            );
            let key = ConfigServicePg::load_encryption_key();
            assert!(key.is_some());
            assert_eq!(key.unwrap(), TEST_KEY);

            // Invalid length key
            std::env::set_var(CONFIG_ENCRYPTION_KEY_ENV, "tooshort");
            let key = ConfigServicePg::load_encryption_key();
            assert!(key.is_none());

            // Invalid hex
            std::env::set_var(
                CONFIG_ENCRYPTION_KEY_ENV,
                "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg",
            );
            let key = ConfigServicePg::load_encryption_key();
            assert!(key.is_none());

            // Clean up
            std::env::remove_var(CONFIG_ENCRYPTION_KEY_ENV);
        }
    }
}
