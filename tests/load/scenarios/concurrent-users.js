/**
 * Concurrent Users Load Test
 *
 * Simulates 100 concurrent users with mixed workloads to validate
 * the system can handle the target user count specified in requirements.
 *
 * Workload Distribution:
 *   - 60% Read operations (list projects, get config)
 *   - 25% Write operations (create projects)
 *   - 15% Delete operations (cleanup)
 *
 * Usage:
 *   k6 run scenarios/concurrent-users.js
 *   k6 run --vus 50 --duration 5m scenarios/concurrent-users.js
 *
 * Expected Results:
 *   - 100 concurrent users supported
 *   - p95 response time < 200ms
 *   - Error rate < 1%
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter, Gauge } from 'k6/metrics';
import {
  ENDPOINTS,
  STANDARD_THRESHOLDS,
  CONCURRENT_USER_STAGES,
  getAuthParams,
  generateProjectName,
} from '../config.js';
import { getDefaultTokenPool } from '../utils/jwt.js';

// Custom metrics for detailed analysis
const readOperationDuration = new Trend('read_operation_duration', true);
const writeOperationDuration = new Trend('write_operation_duration', true);
const deleteOperationDuration = new Trend('delete_operation_duration', true);
const operationErrors = new Rate('operation_errors');
const activeProjects = new Counter('active_projects');
const concurrentUsers = new Gauge('concurrent_users_gauge');

// Track created projects for cleanup
const userProjects = new Map();

// Test configuration
export const options = {
  stages: CONCURRENT_USER_STAGES,
  thresholds: {
    ...STANDARD_THRESHOLDS,
    read_operation_duration: ['p(95)<200', 'p(99)<400'],
    write_operation_duration: ['p(95)<300', 'p(99)<500'],
    delete_operation_duration: ['p(95)<200', 'p(99)<400'],
    operation_errors: ['rate<0.01'], // Less than 1% error rate
  },
  tags: {
    test_type: 'concurrent-users',
  },
  // Summary options
  summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(90)', 'p(95)', 'p(99)'],
};

// Setup
export function setup() {
  console.log('=== Concurrent Users Load Test ===');
  console.log(`Target: ${CONCURRENT_USER_STAGES[0].target} users`);
  console.log(`Base URL: ${ENDPOINTS.health.replace('/health', '')}`);

  // Verify server is healthy
  const response = http.get(ENDPOINTS.health);
  if (response.status !== 200) {
    throw new Error(`Server health check failed: ${response.status}`);
  }

  return {
    startTime: Date.now(),
    targetUsers: CONCURRENT_USER_STAGES.find(s => s.target > 0)?.target || 100,
  };
}

// Weighted random selection for workload distribution
function selectOperation() {
  const rand = Math.random();
  if (rand < 0.60) return 'read';      // 60% read
  if (rand < 0.85) return 'write';     // 25% write
  return 'delete';                      // 15% delete
}

// Read operation - list projects or get config
function performReadOperation(authParams) {
  const start = Date.now();

  // Randomly choose between list projects and get config
  const endpoint = Math.random() < 0.7 ? ENDPOINTS.projects : ENDPOINTS.config;
  const response = http.get(endpoint, {
    ...authParams,
    tags: { operation: 'read' },
  });

  readOperationDuration.add(Date.now() - start);

  const success = check(response, {
    'read: status is 200': (r) => r.status === 200,
    'read: response time < 300ms': (r) => r.timings.duration < 300,
    'read: valid response': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.success !== undefined;
      } catch {
        return false;
      }
    },
  });

  if (!success) {
    operationErrors.add(1);
  }

  return success;
}

// Write operation - create a project
function performWriteOperation(authParams, userId) {
  const start = Date.now();

  const projectName = generateProjectName();
  const payload = JSON.stringify({
    name: projectName,
    repositories: [],
  });

  const response = http.post(ENDPOINTS.projects, payload, {
    ...authParams,
    tags: { operation: 'write' },
  });

  writeOperationDuration.add(Date.now() - start);

  const success = check(response, {
    'write: status is 200': (r) => r.status === 200,
    'write: response time < 500ms': (r) => r.timings.duration < 500,
    'write: project created': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.success === true && body.data?.id;
      } catch {
        return false;
      }
    },
  });

  if (success) {
    try {
      const body = JSON.parse(response.body);
      const projectId = body.data?.id;
      if (projectId) {
        activeProjects.add(1);
        // Track for potential cleanup
        if (!userProjects.has(userId)) {
          userProjects.set(userId, []);
        }
        userProjects.get(userId).push(projectId);
      }
    } catch {
      // Ignore parse errors
    }
  } else {
    operationErrors.add(1);
  }

  return success;
}

// Delete operation - delete a project
function performDeleteOperation(authParams, userId) {
  // Get a project to delete from this user's tracked projects
  const projects = userProjects.get(userId) || [];
  if (projects.length === 0) {
    // No projects to delete, do a read instead
    return performReadOperation(authParams);
  }

  const projectId = projects.pop();
  const start = Date.now();

  const response = http.del(`${ENDPOINTS.projects}/${projectId}`, null, {
    ...authParams,
    tags: { operation: 'delete' },
  });

  deleteOperationDuration.add(Date.now() - start);

  const success = check(response, {
    'delete: status is 200 or 202 or 404': (r) =>
      r.status === 200 || r.status === 202 || r.status === 404,
    'delete: response time < 500ms': (r) => r.timings.duration < 500,
  });

  if (!success) {
    operationErrors.add(1);
  }

  return success;
}

// Main test function
export default function (data) {
  // Get token for this virtual user
  const tokenPool = getDefaultTokenPool();
  const { token, userId } = tokenPool.getForVU(__VU);
  const authParams = getAuthParams(token);

  // Track concurrent users
  concurrentUsers.add(__VU);

  // Select operation based on weighted distribution
  const operation = selectOperation();

  group(`User ${__VU} - ${operation} operation`, () => {
    switch (operation) {
      case 'read':
        performReadOperation(authParams);
        break;
      case 'write':
        performWriteOperation(authParams, userId);
        break;
      case 'delete':
        performDeleteOperation(authParams, userId);
        break;
    }
  });

  // Variable think time to simulate realistic user behavior
  // Shorter for read operations, longer for write operations
  const thinkTime = operation === 'read' ? 0.5 : 1.5;
  sleep(thinkTime + Math.random() * thinkTime);
}

// Teardown - cleanup and report
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\nConcurrent users test completed in ${duration.toFixed(2)} seconds`);
  console.log(`Target users: ${data.targetUsers}`);
}

// Summary handler
export function handleSummary(data) {
  const http_reqs = data.metrics.http_reqs?.values || {};
  const http_req_duration = data.metrics.http_req_duration?.values || {};
  const http_req_failed = data.metrics.http_req_failed?.values || {};

  const summary = {
    timestamp: new Date().toISOString(),
    test: 'concurrent-users',
    config: {
      targetUsers: 100,
      duration: '8 minutes total',
      workloadDistribution: {
        read: '60%',
        write: '25%',
        delete: '15%',
      },
    },
    results: {
      totalRequests: http_reqs.count || 0,
      requestsPerSecond: http_reqs.rate?.toFixed(2) || 'N/A',
      responseTime: {
        avg: http_req_duration.avg?.toFixed(2) + 'ms',
        min: http_req_duration.min?.toFixed(2) + 'ms',
        max: http_req_duration.max?.toFixed(2) + 'ms',
        p90: http_req_duration['p(90)']?.toFixed(2) + 'ms',
        p95: http_req_duration['p(95)']?.toFixed(2) + 'ms',
        p99: http_req_duration['p(99)']?.toFixed(2) + 'ms',
      },
      errorRate: ((http_req_failed.rate || 0) * 100).toFixed(2) + '%',
    },
    operationMetrics: {
      read: {
        p95: data.metrics.read_operation_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      },
      write: {
        p95: data.metrics.write_operation_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      },
      delete: {
        p95: data.metrics.delete_operation_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      },
    },
    thresholds: {
      allPassed: !Object.entries(data.metrics).some(
        ([name, metric]) => metric.thresholds && Object.values(metric.thresholds).some(t => !t.ok)
      ),
    },
  };

  // Console output
  console.log('\n' + '='.repeat(60));
  console.log('CONCURRENT USERS TEST RESULTS');
  console.log('='.repeat(60));
  console.log(`Total Requests:     ${summary.results.totalRequests}`);
  console.log(`Requests/sec:       ${summary.results.requestsPerSecond}`);
  console.log(`Error Rate:         ${summary.results.errorRate}`);
  console.log('-'.repeat(60));
  console.log('Response Times:');
  console.log(`  Average:          ${summary.results.responseTime.avg}`);
  console.log(`  p95:              ${summary.results.responseTime.p95}`);
  console.log(`  p99:              ${summary.results.responseTime.p99}`);
  console.log('-'.repeat(60));
  console.log('Operation p95 Times:');
  console.log(`  Read:             ${summary.operationMetrics.read.p95}`);
  console.log(`  Write:            ${summary.operationMetrics.write.p95}`);
  console.log(`  Delete:           ${summary.operationMetrics.delete.p95}`);
  console.log('-'.repeat(60));
  console.log(`All Thresholds Passed: ${summary.thresholds.allPassed ? 'YES' : 'NO'}`);
  console.log('='.repeat(60));

  return {
    stdout: JSON.stringify(summary, null, 2),
    'results/concurrent-users.json': JSON.stringify(summary, null, 2),
  };
}
