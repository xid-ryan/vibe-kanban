/**
 * Health Check Load Test
 *
 * Tests the /api/health endpoint as a baseline performance metric.
 * This endpoint requires no authentication and should have minimal latency.
 *
 * Usage:
 *   k6 run scenarios/health.js
 *
 * Expected Results:
 *   - p95 response time < 50ms
 *   - p99 response time < 100ms
 *   - Error rate < 0.1%
 */

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';
import { ENDPOINTS, HEALTH_THRESHOLDS, SMOKE_TEST_STAGES } from '../config.js';

// Custom metrics
const healthCheckDuration = new Trend('health_check_duration', true);
const healthCheckErrors = new Rate('health_check_errors');

// Test configuration
export const options = {
  stages: SMOKE_TEST_STAGES,
  thresholds: HEALTH_THRESHOLDS,
  // Tags for filtering in metrics
  tags: {
    test_type: 'health',
  },
};

// Setup - runs once before the test
export function setup() {
  console.log(`Testing health endpoint at: ${ENDPOINTS.health}`);
  return { startTime: Date.now() };
}

// Main test function - runs for each virtual user
export default function () {
  const response = http.get(ENDPOINTS.health, {
    tags: { endpoint: 'health' },
    timeout: '10s',
  });

  // Record custom metrics
  healthCheckDuration.add(response.timings.duration);

  // Validate response
  const checkResult = check(response, {
    'status is 200': (r) => r.status === 200,
    'response time < 100ms': (r) => r.timings.duration < 100,
    'response has success field': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.success === true;
      } catch {
        return false;
      }
    },
    'response data is OK': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.data === 'OK';
      } catch {
        return false;
      }
    },
  });

  // Track errors
  if (!checkResult) {
    healthCheckErrors.add(1);
  } else {
    healthCheckErrors.add(0);
  }

  // Small think time between requests
  sleep(0.1 + Math.random() * 0.2);
}

// Teardown - runs once after the test
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`Health check test completed in ${duration.toFixed(2)} seconds`);
}

// Handle summary report
export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'health-check',
    metrics: {
      requests: data.metrics.http_reqs?.values?.count || 0,
      avgDuration: data.metrics.http_req_duration?.values?.avg?.toFixed(2) || 'N/A',
      p95Duration: data.metrics.http_req_duration?.values?.['p(95)']?.toFixed(2) || 'N/A',
      p99Duration: data.metrics.http_req_duration?.values?.['p(99)']?.toFixed(2) || 'N/A',
      errorRate: ((data.metrics.http_req_failed?.values?.rate || 0) * 100).toFixed(2) + '%',
    },
    thresholds: {
      passed: Object.values(data.root_group?.checks || {}).every(c => c.passes > 0),
    },
  };

  console.log('\n=== Health Check Test Summary ===');
  console.log(JSON.stringify(summary, null, 2));

  return {
    stdout: JSON.stringify(summary, null, 2),
  };
}
