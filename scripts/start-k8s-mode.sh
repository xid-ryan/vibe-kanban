#!/bin/bash

# Start vibe-kanban in Kubernetes mode for local testing

set -e

echo "=== Starting vibe-kanban in Kubernetes mode ==="

# Check if PostgreSQL is running
if ! docker ps | grep -q vibe-postgres; then
  echo ""
  echo "Starting PostgreSQL container..."
  docker run -d \
    --name vibe-postgres \
    -e POSTGRES_PASSWORD=dev_password \
    -e POSTGRES_DB=vibe_kanban \
    -p 5432:5432 \
    postgres:15

  echo "Waiting for PostgreSQL to be ready..."
  sleep 5
else
  echo ""
  echo "PostgreSQL container already running"
fi

# Set environment variables
export DEPLOYMENT_MODE=kubernetes
export DATABASE_URL="postgresql://postgres:dev_password@localhost:5432/vibe_kanban"
export JWT_SECRET="your-secret-key-for-testing-min-32-chars-long-abcdef"
export CONFIG_ENCRYPTION_KEY="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
export WORKSPACE_BASE_DIR="./workspaces"

# Create workspaces directory
mkdir -p workspaces

echo ""
echo "Running migrations..."
cd "$(dirname "$0")/.."
sqlx migrate run --source crates/db/pg_migrations --database-url "$DATABASE_URL"

echo ""
echo "=== Environment configured ==="
echo "DEPLOYMENT_MODE=$DEPLOYMENT_MODE"
echo "DATABASE_URL=$DATABASE_URL"
echo "WORKSPACE_BASE_DIR=$WORKSPACE_BASE_DIR"
echo ""
echo "=== Generate a test JWT token ==="
echo "node scripts/generate-test-jwt.js"
echo ""
echo "=== Starting backend server ==="
pnpm run backend:dev
