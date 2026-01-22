/**
 * Load Test Configuration
 *
 * Shared configuration for all load test scenarios.
 * Values can be overridden via environment variables.
 */

// Base URL for the API - required
export const BASE_URL = __ENV.BASE_URL || 'http://localhost:8081';

// JWT Secret for token generation in multi-user mode
export const JWT_SECRET = __ENV.JWT_SECRET || 'test-jwt-secret-for-load-testing';

// Response time thresholds (in milliseconds)
export const THRESHOLDS = {
  // API response times
  http_req_duration_p95: parseInt(__ENV.RESPONSE_TIME_P95) || 200,
  http_req_duration_p99: parseInt(__ENV.RESPONSE_TIME_P99) || 500,

  // Database query times (measured via API response patterns)
  db_query_p99: parseInt(__ENV.DB_QUERY_P99) || 100,

  // WebSocket/PTY latency
  ws_latency_p95: parseInt(__ENV.WS_LATENCY_P95) || 50,

  // Error rate threshold
  error_rate_max: parseFloat(__ENV.ERROR_RATE_MAX) || 0.01, // 1%
};

// Default load test stages for concurrent user tests
export const CONCURRENT_USER_STAGES = [
  { duration: '2m', target: 100 }, // Ramp up to 100 users over 2 minutes
  { duration: '5m', target: 100 }, // Stay at 100 users for 5 minutes
  { duration: '1m', target: 0 },   // Ramp down to 0 users
];

// Lighter stages for quick validation
export const SMOKE_TEST_STAGES = [
  { duration: '30s', target: 5 },  // Ramp up to 5 users
  { duration: '1m', target: 5 },   // Stay at 5 users
  { duration: '30s', target: 0 },  // Ramp down
];

// Stress test stages (beyond normal capacity)
export const STRESS_TEST_STAGES = [
  { duration: '2m', target: 100 },  // Normal load
  { duration: '2m', target: 200 },  // Beyond normal capacity
  { duration: '3m', target: 200 },  // Stay at stress level
  { duration: '2m', target: 100 },  // Scale down to recovery
  { duration: '1m', target: 0 },    // Ramp down
];

// Spike test stages (sudden traffic burst)
export const SPIKE_TEST_STAGES = [
  { duration: '30s', target: 10 },  // Normal load
  { duration: '10s', target: 150 }, // Sudden spike
  { duration: '1m', target: 150 },  // Maintain spike
  { duration: '10s', target: 10 },  // Drop back
  { duration: '1m', target: 10 },   // Recovery period
  { duration: '30s', target: 0 },   // Ramp down
];

// Standard k6 thresholds configuration
export const STANDARD_THRESHOLDS = {
  http_req_duration: [
    `p(95)<${THRESHOLDS.http_req_duration_p95}`,
    `p(99)<${THRESHOLDS.http_req_duration_p99}`,
  ],
  http_req_failed: [`rate<${THRESHOLDS.error_rate_max}`],
  http_reqs: ['rate>10'], // Minimum 10 requests per second
};

// Thresholds for health checks (should be faster)
export const HEALTH_THRESHOLDS = {
  http_req_duration: ['p(95)<50', 'p(99)<100'],
  http_req_failed: ['rate<0.001'],
};

// API endpoints
export const ENDPOINTS = {
  health: `${BASE_URL}/api/health`,
  projects: `${BASE_URL}/api/projects`,
  tasks: (projectId) => `${BASE_URL}/api/tasks?project_id=${projectId}`,
  terminal: `${BASE_URL.replace('http', 'ws')}/api/terminal`,
  config: `${BASE_URL}/api/config`,
  filesystem: `${BASE_URL}/api/filesystem`,
};

// Default HTTP parameters
export const DEFAULT_PARAMS = {
  headers: {
    'Content-Type': 'application/json',
    'Accept': 'application/json',
  },
  timeout: '30s',
};

// Helper to create authenticated params
export function getAuthParams(token) {
  return {
    ...DEFAULT_PARAMS,
    headers: {
      ...DEFAULT_PARAMS.headers,
      'Authorization': `Bearer ${token}`,
    },
  };
}

// Test data generators
export function generateProjectName() {
  const timestamp = Date.now();
  const random = Math.random().toString(36).substring(7);
  return `load-test-project-${timestamp}-${random}`;
}

export function generateTaskTitle() {
  const timestamp = Date.now();
  const random = Math.random().toString(36).substring(7);
  return `Load Test Task ${timestamp}-${random}`;
}

export function generateUserId() {
  // Generate a UUID v4-like string
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = Math.random() * 16 | 0;
    const v = c === 'x' ? r : (r & 0x3 | 0x8);
    return v.toString(16);
  });
}
