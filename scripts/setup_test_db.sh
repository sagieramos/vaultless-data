#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Setting up test database...${NC}"

# Database credentials
DB_USER="vaultless"
DB_PASS="vaultless_dev_pass"
DB_HOST="localhost"
DB_PORT="5432"
TEST_DB="vaultless_db"

# Check if PostgreSQL is running
if ! pg_isready -h $DB_HOST -p $DB_PORT > /dev/null 2>&1; then
    echo "Error: PostgreSQL is not running on $DB_HOST:$DB_PORT"
    echo "Start it with: docker-compose up -d postgres"
    exit 1
fi

# Drop test database if exists
echo "Dropping existing test database (if any)..."
PGPASSWORD=$DB_PASS psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d postgres -c "DROP DATABASE IF EXISTS $TEST_DB;" 2>/dev/null || true

# Create test database
echo "Creating test database..."
PGPASSWORD=$DB_PASS psql -h $DB_HOST -p $DB_PORT -U $DB_USER -d postgres -c "CREATE DATABASE $TEST_DB;"

# Run migrations
echo "Running migrations..."
export DATABASE_URL="postgresql://$DB_USER:$DB_PASS@$DB_HOST:$DB_PORT/$TEST_DB"
sqlx migrate run --source ./vaultless-api/migrations

echo -e "${GREEN}✓ Test database setup complete!${NC}"
echo ""
echo "Test database: $TEST_DB"
echo "Connection string: $DATABASE_URL"
echo ""
echo "Run tests with: cargo test --workspace"
