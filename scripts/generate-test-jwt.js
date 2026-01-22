#!/usr/bin/env node

/**
 * Generate a test JWT token for local Kubernetes mode testing
 *
 * Usage:
 *   node scripts/generate-test-jwt.js
 *   node scripts/generate-test-jwt.js <user_id> <email>
 */

const crypto = require('crypto');

// Read JWT_SECRET from environment or use default
const JWT_SECRET = process.env.JWT_SECRET || 'your-secret-key-for-testing-min-32-chars-long';

// Parse command line arguments
const userId = process.argv[2] || crypto.randomUUID();
const email = process.argv[3] || `user-${userId.split('-')[0]}@example.com`;

// JWT header and payload
const header = {
  alg: 'HS256',
  typ: 'JWT'
};

const payload = {
  sub: userId,  // user_id
  email: email,
  iat: Math.floor(Date.now() / 1000),
  exp: Math.floor(Date.now() / 1000) + (24 * 60 * 60) // 24 hours
};

// Base64URL encoding
function base64urlEncode(str) {
  return Buffer.from(str)
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=/g, '');
}

// Create JWT
const headerEncoded = base64urlEncode(JSON.stringify(header));
const payloadEncoded = base64urlEncode(JSON.stringify(payload));
const signatureInput = `${headerEncoded}.${payloadEncoded}`;

const signature = crypto
  .createHmac('sha256', JWT_SECRET)
  .update(signatureInput)
  .digest('base64')
  .replace(/\+/g, '-')
  .replace(/\//g, '_')
  .replace(/=/g, '');

const jwt = `${headerEncoded}.${payloadEncoded}.${signature}`;

// Output
console.log('\n=== Test JWT Token Generated ===\n');
console.log('User ID:', userId);
console.log('Email:', email);
console.log('\nJWT Token:');
console.log(jwt);
console.log('\n=== Usage ===\n');
console.log('Set Authorization header:');
console.log(`Authorization: Bearer ${jwt}`);
console.log('\nOr for WebSocket (query parameter):');
console.log(`ws://localhost:8081/terminal?token=${jwt}`);
console.log('\nTest with curl:');
console.log(`curl -H "Authorization: Bearer ${jwt}" http://localhost:8081/projects`);
console.log('\n');
