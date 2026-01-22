# Local Kubernetes Mode Testing

This guide explains how to test the multi-user Kubernetes deployment features locally.

## Quick Start

### Option 1: Desktop Mode (Default)

Run the application in single-user desktop mode (no authentication):

```bash
pnpm run dev
```

### Option 2: Kubernetes Mode (Multi-User with Auth)

Run the application in multi-user mode with JWT authentication:

```bash
# Start PostgreSQL and configure environment
./scripts/start-k8s-mode.sh
```

In a separate terminal, start the frontend:

```bash
pnpm run frontend:dev
```

## Detailed Setup for Kubernetes Mode

### 1. Start PostgreSQL

```bash
docker run -d \
  --name vibe-postgres \
  -e POSTGRES_PASSWORD=dev_password \
  -e POSTGRES_DB=vibe_kanban \
  -p 5432:5432 \
  postgres:15
```

### 2. Run Database Migrations

```bash
DATABASE_URL="postgresql://postgres:dev_password@localhost:5432/vibe_kanban" \
  sqlx migrate run --source crates/db/pg_migrations
```

### 3. Configure Environment Variables

Create a `.env.k8s` file:

```bash
DEPLOYMENT_MODE=kubernetes
DATABASE_URL=postgresql://postgres:dev_password@localhost:5432/vibe_kanban
JWT_SECRET=your-secret-key-for-testing-min-32-chars-long-abcdef
CONFIG_ENCRYPTION_KEY=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
WORKSPACE_BASE_DIR=./workspaces
HOST=0.0.0.0
PORT=8081
```

Load the environment:

```bash
export $(cat .env.k8s | xargs)
```

### 4. Start the Backend

```bash
pnpm run backend:dev
```

### 5. Start the Frontend (separate terminal)

```bash
pnpm run frontend:dev
```

## Generate Test JWT Tokens

Use the provided script to generate JWT tokens for testing:

```bash
# Generate token with random user ID
node scripts/generate-test-jwt.js

# Generate token with specific user ID and email
node scripts/generate-test-jwt.js "550e8400-e29b-41d4-a716-446655440000" "alice@example.com"
```

The script will output:
- User ID
- Email
- JWT token
- Usage examples

## Testing with JWT Tokens

### REST API

```bash
# Set your token
TOKEN="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."

# Test authentication
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:8081/projects

# Create a project
curl -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"Test Project"}' \
  http://localhost:8081/projects

# List tasks
curl -H "Authorization: Bearer $TOKEN" \
  http://localhost:8081/tasks
```

### WebSocket (Terminal)

Connect to WebSocket with token as query parameter:

```javascript
const ws = new WebSocket(`ws://localhost:8081/terminal?token=${TOKEN}`);
```

Or use a WebSocket client like `websocat`:

```bash
websocat "ws://localhost:8081/terminal?token=${TOKEN}"
```

## Testing User Isolation

Generate tokens for two different users and verify isolation:

```bash
# User A
TOKEN_A=$(node scripts/generate-test-jwt.js "user-a-uuid" "alice@example.com" | grep "^eyJ" | head -1)

# User B
TOKEN_B=$(node scripts/generate-test-jwt.js "user-b-uuid" "bob@example.com" | grep "^eyJ" | head -1)

# Create project as User A
PROJECT_ID=$(curl -s -X POST \
  -H "Authorization: Bearer $TOKEN_A" \
  -H "Content-Type: application/json" \
  -d '{"name":"Alice Project"}' \
  http://localhost:8081/projects | jq -r '.id')

# Try to access as User B (should return 404)
curl -H "Authorization: Bearer $TOKEN_B" \
  http://localhost:8081/projects/$PROJECT_ID
```

## Running Tests

### Unit Tests

```bash
cargo test --workspace
```

### Integration Tests

```bash
# User isolation tests
cargo test -p server --test user_isolation -- --test-threads=1

# Security path tests
cargo test -p services --test security_paths

# WebSocket auth tests
cargo test -p server --test websocket_auth -- --test-threads=1

# Migration tests (requires PostgreSQL)
DATABASE_URL="postgresql://postgres:dev_password@localhost:5432/vibe_kanban_test" \
  cargo test -p db --test migrations -- --ignored --test-threads=1
```

### Load Tests (k6)

```bash
# Install k6
brew install k6  # macOS

# Set up environment
export BASE_URL="http://localhost:8081"
export JWT_SECRET="your-secret-key-for-testing-min-32-chars-long-abcdef"

# Run all tests
cd tests/load
k6 run main.js

# Run specific scenarios
k6 run scenarios/concurrent-users.js
k6 run scenarios/db-pool.js
```

## Cleanup

Stop and remove PostgreSQL container:

```bash
docker stop vibe-postgres
docker rm vibe-postgres
```

Clean workspaces directory:

```bash
rm -rf workspaces/*
```

## Environment Variables Reference

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DEPLOYMENT_MODE` | No | `desktop` | Set to `kubernetes` for multi-user mode |
| `DATABASE_URL` | Yes (K8s) | - | PostgreSQL connection string |
| `JWT_SECRET` | Yes (K8s) | - | Secret key for JWT signing (min 32 chars) |
| `CONFIG_ENCRYPTION_KEY` | Yes (K8s) | - | 32-byte hex key for OAuth credential encryption |
| `WORKSPACE_BASE_DIR` | No | `/workspaces` | Base directory for user workspaces |
| `PTY_SESSION_TIMEOUT_SECS` | No | `1800` | PTY session idle timeout (30 minutes) |
| `CLEANUP_INTERVAL_SECS` | No | `300` | Cleanup job interval (5 minutes) |

## Troubleshooting

### PostgreSQL Connection Failed

Check if PostgreSQL is running:

```bash
docker ps | grep vibe-postgres
```

Test connection:

```bash
psql -h localhost -U postgres -d vibe_kanban
# Password: dev_password
```

### JWT Authentication Failed

Verify JWT secret matches between token generation and server:

```bash
echo $JWT_SECRET
```

Check token expiration (default 24 hours from generation).

### Workspace Path Validation Failed

Ensure `WORKSPACE_BASE_DIR` exists and is writable:

```bash
mkdir -p workspaces
chmod 755 workspaces
```

### Migration Failed

Reset database:

```bash
docker exec -it vibe-postgres psql -U postgres -c "DROP DATABASE vibe_kanban;"
docker exec -it vibe-postgres psql -U postgres -c "CREATE DATABASE vibe_kanban;"
sqlx migrate run --source crates/db/pg_migrations
```
