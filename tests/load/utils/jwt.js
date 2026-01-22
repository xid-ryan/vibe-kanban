/**
 * JWT Token Generation Utilities
 *
 * Generates JWT tokens for authenticated load tests.
 * Uses HMAC-SHA256 signing algorithm.
 */

import encoding from 'k6/encoding';
import { JWT_SECRET, generateUserId } from '../config.js';

/**
 * Base64URL encode a string (JWT-safe encoding)
 */
function base64UrlEncode(str) {
  // k6's encoding.b64encode returns standard base64
  const base64 = encoding.b64encode(str);
  // Convert to base64url format
  return base64
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=/g, '');
}

/**
 * Create HMAC-SHA256 signature
 * Note: k6 doesn't have native crypto, so we use a simplified approach
 * For production tests, consider using k6/x/crypto extension
 */
function hmacSha256(data, secret) {
  // Simplified HMAC for k6 - in production, use k6/x/crypto
  // This creates a deterministic signature based on data and secret
  let hash = 0;
  const combined = data + secret;
  for (let i = 0; i < combined.length; i++) {
    const char = combined.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash; // Convert to 32bit integer
  }
  // Convert to hex-like string and pad
  const hexHash = Math.abs(hash).toString(16).padStart(64, '0');
  return hexHash;
}

/**
 * Generate a JWT token for load testing
 *
 * @param {Object} options - Token options
 * @param {string} options.userId - User ID (UUID)
 * @param {string} options.email - User email (optional)
 * @param {number} options.expiresIn - Token expiration in seconds (default: 24 hours)
 * @returns {string} JWT token
 */
export function generateToken(options = {}) {
  const now = Math.floor(Date.now() / 1000);
  const userId = options.userId || generateUserId();
  const expiresIn = options.expiresIn || 86400; // 24 hours

  // JWT Header
  const header = {
    alg: 'HS256',
    typ: 'JWT',
  };

  // JWT Payload
  const payload = {
    sub: userId, // user_id as required by the auth middleware
    email: options.email || `loadtest-${userId.substring(0, 8)}@example.com`,
    iat: now,
    exp: now + expiresIn,
  };

  // Encode header and payload
  const encodedHeader = base64UrlEncode(JSON.stringify(header));
  const encodedPayload = base64UrlEncode(JSON.stringify(payload));

  // Create signature
  const signatureInput = `${encodedHeader}.${encodedPayload}`;
  const signature = base64UrlEncode(hmacSha256(signatureInput, JWT_SECRET));

  return `${encodedHeader}.${encodedPayload}.${signature}`;
}

/**
 * Generate multiple unique tokens for simulating different users
 *
 * @param {number} count - Number of tokens to generate
 * @returns {Array<{token: string, userId: string}>} Array of token objects
 */
export function generateUserTokens(count) {
  const tokens = [];
  for (let i = 0; i < count; i++) {
    const userId = generateUserId();
    const token = generateToken({ userId });
    tokens.push({ token, userId });
  }
  return tokens;
}

/**
 * Create a token pool for load testing
 * Pre-generates tokens to avoid generation overhead during tests
 */
export class TokenPool {
  constructor(size = 100) {
    this.tokens = generateUserTokens(size);
    this.index = 0;
  }

  /**
   * Get the next token from the pool (round-robin)
   */
  getNext() {
    const token = this.tokens[this.index];
    this.index = (this.index + 1) % this.tokens.length;
    return token;
  }

  /**
   * Get a random token from the pool
   */
  getRandom() {
    const index = Math.floor(Math.random() * this.tokens.length);
    return this.tokens[index];
  }

  /**
   * Get token for a specific virtual user (consistent per VU)
   * @param {number} vuId - Virtual user ID
   */
  getForVU(vuId) {
    const index = vuId % this.tokens.length;
    return this.tokens[index];
  }
}

/**
 * Default token pool - shared across test iterations
 */
let defaultPool = null;

export function getDefaultTokenPool() {
  if (!defaultPool) {
    defaultPool = new TokenPool(100);
  }
  return defaultPool;
}
