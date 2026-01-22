/**
 * Main Load Test Entry Point
 *
 * Runs all load test scenarios in sequence or allows selection
 * of specific scenarios via environment variables.
 *
 * Usage:
 *   k6 run main.js                    # Run all scenarios
 *   k6 run -e SCENARIO=health main.js # Run specific scenario
 *   k6 run -e SCENARIO=concurrent main.js
 *   k6 run -e SCENARIO=db-pool main.js
 *   k6 run -e SCENARIO=websocket main.js
 *
 * Environment Variables:
 *   BASE_URL     - API base URL (default: http://localhost:8081)
 *   JWT_SECRET   - JWT signing secret for token generation
 *   SCENARIO     - Specific scenario to run (health, crud, concurrent, db-pool, websocket)
 *   TEST_MODE    - Test intensity: smoke, load, stress, spike (default: load)
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';
import {
  ENDPOINTS,
  STANDARD_THRESHOLDS,
  CONCURRENT_USER_STAGES,
  SMOKE_TEST_STAGES,
  STRESS_TEST_STAGES,
  SPIKE_TEST_STAGES,
  getAuthParams,
  generateProjectName,
} from './config.js';
import { getDefaultTokenPool } from './utils/jwt.js';

// Custom metrics
const scenarioSuccess = new Rate('scenario_success');
const overallDuration = new Trend('overall_duration', true);
const totalErrors = new Counter('total_errors');

// Get test configuration from environment
const SCENARIO = __ENV.SCENARIO || 'all';
const TEST_MODE = __ENV.TEST_MODE || 'load';

// Select stages based on test mode
function getStages() {
  switch (TEST_MODE) {
    case 'smoke':
      return SMOKE_TEST_STAGES;
    case 'stress':
      return STRESS_TEST_STAGES;
    case 'spike':
      return SPIKE_TEST_STAGES;
    case 'load':
    default:
      return CONCURRENT_USER_STAGES;
  }
}

// Test configuration
export const options = {
  stages: getStages(),
  thresholds: {
    ...STANDARD_THRESHOLDS,
    scenario_success: ['rate>0.95'], // 95% of scenarios must pass
  },
  tags: {
    test_type: 'main',
    scenario: SCENARIO,
    mode: TEST_MODE,
  },
  summaryTrendStats: ['avg', 'min', 'med', 'max', 'p(90)', 'p(95)', 'p(99)'],
};

// Setup - verify connectivity
export function setup() {
  console.log('='.repeat(60));
  console.log('VIBE-KANBAN LOAD TEST SUITE');
  console.log('='.repeat(60));
  console.log(`Scenario:   ${SCENARIO}`);
  console.log(`Test Mode:  ${TEST_MODE}`);
  console.log(`Base URL:   ${ENDPOINTS.health.replace('/health', '')}`);
  console.log('='.repeat(60));

  // Health check
  const healthResponse = http.get(ENDPOINTS.health);
  if (healthResponse.status !== 200) {
    throw new Error(`Server health check failed: ${healthResponse.status}`);
  }
  console.log('Server health check: PASSED');

  return {
    startTime: Date.now(),
    scenario: SCENARIO,
    testMode: TEST_MODE,
  };
}

// Health check scenario
function runHealthScenario(authParams) {
  const response = http.get(ENDPOINTS.health, { timeout: '10s' });

  const success = check(response, {
    'health: status 200': (r) => r.status === 200,
    'health: fast response': (r) => r.timings.duration < 100,
  });

  return success;
}

// CRUD operations scenario
function runCrudScenario(authParams) {
  let success = true;
  let projectId = null;

  // List projects
  group('CRUD: List', () => {
    const response = http.get(ENDPOINTS.projects, authParams);
    success = success && check(response, {
      'list: status 200': (r) => r.status === 200,
    });
  });

  sleep(0.3);

  // Create project
  group('CRUD: Create', () => {
    const response = http.post(
      ENDPOINTS.projects,
      JSON.stringify({ name: generateProjectName(), repositories: [] }),
      authParams
    );

    const createSuccess = check(response, {
      'create: status 200': (r) => r.status === 200,
    });

    if (createSuccess) {
      try {
        projectId = JSON.parse(response.body).data?.id;
      } catch {
        // Ignore
      }
    }
    success = success && createSuccess;
  });

  sleep(0.3);

  // Delete project if created
  if (projectId) {
    group('CRUD: Delete', () => {
      const response = http.del(`${ENDPOINTS.projects}/${projectId}`, null, authParams);
      success = success && check(response, {
        'delete: status 200 or 202': (r) => r.status === 200 || r.status === 202,
      });
    });
  }

  return success;
}

// Concurrent operations scenario - mixed workload
function runConcurrentScenario(authParams) {
  const operations = ['list', 'create', 'list', 'config', 'list'];
  const operation = operations[__ITER % operations.length];
  let success = true;

  group(`Concurrent: ${operation}`, () => {
    switch (operation) {
      case 'list':
        const listResp = http.get(ENDPOINTS.projects, authParams);
        success = check(listResp, { 'concurrent list: 200': (r) => r.status === 200 });
        break;

      case 'create':
        const createResp = http.post(
          ENDPOINTS.projects,
          JSON.stringify({ name: generateProjectName(), repositories: [] }),
          authParams
        );
        success = check(createResp, { 'concurrent create: 200': (r) => r.status === 200 });
        break;

      case 'config':
        const configResp = http.get(ENDPOINTS.config, authParams);
        success = check(configResp, { 'concurrent config: 200': (r) => r.status === 200 });
        break;
    }
  });

  return success;
}

// Database pool stress scenario
function runDbPoolScenario(authParams) {
  // Rapid-fire requests to stress connection pool
  const requests = [
    ['GET', ENDPOINTS.projects, null],
    ['GET', ENDPOINTS.config, null],
    ['POST', ENDPOINTS.projects, JSON.stringify({ name: generateProjectName(), repositories: [] })],
  ];

  let success = true;
  for (const [method, url, body] of requests) {
    const response = method === 'POST'
      ? http.post(url, body, { ...authParams, timeout: '15s' })
      : http.get(url, { ...authParams, timeout: '15s' });

    success = success && check(response, {
      'db-pool: response received': (r) => r.status > 0,
      'db-pool: not timeout': (r) => r.status !== 0,
    });

    if (response.status === 503 || response.status === 500) {
      totalErrors.add(1);
    }
  }

  return success;
}

// Main test function
export default function (data) {
  const startTime = Date.now();
  const tokenPool = getDefaultTokenPool();
  const { token } = tokenPool.getForVU(__VU);
  const authParams = getAuthParams(token);

  let scenarioPassed = false;

  switch (data.scenario) {
    case 'health':
      scenarioPassed = runHealthScenario(authParams);
      break;

    case 'crud':
      scenarioPassed = runCrudScenario(authParams);
      break;

    case 'concurrent':
      scenarioPassed = runConcurrentScenario(authParams);
      break;

    case 'db-pool':
      scenarioPassed = runDbPoolScenario(authParams);
      break;

    case 'all':
    default:
      // Run all scenarios in rotation
      const scenarios = ['health', 'crud', 'concurrent'];
      const selectedScenario = scenarios[__ITER % scenarios.length];

      group(`All: ${selectedScenario}`, () => {
        switch (selectedScenario) {
          case 'health':
            scenarioPassed = runHealthScenario(authParams);
            break;
          case 'crud':
            scenarioPassed = runCrudScenario(authParams);
            break;
          case 'concurrent':
            scenarioPassed = runConcurrentScenario(authParams);
            break;
        }
      });
  }

  // Record metrics
  overallDuration.add(Date.now() - startTime);
  scenarioSuccess.add(scenarioPassed ? 1 : 0);

  if (!scenarioPassed) {
    totalErrors.add(1);
  }

  // Variable think time
  sleep(0.5 + Math.random() * 1.5);
}

// Teardown
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\nTest suite completed in ${duration.toFixed(2)} seconds`);
}

// Summary handler
export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'load-test-suite',
    configuration: {
      scenario: SCENARIO,
      testMode: TEST_MODE,
      baseUrl: ENDPOINTS.health.replace('/health', ''),
    },
    results: {
      totalRequests: data.metrics.http_reqs?.values?.count || 0,
      requestsPerSecond: data.metrics.http_reqs?.values?.rate?.toFixed(2) || 'N/A',
      iterations: data.metrics.iterations?.values?.count || 0,
    },
    responseTime: {
      avg: data.metrics.http_req_duration?.values?.avg?.toFixed(2) + 'ms',
      min: data.metrics.http_req_duration?.values?.min?.toFixed(2) + 'ms',
      max: data.metrics.http_req_duration?.values?.max?.toFixed(2) + 'ms',
      p90: data.metrics.http_req_duration?.values?.['p(90)']?.toFixed(2) + 'ms',
      p95: data.metrics.http_req_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      p99: data.metrics.http_req_duration?.values?.['p(99)']?.toFixed(2) + 'ms',
    },
    errors: {
      httpFailRate: ((data.metrics.http_req_failed?.values?.rate || 0) * 100).toFixed(2) + '%',
      totalErrors: data.metrics.total_errors?.values?.count || 0,
      scenarioSuccessRate: ((data.metrics.scenario_success?.values?.rate || 0) * 100).toFixed(2) + '%',
    },
    thresholds: {
      allPassed: !Object.entries(data.metrics || {}).some(
        ([, metric]) => metric.thresholds && Object.values(metric.thresholds).some(t => !t.ok)
      ),
    },
  };

  // Determine overall status
  const p95 = data.metrics.http_req_duration?.values?.['p(95)'] || 0;
  const errorRate = data.metrics.http_req_failed?.values?.rate || 0;

  summary.verdict = {
    responseTimeOK: p95 < 200,
    errorRateOK: errorRate < 0.01,
    overallPass: p95 < 200 && errorRate < 0.01 && summary.thresholds.allPassed,
  };

  // Console output
  console.log('\n' + '='.repeat(70));
  console.log('LOAD TEST SUITE - FINAL RESULTS');
  console.log('='.repeat(70));
  console.log(`Scenario:              ${summary.configuration.scenario}`);
  console.log(`Test Mode:             ${summary.configuration.testMode}`);
  console.log('-'.repeat(70));
  console.log(`Total Requests:        ${summary.results.totalRequests}`);
  console.log(`Requests/sec:          ${summary.results.requestsPerSecond}`);
  console.log(`Total Iterations:      ${summary.results.iterations}`);
  console.log('-'.repeat(70));
  console.log('Response Times:');
  console.log(`  Average:             ${summary.responseTime.avg}`);
  console.log(`  p90:                 ${summary.responseTime.p90}`);
  console.log(`  p95:                 ${summary.responseTime.p95} ${summary.verdict.responseTimeOK ? '(OK)' : '(EXCEEDS THRESHOLD)'}`);
  console.log(`  p99:                 ${summary.responseTime.p99}`);
  console.log('-'.repeat(70));
  console.log('Error Rates:');
  console.log(`  HTTP Failures:       ${summary.errors.httpFailRate} ${summary.verdict.errorRateOK ? '(OK)' : '(EXCEEDS THRESHOLD)'}`);
  console.log(`  Scenario Success:    ${summary.errors.scenarioSuccessRate}`);
  console.log('-'.repeat(70));
  console.log(`All Thresholds Passed: ${summary.thresholds.allPassed ? 'YES' : 'NO'}`);
  console.log(`OVERALL RESULT:        ${summary.verdict.overallPass ? 'PASS' : 'FAIL'}`);
  console.log('='.repeat(70));

  return {
    stdout: JSON.stringify(summary, null, 2),
    'results/load-test-summary.json': JSON.stringify(summary, null, 2),
  };
}
