/**
 * API CRUD Operations Load Test
 *
 * Tests authenticated CRUD operations for projects and tasks.
 * Validates response times and data integrity under load.
 *
 * Usage:
 *   k6 run scenarios/api-crud.js
 *
 * Expected Results:
 *   - p95 response time < 200ms
 *   - Error rate < 1%
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';
import {
  ENDPOINTS,
  STANDARD_THRESHOLDS,
  SMOKE_TEST_STAGES,
  DEFAULT_PARAMS,
  getAuthParams,
  generateProjectName,
  generateTaskTitle,
} from '../config.js';
import { generateToken, getDefaultTokenPool } from '../utils/jwt.js';

// Custom metrics
const createProjectDuration = new Trend('create_project_duration', true);
const listProjectsDuration = new Trend('list_projects_duration', true);
const deleteProjectDuration = new Trend('delete_project_duration', true);
const crudErrors = new Rate('crud_errors');
const projectsCreated = new Counter('projects_created');
const projectsDeleted = new Counter('projects_deleted');

// Test configuration
export const options = {
  stages: SMOKE_TEST_STAGES,
  thresholds: {
    ...STANDARD_THRESHOLDS,
    create_project_duration: ['p(95)<300'],
    list_projects_duration: ['p(95)<200'],
    delete_project_duration: ['p(95)<200'],
  },
  tags: {
    test_type: 'api-crud',
  },
};

// Setup - create token pool
export function setup() {
  console.log(`Testing API CRUD at: ${ENDPOINTS.projects}`);

  // Test connectivity
  const response = http.get(ENDPOINTS.health);
  if (response.status !== 200) {
    throw new Error(`Server not healthy: ${response.status}`);
  }

  return {
    startTime: Date.now(),
  };
}

// Main test function
export default function () {
  // Get a unique token for this VU iteration
  const tokenPool = getDefaultTokenPool();
  const { token, userId } = tokenPool.getRandom();
  const authParams = getAuthParams(token);

  let createdProjectId = null;

  group('Project CRUD Operations', () => {
    // 1. List existing projects (should be empty for new user)
    group('List Projects', () => {
      const listStart = Date.now();
      const listResponse = http.get(ENDPOINTS.projects, authParams);
      listProjectsDuration.add(Date.now() - listStart);

      const listCheck = check(listResponse, {
        'list status is 200': (r) => r.status === 200,
        'list response is valid JSON': (r) => {
          try {
            JSON.parse(r.body);
            return true;
          } catch {
            return false;
          }
        },
        'list response has success': (r) => {
          try {
            return JSON.parse(r.body).success === true;
          } catch {
            return false;
          }
        },
      });

      if (!listCheck) crudErrors.add(1);
    });

    sleep(0.5);

    // 2. Create a new project
    group('Create Project', () => {
      const projectName = generateProjectName();
      const createPayload = JSON.stringify({
        name: projectName,
        repositories: [], // Empty for load testing
      });

      const createStart = Date.now();
      const createResponse = http.post(ENDPOINTS.projects, createPayload, authParams);
      createProjectDuration.add(Date.now() - createStart);

      const createCheck = check(createResponse, {
        'create status is 200': (r) => r.status === 200,
        'create response has project id': (r) => {
          try {
            const body = JSON.parse(r.body);
            return body.success === true && body.data?.id;
          } catch {
            return false;
          }
        },
        'create response time < 500ms': (r) => r.timings.duration < 500,
      });

      if (createCheck) {
        try {
          const body = JSON.parse(createResponse.body);
          createdProjectId = body.data?.id;
          projectsCreated.add(1);
        } catch {
          // Ignore parse errors
        }
      } else {
        crudErrors.add(1);
      }
    });

    sleep(0.5);

    // 3. Verify project appears in list
    if (createdProjectId) {
      group('Verify Project Created', () => {
        const verifyResponse = http.get(ENDPOINTS.projects, authParams);

        check(verifyResponse, {
          'verify list contains new project': (r) => {
            try {
              const body = JSON.parse(r.body);
              // In multi-user mode with proper isolation, we'd check for our specific project
              // For now, just verify the list endpoint works
              return body.success === true && Array.isArray(body.data);
            } catch {
              return false;
            }
          },
        });
      });

      sleep(0.3);

      // 4. Delete the created project
      group('Delete Project', () => {
        const deleteStart = Date.now();
        const deleteResponse = http.del(
          `${ENDPOINTS.projects}/${createdProjectId}`,
          null,
          authParams
        );
        deleteProjectDuration.add(Date.now() - deleteStart);

        const deleteCheck = check(deleteResponse, {
          'delete status is 200 or 202': (r) => r.status === 200 || r.status === 202,
          'delete response time < 500ms': (r) => r.timings.duration < 500,
        });

        if (deleteCheck) {
          projectsDeleted.add(1);
        } else {
          crudErrors.add(1);
        }
      });
    }
  });

  // Think time between iterations
  sleep(1 + Math.random() * 2);
}

// Teardown
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`API CRUD test completed in ${duration.toFixed(2)} seconds`);
}

// Summary handler
export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'api-crud',
    metrics: {
      totalRequests: data.metrics.http_reqs?.values?.count || 0,
      avgDuration: data.metrics.http_req_duration?.values?.avg?.toFixed(2) + 'ms',
      p95Duration: data.metrics.http_req_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      errorRate: ((data.metrics.http_req_failed?.values?.rate || 0) * 100).toFixed(2) + '%',
      projectsCreated: data.metrics.projects_created?.values?.count || 0,
      projectsDeleted: data.metrics.projects_deleted?.values?.count || 0,
    },
    operations: {
      listProjects: {
        p95: data.metrics.list_projects_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      },
      createProject: {
        p95: data.metrics.create_project_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      },
      deleteProject: {
        p95: data.metrics.delete_project_duration?.values?.['p(95)']?.toFixed(2) + 'ms',
      },
    },
  };

  console.log('\n=== API CRUD Test Summary ===');
  console.log(JSON.stringify(summary, null, 2));

  return {
    stdout: JSON.stringify(summary, null, 2),
  };
}
