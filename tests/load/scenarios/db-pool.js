/**
 * Database Connection Pool Behavior Test
 *
 * Tests database connection pool behavior under various load patterns.
 * Validates pool exhaustion handling, recovery, and query performance.
 *
 * Test Scenarios:
 *   1. Burst load - sudden spike of concurrent requests
 *   2. Sustained load - steady high throughput
 *   3. Recovery - behavior after pool exhaustion
 *
 * Usage:
 *   k6 run scenarios/db-pool.js
 *
 * Expected Results:
 *   - Database queries complete < 100ms at p99
 *   - No connection timeout errors under normal load
 *   - Graceful degradation under extreme load
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';
import {
  ENDPOINTS,
  SPIKE_TEST_STAGES,
  getAuthParams,
  generateProjectName,
  THRESHOLDS,
} from '../config.js';
import { getDefaultTokenPool } from '../utils/jwt.js';

// Custom metrics for database behavior analysis
const dbQueryDuration = new Trend('db_query_duration', true);
const connectionErrors = new Rate('connection_errors');
const timeoutErrors = new Counter('timeout_errors');
const poolExhaustionEvents = new Counter('pool_exhaustion_events');
const successfulQueries = new Counter('successful_queries');

// Test configuration - uses spike stages to stress the connection pool
export const options = {
  stages: SPIKE_TEST_STAGES,
  thresholds: {
    http_req_duration: ['p(95)<500', 'p(99)<1000'], // More lenient for stress test
    db_query_duration: [`p(99)<${THRESHOLDS.db_query_p99}`],
    connection_errors: ['rate<0.05'], // Allow up to 5% during spikes
    http_req_failed: ['rate<0.05'],
  },
  tags: {
    test_type: 'db-pool',
  },
};

// Setup
export function setup() {
  console.log('=== Database Connection Pool Behavior Test ===');
  console.log('Testing connection pool under spike load patterns');

  // Verify server health
  const response = http.get(ENDPOINTS.health);
  if (response.status !== 200) {
    throw new Error(`Server health check failed: ${response.status}`);
  }

  return {
    startTime: Date.now(),
  };
}

// Execute a database-heavy operation
function executeDbOperation(authParams, operationType) {
  const startTime = Date.now();
  let response;
  let endpoint;

  switch (operationType) {
    case 'list':
      // List projects - involves DB query
      endpoint = ENDPOINTS.projects;
      response = http.get(endpoint, {
        ...authParams,
        tags: { db_operation: 'list' },
        timeout: '10s',
      });
      break;

    case 'create':
      // Create project - involves DB insert
      endpoint = ENDPOINTS.projects;
      response = http.post(
        endpoint,
        JSON.stringify({
          name: generateProjectName(),
          repositories: [],
        }),
        {
          ...authParams,
          tags: { db_operation: 'create' },
          timeout: '10s',
        }
      );
      break;

    case 'config':
      // Get config - involves DB query
      endpoint = ENDPOINTS.config;
      response = http.get(endpoint, {
        ...authParams,
        tags: { db_operation: 'config' },
        timeout: '10s',
      });
      break;

    default:
      // Default to health check
      endpoint = ENDPOINTS.health;
      response = http.get(endpoint, { timeout: '5s' });
  }

  const duration = Date.now() - startTime;
  dbQueryDuration.add(duration);

  return { response, duration, endpoint };
}

// Check for connection pool issues in response
function analyzeResponse(response, duration) {
  // Check for timeout
  if (response.error && response.error.includes('timeout')) {
    timeoutErrors.add(1);
    connectionErrors.add(1);
    return { success: false, reason: 'timeout' };
  }

  // Check for connection errors
  if (response.status === 0) {
    connectionErrors.add(1);
    return { success: false, reason: 'connection_failed' };
  }

  // Check for server errors that might indicate pool exhaustion
  if (response.status === 503 || response.status === 500) {
    const body = response.body?.toLowerCase() || '';
    if (body.includes('connection') || body.includes('pool') || body.includes('database')) {
      poolExhaustionEvents.add(1);
      connectionErrors.add(1);
      return { success: false, reason: 'pool_exhausted' };
    }
    connectionErrors.add(1);
    return { success: false, reason: 'server_error' };
  }

  // Check for client errors
  if (response.status >= 400 && response.status < 500) {
    // 401/403 are expected for auth issues, not DB issues
    if (response.status === 401 || response.status === 403) {
      return { success: true, reason: 'auth_expected' };
    }
    return { success: false, reason: 'client_error' };
  }

  // Success
  successfulQueries.add(1);
  return { success: true, reason: 'ok' };
}

// Main test function
export default function () {
  const tokenPool = getDefaultTokenPool();
  const { token } = tokenPool.getRandom();
  const authParams = getAuthParams(token);

  // Cycle through different DB operations
  const operations = ['list', 'create', 'config', 'list', 'list'];
  const operation = operations[__ITER % operations.length];

  group(`DB Pool - ${operation}`, () => {
    const { response, duration } = executeDbOperation(authParams, operation);
    const analysis = analyzeResponse(response, duration);

    // Validation checks
    check(response, {
      'response received': (r) => r.status > 0,
      'no timeout': (r) => !r.error?.includes('timeout'),
      'no connection error': (r) => r.status !== 0,
      'database query < 200ms': () => duration < 200,
      'database query < 500ms': () => duration < 500,
    });

    // Log slow queries or errors
    if (!analysis.success) {
      console.log(
        `[VU:${__VU}] ${operation} failed: ${analysis.reason} ` +
        `(status: ${response.status}, duration: ${duration}ms)`
      );
    } else if (duration > 500) {
      console.log(
        `[VU:${__VU}] Slow ${operation} query: ${duration}ms ` +
        `(status: ${response.status})`
      );
    }
  });

  // Minimal sleep during spike phases, longer during normal
  // This helps stress the connection pool during spikes
  const currentVUs = __VU;
  if (currentVUs > 100) {
    // During spike - minimal delay
    sleep(0.1);
  } else {
    // Normal load - standard delay
    sleep(0.5 + Math.random() * 0.5);
  }
}

// Teardown
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\nDatabase pool test completed in ${duration.toFixed(2)} seconds`);
}

// Summary handler
export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'db-pool-behavior',
    description: 'Tests database connection pool under spike load patterns',
    results: {
      totalRequests: data.metrics.http_reqs?.values?.count || 0,
      requestsPerSecond: data.metrics.http_reqs?.values?.rate?.toFixed(2) || 'N/A',
      successfulQueries: data.metrics.successful_queries?.values?.count || 0,
    },
    responseTime: {
      avg: data.metrics.http_req_duration?.values?.avg?.toFixed(2) + 'ms',
      p95: data.metrics.http_req_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      p99: data.metrics.http_req_duration?.values?.['p(99)']?.toFixed(2) + 'ms',
    },
    dbQueryTime: {
      avg: data.metrics.db_query_duration?.values?.avg?.toFixed(2) + 'ms',
      p95: data.metrics.db_query_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      p99: data.metrics.db_query_duration?.values?.['p(99)']?.toFixed(2) + 'ms',
      max: data.metrics.db_query_duration?.values?.max?.toFixed(2) + 'ms',
    },
    errors: {
      connectionErrorRate: ((data.metrics.connection_errors?.values?.rate || 0) * 100).toFixed(2) + '%',
      timeoutCount: data.metrics.timeout_errors?.values?.count || 0,
      poolExhaustionEvents: data.metrics.pool_exhaustion_events?.values?.count || 0,
      httpFailRate: ((data.metrics.http_req_failed?.values?.rate || 0) * 100).toFixed(2) + '%',
    },
    analysis: {
      dbQueryP99UnderThreshold: (data.metrics.db_query_duration?.values?.['p(99)'] || 0) < THRESHOLDS.db_query_p99,
      connectionPoolStable: (data.metrics.pool_exhaustion_events?.values?.count || 0) === 0,
      acceptableErrorRate: (data.metrics.connection_errors?.values?.rate || 0) < 0.05,
    },
  };

  // Console output
  console.log('\n' + '='.repeat(60));
  console.log('DATABASE CONNECTION POOL TEST RESULTS');
  console.log('='.repeat(60));
  console.log(`Total Requests:        ${summary.results.totalRequests}`);
  console.log(`Requests/sec:          ${summary.results.requestsPerSecond}`);
  console.log(`Successful Queries:    ${summary.results.successfulQueries}`);
  console.log('-'.repeat(60));
  console.log('DB Query Performance:');
  console.log(`  Average:             ${summary.dbQueryTime.avg}`);
  console.log(`  p95:                 ${summary.dbQueryTime.p95}`);
  console.log(`  p99:                 ${summary.dbQueryTime.p99}`);
  console.log(`  Max:                 ${summary.dbQueryTime.max}`);
  console.log('-'.repeat(60));
  console.log('Connection Pool Health:');
  console.log(`  Connection Errors:   ${summary.errors.connectionErrorRate}`);
  console.log(`  Timeouts:            ${summary.errors.timeoutCount}`);
  console.log(`  Pool Exhaustion:     ${summary.errors.poolExhaustionEvents}`);
  console.log('-'.repeat(60));
  console.log('Analysis:');
  console.log(`  DB Query p99 OK:     ${summary.analysis.dbQueryP99UnderThreshold ? 'YES' : 'NO'}`);
  console.log(`  Pool Stable:         ${summary.analysis.connectionPoolStable ? 'YES' : 'NO'}`);
  console.log(`  Error Rate OK:       ${summary.analysis.acceptableErrorRate ? 'YES' : 'NO'}`);
  console.log('='.repeat(60));

  return {
    stdout: JSON.stringify(summary, null, 2),
    'results/db-pool.json': JSON.stringify(summary, null, 2),
  };
}
