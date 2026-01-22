# Load Testing Suite for K8s Multi-User Deployment

This directory contains load tests for validating the vibe-kanban multi-user Kubernetes deployment performance requirements.

## Overview

These tests verify the non-functional requirements from the K8s multi-user deployment specification:

- **100 concurrent users** per pod
- **API response time < 200ms** at p95 (excluding long-running operations)
- **Database queries < 100ms** at p99
- **PTY input latency < 50ms** under normal load

## Prerequisites

### Install k6

k6 is a modern load testing tool. Install it using one of these methods:

**macOS (Homebrew):**
```bash
brew install k6
```

**Linux (Debian/Ubuntu):**
```bash
sudo gpg -k
sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update
sudo apt-get install k6
```

**Docker:**
```bash
docker pull grafana/k6
```

**Windows (Chocolatey):**
```bash
choco install k6
```

### Environment Setup

Before running tests, set the required environment variables:

```bash
# Required: Base URL of the vibe-kanban API
export BASE_URL="http://localhost:8081"

# Required for multi-user (K8s) mode: JWT secret for token generation
export JWT_SECRET="your-jwt-secret-here"

# Optional: Override default thresholds
export RESPONSE_TIME_P95=200  # ms
export RESPONSE_TIME_P99=500  # ms
export DB_QUERY_P99=100       # ms
```

## Running Tests

### Quick Start

Run all tests with default settings:
```bash
cd tests/load
k6 run main.js
```

### Individual Test Scenarios

**Health Check Test** - Baseline performance:
```bash
k6 run scenarios/health.js
```

**API CRUD Operations** - Tests project/task creation, listing, deletion:
```bash
k6 run scenarios/api-crud.js
```

**Concurrent Users Test** - Simulates 100 concurrent users:
```bash
k6 run scenarios/concurrent-users.js
```

**Database Pool Test** - Validates connection pool behavior under load:
```bash
k6 run scenarios/db-pool.js
```

**WebSocket/Terminal Test** - Tests PTY latency:
```bash
k6 run scenarios/websocket.js
```

### Custom Test Runs

**Adjust Virtual Users:**
```bash
k6 run --vus 50 --duration 2m scenarios/concurrent-users.js
```

**Run with Summary Export:**
```bash
k6 run --out json=results.json main.js
```

**Run with Grafana Cloud Integration:**
```bash
k6 run --out cloud main.js
```

### Using Docker

```bash
docker run --rm -i \
  -e BASE_URL=http://host.docker.internal:8081 \
  -e JWT_SECRET=your-secret \
  -v $(pwd):/scripts \
  grafana/k6 run /scripts/main.js
```

## Test Scenarios

### 1. Health Check (`scenarios/health.js`)

Tests the `/api/health` endpoint as a baseline:
- No authentication required
- Expected: < 50ms response time at p95
- Verifies basic connectivity and server responsiveness

### 2. API CRUD Operations (`scenarios/api-crud.js`)

Tests authenticated CRUD operations:
- GET /api/projects - List projects
- POST /api/projects - Create project
- DELETE /api/projects/:id - Delete project
- Expected: < 200ms response time at p95

### 3. Concurrent Users (`scenarios/concurrent-users.js`)

Simulates 100 concurrent users with mixed workloads:
- **Ramp-up**: 0 → 100 users over 2 minutes
- **Sustained load**: 100 users for 5 minutes
- **Ramp-down**: 100 → 0 users over 1 minute

Workload distribution:
- 60% - Read operations (list projects, get tasks)
- 25% - Write operations (create task, update task)
- 15% - Delete operations

### 4. Database Pool Behavior (`scenarios/db-pool.js`)

Tests database connection pool under stress:
- Burst requests to trigger pool exhaustion
- Monitors for connection timeout errors
- Validates pool recovery behavior

### 5. WebSocket/Terminal (`scenarios/websocket.js`)

Tests terminal WebSocket connections:
- Connection establishment time
- Message round-trip latency
- Concurrent WebSocket connections

## Metrics and Thresholds

### Key Metrics

| Metric | Description | Threshold |
|--------|-------------|-----------|
| `http_req_duration{p95}` | 95th percentile response time | < 200ms |
| `http_req_duration{p99}` | 99th percentile response time | < 500ms |
| `http_req_failed` | Failed request rate | < 1% |
| `http_reqs` | Requests per second | > 100 rps |
| `vus` | Virtual users | 100 |
| `ws_connecting` | WebSocket connection time | < 100ms |
| `ws_session_duration` | WebSocket session length | varies |

### Expected Results

For a healthy deployment with proper resource allocation:

```
✓ http_req_duration............: avg=45ms   min=5ms    med=35ms   max=450ms  p(90)=85ms   p(95)=120ms
✓ http_req_failed..............: 0.15%   ✓ 9985      ✗ 15
✓ http_reqs....................: 156/s
✓ iteration_duration...........: avg=640ms  min=450ms  med=600ms  max=2.1s   p(90)=850ms  p(95)=1s
✓ vus..........................: 100     min=1       max=100
✓ vus_max......................: 100     min=100     max=100
```

### Interpreting Results

**Green Checks (✓)**: Metric is within acceptable thresholds.

**Red Crosses (✗)**: Metric exceeded thresholds - investigate:
1. Check server resource utilization (CPU, memory)
2. Check database connection pool settings
3. Check network latency between test machine and server
4. Review application logs for errors

## Troubleshooting

### Common Issues

**"Connection refused" errors:**
- Verify the server is running at BASE_URL
- Check firewall rules
- Ensure correct port is exposed

**"401 Unauthorized" errors:**
- Verify JWT_SECRET matches the server's configuration
- Check token generation in `utils/jwt.js`

**High latency in K8s:**
- Check pod resource limits
- Verify EFS volume performance
- Check database connection pool size
- Review ALB timeout settings

**WebSocket connection failures:**
- Verify WebSocket upgrade is allowed
- Check ALB idle timeout (should be 3600s)
- Verify sticky sessions for WebSocket routes

### Debug Mode

Run with verbose logging:
```bash
k6 run --verbose scenarios/concurrent-users.js
```

Run with HTTP debug output:
```bash
k6 run --http-debug scenarios/api-crud.js
```

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Load Tests
on:
  workflow_dispatch:
  schedule:
    - cron: '0 2 * * 1'  # Weekly on Monday at 2 AM

jobs:
  load-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install k6
        run: |
          sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg \
            --keyserver hkp://keyserver.ubuntu.com:80 \
            --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
          echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" \
            | sudo tee /etc/apt/sources.list.d/k6.list
          sudo apt-get update && sudo apt-get install k6

      - name: Run Load Tests
        env:
          BASE_URL: ${{ secrets.LOAD_TEST_BASE_URL }}
          JWT_SECRET: ${{ secrets.JWT_SECRET }}
        run: |
          cd tests/load
          k6 run --out json=results.json main.js

      - name: Upload Results
        uses: actions/upload-artifact@v4
        with:
          name: load-test-results
          path: tests/load/results.json
```

## Performance Tuning Recommendations

Based on load test results, consider these optimizations:

1. **Database Connection Pool**: Increase `max_connections` if seeing pool exhaustion
2. **Pod Resources**: Adjust CPU/memory limits based on utilization
3. **Horizontal Scaling**: Add replicas if single pod shows bottlenecks
4. **Caching**: Implement caching for frequently accessed data
5. **Database Indexes**: Ensure proper indexes exist for query patterns

## Related Documentation

- [K8s Multi-User Requirements](/docs/specs/k8s-multiuser/requirements.md)
- [K8s Multi-User Design](/docs/specs/k8s-multiuser/design.md)
- [k6 Documentation](https://k6.io/docs/)
