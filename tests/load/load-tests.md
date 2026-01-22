# Load Test Cases Documentation

## Test File

`tests/load/main.js` with scenario modules in `tests/load/scenarios/`

## Test Purpose

This load test suite validates the K8s multi-user deployment can handle the specified non-functional requirements:
- Support 100 concurrent users per pod
- API response time < 200ms at p95
- Database queries < 100ms at p99
- PTY input latency < 50ms under normal load

## Test Cases Overview

| Case ID | Feature Description | Test Type |
|---------|---------------------|-----------|
| LT-01 | Health endpoint baseline performance | Smoke Test |
| LT-02 | API CRUD operations under load | Load Test |
| LT-03 | 100 concurrent users with mixed workload | Load Test |
| LT-04 | Database connection pool behavior | Stress Test |
| LT-05 | WebSocket/Terminal latency | Performance Test |
| LT-06 | Combined scenario execution | Integration Test |

## Detailed Test Steps

### LT-01: Health Endpoint Baseline

**Test Purpose**: Establish baseline performance metrics for the health endpoint which has no authentication or database dependencies.

**Test Data Preparation**:
- No special data required
- Server must be running at BASE_URL

**Test Steps**:
1. Ramp up to 5 virtual users over 30 seconds
2. Maintain 5 users for 1 minute, each making GET requests to `/api/health`
3. Ramp down to 0 users over 30 seconds
4. Record response times and error rates

**Expected Results**:
- Status code 200 for all requests
- p95 response time < 50ms
- p99 response time < 100ms
- Error rate < 0.1%

**Test File**: `scenarios/health.js`

---

### LT-02: API CRUD Operations

**Test Purpose**: Validate authenticated CRUD operations (Create, Read, Delete) for projects maintain acceptable performance under load.

**Test Data Preparation**:
- JWT token generation for authentication
- Token pool with 100 pre-generated tokens

**Test Steps**:
1. Ramp up to 5 virtual users over 30 seconds
2. Each user performs in sequence:
   - GET /api/projects (list projects)
   - POST /api/projects (create new project)
   - DELETE /api/projects/:id (delete created project)
3. Maintain load for 1 minute
4. Ramp down to 0 users

**Expected Results**:
- List projects p95 < 200ms
- Create project p95 < 300ms
- Delete project p95 < 200ms
- Error rate < 1%

**Test File**: `scenarios/api-crud.js`

---

### LT-03: 100 Concurrent Users

**Test Purpose**: Validate the system supports 100 concurrent users with mixed read/write/delete workloads.

**Test Data Preparation**:
- Token pool with 100 unique user tokens
- Each virtual user gets a consistent token (round-robin assignment)

**Test Steps**:
1. Ramp up from 0 to 100 users over 2 minutes
2. Maintain 100 concurrent users for 5 minutes
3. Each user performs operations with weighted distribution:
   - 60% read operations (list projects, get config)
   - 25% write operations (create projects)
   - 15% delete operations
4. Ramp down from 100 to 0 users over 1 minute

**Expected Results**:
- System handles 100 concurrent users without errors
- Overall p95 response time < 200ms
- Read operations p95 < 200ms
- Write operations p95 < 300ms
- Delete operations p95 < 200ms
- Error rate < 1%
- No connection pool exhaustion

**Test File**: `scenarios/concurrent-users.js`

---

### LT-04: Database Connection Pool Behavior

**Test Purpose**: Validate database connection pool handles spike traffic patterns gracefully without exhaustion or failures.

**Test Data Preparation**:
- Token pool for authentication
- Monitoring for connection timeout errors

**Test Steps**:
1. Start with normal load (10 users) for 30 seconds
2. Sudden spike to 150 users over 10 seconds
3. Maintain 150 users for 1 minute
4. Drop back to 10 users over 10 seconds
5. Recovery period at 10 users for 1 minute
6. Ramp down to 0

**Expected Results**:
- Database query p99 < 100ms under normal load
- Graceful degradation during spikes (< 5% errors)
- No permanent connection pool exhaustion
- Recovery to normal performance after spike
- Connection error rate < 5%

**Test File**: `scenarios/db-pool.js`

---

### LT-05: WebSocket/Terminal Latency

**Test Purpose**: Validate PTY terminal WebSocket connections maintain acceptable latency for real-time interaction.

**Test Data Preparation**:
- JWT tokens with WebSocket query parameter support
- WebSocket connection URL construction

**Test Steps**:
1. Ramp up to 5 virtual users
2. Each user:
   - Establishes WebSocket connection to /api/terminal
   - Measures connection establishment time
   - Sends 5 ping messages with timestamps
   - Measures message round-trip latency
   - Closes connection after 10 seconds
3. Maintain test for 2 minutes
4. Ramp down

**Expected Results**:
- WebSocket connection time p95 < 100ms
- Message round-trip latency p95 < 50ms
- Error rate < 5%
- Stable connections without unexpected drops

**Test File**: `scenarios/websocket.js`

---

### LT-06: Combined Scenario Execution

**Test Purpose**: Run all test scenarios together to validate overall system performance under realistic mixed workloads.

**Test Data Preparation**:
- All prerequisites from individual scenarios
- Environment variables: BASE_URL, JWT_SECRET

**Test Steps**:
1. Select test mode (smoke, load, stress, spike)
2. Run selected scenarios in rotation:
   - Iteration 1: Health check
   - Iteration 2: CRUD operations
   - Iteration 3: Concurrent operations
3. Collect aggregate metrics across all scenarios
4. Generate combined summary report

**Expected Results**:
- 95% scenario success rate
- Overall p95 response time < 200ms
- HTTP failure rate < 1%
- All individual scenario thresholds met

**Test File**: `main.js`

---

## Test Considerations

### Mock Strategy

**JWT Tokens**:
- Tokens are generated locally using a simplified HMAC-SHA256 implementation
- In production tests, the JWT_SECRET must match the server's secret
- Token pool is pre-generated to avoid generation overhead during tests

**User Isolation**:
- Each virtual user gets a unique user_id from the token pool
- In multi-user mode, this simulates different users accessing their isolated data
- Token assignment is consistent per VU (virtual user) for reproducibility

### Boundary Conditions

**Connection Pool Limits**:
- Default PostgreSQL pool size: 10 connections
- Tests should verify behavior when pool approaches capacity
- Spike tests intentionally exceed normal capacity to test graceful degradation

**Token Expiration**:
- Test tokens have 24-hour expiration
- Long-running tests should not exceed this duration
- Token refresh is not implemented in load tests

**Rate Limiting**:
- Tests assume no rate limiting is in place
- If rate limiting is added, adjust concurrent user counts accordingly

### Asynchronous Operations

**WebSocket Handling**:
- k6's WebSocket support is callback-based
- Message latency is measured from send to receive timestamp
- Connection timeouts are set to prevent hung connections

**Cleanup Operations**:
- Project deletions may return 202 (Accepted) for async cleanup
- Tests treat both 200 and 202 as success for delete operations
- Background cleanup is not waited for in tests

### Environment Requirements

**Server Configuration**:
- Server must be running in K8s mode (DEPLOYMENT_MODE=kubernetes) for auth
- Or in desktop mode without authentication for basic connectivity tests
- DATABASE_URL must point to a PostgreSQL database for full functionality

**Resource Recommendations**:
- Test machine should have stable network connection
- Minimum 4 CPU cores for running 100 VU tests
- At least 8GB RAM for large-scale tests

### Running Tests

**Quick Validation**:
```bash
k6 run --vus 5 --duration 1m scenarios/health.js
```

**Full 100-User Test**:
```bash
k6 run scenarios/concurrent-users.js
```

**Custom Configuration**:
```bash
k6 run -e BASE_URL=https://api.example.com -e JWT_SECRET=your-secret main.js
```

### Metrics Interpretation

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| p95 Response Time | < 200ms | 200-500ms | > 500ms |
| p99 Response Time | < 500ms | 500-1000ms | > 1000ms |
| Error Rate | < 0.1% | 0.1-1% | > 1% |
| DB Query p99 | < 100ms | 100-200ms | > 200ms |
| WS Latency p95 | < 50ms | 50-100ms | > 100ms |
