# 🧪 Testing Checklist for PR Review

## Pre-Review Testing (Author)

### Setup
- [x] Code compiles without warnings
- [x] All dependencies installed
- [x] Docker services running (PostgreSQL, Dragonfly)
- [x] Migrations executed successfully
- [x] Server starts without errors

### Unit Tests
- [x] Crypto tests pass
- [x] Model tests pass
- [x] Middleware tests pass
- [ ] Integration tests (TODO: next PR)

### Manual Testing
- [x] Health endpoints respond correctly
- [x] API key creation works
- [x] Message send returns 201
- [x] Message receive returns messages
- [x] Analytics dashboard loads
- [x] Cache is being used (check logs)
- [x] TimescaleDB aggregates are populated

---

## Reviewer Testing Checklist

### 🏗️ Setup Verification (5 min)

```bash
# 1. Checkout branch
git checkout feature/message-endpoints-with-cache

# 2. Build
cargo build

# 3. Start services
docker-compose up -d
docker run -d --name dragonfly -p 6379:6379 docker.dragonflydb.io/dragonflydb/dragonfly

# 4. Run migrations
cd vaultless-api && sqlx migrate run

# 5. Start server
cargo run --bin vaultless-api
```

**Expected:** Server starts with:
```
🚀 Starting Vaultless Data API Server
✅ Database connected
💾 Cache connected
🌐 Server listening on http://0.0.0.0:8080
```

- [ ] All services start successfully
- [ ] No error messages in logs
- [ ] Health check returns 200

---

### 🔍 Functional Testing (15 min)

#### Test 1: Health Checks
```bash
curl http://localhost:8080/health
curl http://localhost:8080/ready
curl http://localhost:8080/live
```

**Expected:**
- [ ] `/health` returns JSON with status "healthy"
- [ ] `/ready` returns 200
- [ ] `/live` returns 200
- [ ] Database connection is true

---

#### Test 2: API Key Creation
```bash
curl -X POST http://localhost:8080/api/v1/admin/keys/create \
  -H "Content-Type: application/json" \
  -d '{"owner_email": "test@example.com", "tier": "pro"}'
```

**Expected:**
- [ ] Returns 201 Created
- [ ] Response includes `api_key` starting with "vlt_"
- [ ] Response includes `tier: "pro"`
- [ ] Response includes `monthly_quota: 500000`
- [ ] Warning message about saving key

**Save the API key for next tests!**

---

#### Test 3: Send Message
```bash
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY_HERE" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "test@example.com",
    "ciphertext": "dGVzdF9jaXBoZXJ0ZXh0",
    "nonce": "dGVzdF9ub25jZQ==",
    "content_size_bytes": 100
  }'
```

**Expected:**
- [ ] Returns 201 Created
- [ ] Response includes `message_id` (UUID)
- [ ] Response includes `recipient_id`
- [ ] Response includes `expires_at` timestamp
- [ ] Check logs for "Message sent successfully"

---

#### Test 4: Receive Messages
```bash
curl http://localhost:8080/api/v1/messages/test@example.com \
  -H "Authorization: YOUR_API_KEY_HERE"
```

**Expected:**
- [ ] Returns 200 OK
- [ ] Response includes `total_count: 1`
- [ ] Response includes messages array with 1 message
- [ ] Message includes ciphertext and nonce
- [ ] Check logs for "Cache miss" (first call)

**Call again immediately:**
```bash
curl http://localhost:8080/api/v1/messages/test@example.com \
  -H "Authorization: YOUR_API_KEY_HERE"
```

**Expected:**
- [ ] Returns 200 OK (faster response)
- [ ] Check logs for "Cache hit"

---

#### Test 5: Analytics Dashboard
```bash
curl http://localhost:8080/api/v1/analytics/dashboard \
  -H "Authorization: YOUR_API_KEY_HERE"
```

**Expected:**
- [ ] Returns 200 OK
- [ ] Response includes `current_month` with `total_messages_sent: 1`
- [ ] Response includes `quota_usage` with usage percentage
- [ ] Response includes `trends` object
- [ ] Response includes `last_7_days` array
- [ ] Response includes `last_4_weeks` array

---

#### Test 6: Error Handling

**Test 6a: Invalid API Key**
```bash
curl http://localhost:8080/api/v1/messages/send \
  -H "Authorization: invalid_key" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "test@example.com",
    "ciphertext": "data",
    "nonce": "nonce",
    "content_size_bytes": 100
  }'
```

**Expected:**
- [ ] Returns 401 Unauthorized
- [ ] Error message: "Invalid API key"

**Test 6b: Missing Required Field**
```bash
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY_HERE" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "test@example.com",
    "ciphertext": "data"
  }'
```

**Expected:**
- [ ] Returns 400 Bad Request or 422 Unprocessable Entity
- [ ] Error indicates missing field

**Test 6c: Invalid Message ID**
```bash
curl http://localhost:8080/api/v1/messages/invalid-uuid/metadata \
  -H "Authorization: YOUR_API_KEY_HERE"
```

**Expected:**
- [ ] Returns 400 Bad Request
- [ ] Error message about invalid UUID format

---

### 🚀 Performance Testing (10 min)

#### Test 7: Response Times

Use Postman or Apache Bench:

```bash
# Install Apache Bench if needed
# Ubuntu: sudo apt-get install apache2-utils
# macOS: already installed

# Test health endpoint (100 requests)
ab -n 100 -c 10 http://localhost:8080/health
```

**Expected:**
- [ ] Mean response time < 10ms
- [ ] No failed requests

```bash
# Test analytics dashboard (requires auth - use Postman instead)
```

**Expected (from Postman):**
- [ ] Dashboard response time < 100ms
- [ ] Cached message list < 50ms
- [ ] Uncached message list < 300ms

---

#### Test 8: Cache Verification

```bash
# Send message
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "cache-test@example.com",
    "ciphertext": "data",
    "nonce": "nonce",
    "content_size_bytes": 100
  }'

# First retrieval (cache miss)
time curl http://localhost:8080/api/v1/messages/cache-test@example.com \
  -H "Authorization: YOUR_API_KEY"

# Second retrieval (cache hit - should be faster)
time curl http://localhost:8080/api/v1/messages/cache-test@example.com \
  -H "Authorization: YOUR_API_KEY"
```

**Expected:**
- [ ] Second call is noticeably faster
- [ ] Logs show "Cache hit" on second call
- [ ] Both return same data

**Verify cache in Dragonfly:**
```bash
docker exec -it dragonfly redis-cli
> KEYS messages:*
> GET "messages:cache-test@example.com:list"
> TTL "messages:cache-test@example.com:list"
```

**Expected:**
- [ ] Key exists in cache
- [ ] TTL is ~60 seconds

---

### 📊 Database Verification (10 min)

#### Test 9: Schema Verification

```bash
docker exec -it vaultless-postgres psql -U vaultless -d vaultless_db
```

```sql
-- Check TimescaleDB is enabled
\dx timescaledb

-- Check hypertable
SELECT * FROM timescaledb_information.hypertables WHERE hypertable_name = 'usage_metrics';

-- Check continuous aggregates
SELECT view_name, materialized_only FROM timescaledb_information.continuous_aggregates;

-- Check compression policy
SELECT * FROM timescaledb_information.compression_settings WHERE hypertable_name = 'usage_metrics';

-- Check retention policy
SELECT * FROM timescaledb_information.data_retention_policies;

-- Verify data
SELECT * FROM api_keys;
SELECT * FROM messages;
SELECT * FROM usage_metrics;
SELECT * FROM usage_metrics_daily;

\q
```

**Expected:**
- [ ] TimescaleDB extension installed
- [ ] `usage_metrics` is a hypertable
- [ ] `usage_metrics_daily` aggregate exists
- [ ] `usage_metrics_weekly` aggregate exists
- [ ] Compression policy exists (7 days)
- [ ] Retention policy exists (90 days)
- [ ] API key exists in database
- [ ] Message exists in database
- [ ] Usage metric recorded

---

### 🔒 Security Testing (10 min)

#### Test 10: Authentication

**Test unauthorized access:**
```bash
curl http://localhost:8080/api/v1/messages/send \
  -H "Content-Type: application/json" \
  -d '{"recipient_id": "test", "ciphertext": "data", "nonce": "nonce", "content_size_bytes": 100}'
```

**Expected:**
- [ ] Returns 401 Unauthorized
- [ ] Error: "Missing Authorization header"

**Test with invalid format:**
```bash
curl http://localhost:8080/api/v1/messages/send \
  -H "Authorization: NotAValidKey123" \
  -H "Content-Type: application/json" \
  -d '{"recipient_id": "test", "ciphertext": "data", "nonce": "nonce", "content_size_bytes": 100}'
```

**Expected:**
- [ ] Returns 401 Unauthorized
- [ ] Error: "Invalid API key"

---

#### Test 11: API Key Security

**Verify key is hashed in database:**
```bash
docker exec -it vaultless-postgres psql -U vaultless -d vaultless_db \
  -c "SELECT key_hash, LENGTH(key_hash) as hash_length FROM api_keys LIMIT 1;"
```

**Expected:**
- [ ] `key_hash` is exactly 64 characters (SHA-256 hex)
- [ ] Hash does NOT match the raw API key you were given

**Verify key is not exposed:**
```bash
curl http://localhost:8080/api/v1/admin/keys
```

**Expected:**
- [ ] Response does NOT include actual API keys
- [ ] Only shows `key_prefix` (first 8-12 chars)

---

### 📈 Analytics Verification (5 min)

#### Test 12: TimescaleDB Aggregates

```bash
docker exec -it vaultless-postgres psql -U vaultless -d vaultless_db
```

```sql
-- Check if continuous aggregates are updating
SELECT * FROM usage_metrics_daily ORDER BY day DESC LIMIT 5;
SELECT * FROM usage_metrics_weekly ORDER BY week_start DESC LIMIT 2;

-- Verify aggregation matches raw data
SELECT 
    SUM(messages_sent) as total_sent,
    SUM(messages_received) as total_received
FROM usage_metrics
WHERE api_key_id = (SELECT id FROM api_keys LIMIT 1);

-- Compare with daily aggregate
SELECT 
    SUM(total_messages_sent) as total_sent,
    SUM(total_messages_received) as total_received
FROM usage_metrics_daily
WHERE api_key_id = (SELECT id FROM api_keys LIMIT 1);

\q
```

**Expected:**
- [ ] Daily aggregate shows data
- [ ] Totals match between raw and aggregate tables
- [ ] Aggregates update automatically (check after 1 hour)

---

### 🐛 Edge Cases (10 min)

#### Test 13: Self-Destructing Messages

```bash
# Send message with max_access_count = 2
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "self-destruct@example.com",
    "ciphertext": "secret",
    "nonce": "nonce",
    "content_size_bytes": 100,
    "max_access_count": 2
  }'

# Access 1st time
curl http://localhost:8080/api/v1/messages/self-destruct@example.com \
  -H "Authorization: YOUR_API_KEY"

# Access 2nd time
curl http://localhost:8080/api/v1/messages/self-destruct@example.com \
  -H "Authorization: YOUR_API_KEY"

# Access 3rd time (should be deleted/marked delivered)
curl http://localhost:8080/api/v1/messages/self-destruct@example.com \
  -H "Authorization: YOUR_API_KEY"
```

**Expected:**
- [ ] First two calls return the message
- [ ] Third call returns empty messages array OR 404
- [ ] Message is marked as delivered in database

---

#### Test 14: Message Expiration

```bash
# Send message with 5-second TTL
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "expire-test@example.com",
    "ciphertext": "data",
    "nonce": "nonce",
    "content_size_bytes": 100,
    "ttl_seconds": 5
  }'

# Retrieve immediately
curl http://localhost:8080/api/v1/messages/expire-test@example.com \
  -H "Authorization: YOUR_API_KEY"

# Wait 6 seconds
sleep 6

# Try to retrieve again
curl http://localhost:8080/api/v1/messages/expire-test@example.com \
  -H "Authorization: YOUR_API_KEY"
```

**Expected:**
- [ ] First call returns the message
- [ ] After expiration, message is not returned
- [ ] No errors, just empty messages array

---

#### Test 15: Quota Limits

**Create a Free tier API key:**
```bash
curl -X POST http://localhost:8080/api/v1/admin/keys/create \
  -H "Content-Type: application/json" \
  -d '{"owner_email": "quota-test@example.com", "tier": "free"}'
```

**Expected:**
- [ ] Returns `monthly_quota: 1000`

**Verify quota enforcement:**
```bash
# Check current usage
curl http://localhost:8080/api/v1/analytics/dashboard \
  -H "Authorization: FREE_TIER_API_KEY"

# Note: To fully test quota, you'd need to send 1001 messages
# For review, just verify the quota field is present
```

**Expected:**
- [ ] Dashboard shows quota correctly
- [ ] `quota_usage.monthly_quota` is 1000
- [ ] System tracks usage properly

---

### 📝 Code Quality Review (15 min)

#### Test 16: Code Style and Structure

**Check for:**
- [ ] No compiler warnings
- [ ] Consistent error handling
- [ ] Proper use of `Result` types
- [ ] No `unwrap()` calls in production code (only in tests)
- [ ] Meaningful variable names
- [ ] Functions are reasonably sized (<100 lines)
- [ ] Proper use of `async`/`await`

**Run clippy:**
```bash
cargo clippy --all-targets --all-features
```

**Expected:**
- [ ] No clippy warnings
- [ ] No clippy errors

**Run formatter:**
```bash
cargo fmt --check
```

**Expected:**
- [ ] Code is properly formatted

---

#### Test 17: Documentation

**Check that documentation exists for:**
- [ ] All public functions have doc comments
- [ ] Complex logic has inline comments
- [ ] API documentation is complete
- [ ] Environment variables are documented
- [ ] Setup instructions are clear

**Generate and review docs:**
```bash
cargo doc --no-deps --open
```

**Expected:**
- [ ] Documentation builds without warnings
- [ ] Public API is well documented

---

### 🔍 Architecture Review (10 min)

#### Test 18: Separation of Concerns

**Verify:**
- [ ] Business logic in `vaultless-core`
- [ ] HTTP handling in `vaultless-api`
- [ ] Models are reusable
- [ ] No database queries in handlers (should be in models)
- [ ] Cache logic abstracted in service layer

**Check dependencies:**
```bash
cargo tree --package vaultless-api
cargo tree --package vaultless-core
```

**Expected:**
- [ ] No circular dependencies
- [ ] Core doesn't depend on API
- [ ] Clean dependency graph

---

#### Test 19: Error Handling Consistency

**Review error types:**
- [ ] Custom errors in `VaultlessError` enum
- [ ] Proper error conversion (`From` traits)
- [ ] Client errors (4xx) vs server errors (5xx)
- [ ] No sensitive data in error messages

**Test various error scenarios:**
```bash
# Invalid JSON
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d 'invalid json'

# Missing required field
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{}'

# Invalid UUID
curl http://localhost:8080/api/v1/messages/not-a-uuid/metadata \
  -H "Authorization: YOUR_API_KEY"
```

**Expected:**
- [ ] All return proper error JSON
- [ ] Status codes are correct
- [ ] Error messages are helpful but not exposing internals

---

### 🌐 Integration Testing (10 min)

#### Test 20: Full Message Flow

**Run the complete flow:**

1. Create API key
2. Send 3 messages to same recipient
3. Retrieve messages (should get all 3)
4. Check analytics (should show 3 sent, 1 received)
5. Send to different recipient
6. Retrieve for each recipient separately
7. Check analytics again

**Use Postman "Complete Flow" folder:**
- [ ] Import collection
- [ ] Run "Complete Flow" folder
- [ ] All tests pass
- [ ] No failures

---

### 🔐 Security Review (10 min)

#### Test 21: Security Headers

```bash
curl -I http://localhost:8080/health
```

**Check for security headers:**
- [ ] `X-Request-ID` present
- [ ] No sensitive information in headers
- [ ] CORS headers present (if configured)

---

#### Test 22: SQL Injection Protection

**Verify SQLx prevents SQL injection:**
```bash
# Try SQL injection in recipient_id
curl -X POST http://localhost:8080/api/v1/messages/send \
  -H "Authorization: YOUR_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "recipient_id": "test@example.com; DROP TABLE messages;--",
    "ciphertext": "data",
    "nonce": "nonce",
    "content_size_bytes": 100
  }'

# Verify messages table still exists
docker exec -it vaultless-postgres psql -U vaultless -d vaultless_db \
  -c "SELECT COUNT(*) FROM messages;"
```

**Expected:**
- [ ] Request succeeds (SQLx uses parameterized queries)
- [ ] Messages table still exists
- [ ] No SQL injection occurred

---

### 📦 Deployment Readiness (5 min)

#### Test 23: Environment Configuration

**Verify all required env vars are documented:**
```bash
cat .env.example
```

**Expected:**
- [ ] All required variables listed
- [ ] Example values provided
- [ ] Comments explain each variable

**Test with missing env var:**
```bash
# Remove DATABASE_URL temporarily
unset DATABASE_URL
cargo run --bin vaultless-api
```

**Expected:**
- [ ] Application fails to start with clear error message
- [ ] Error indicates which env var is missing

---

#### Test 24: Docker Compatibility

**Verify services can be containerized:**
```bash
# Check docker-compose configuration
docker-compose config

# Test services start
docker-compose down
docker-compose up -d

# Wait for services to be healthy
sleep 10

# Verify they're running
docker-compose ps
```

**Expected:**
- [ ] docker-compose config is valid
- [ ] All services start successfully
- [ ] Health checks pass

---

### 📊 Performance Benchmarks (5 min)

#### Test 25: Load Testing (Optional)

**Use Apache Bench or wrk:**
```bash
# Install wrk if available
# Ubuntu: sudo apt-get install wrk
# macOS: brew install wrk

# Test health endpoint
wrk -t4 -c100 -d30s http://localhost:8080/health

# For authenticated endpoints, use Postman's Collection Runner
```

**Expected performance targets:**
- [ ] Health endpoint: >1000 req/s
- [ ] Send message: >100 req/s
- [ ] Receive message (cached): >500 req/s
- [ ] Analytics dashboard: >200 req/s

---

## 🎯 Sign-Off Checklist

### For Code Author
- [ ] All automated tests pass
- [ ] Manual testing completed
- [ ] Documentation updated
- [ ] No known security issues
- [ ] Performance benchmarks collected
- [ ] Ready for production deployment

### For Reviewer
- [ ] Code builds successfully
- [ ] All manual tests completed
- [ ] Security considerations reviewed
- [ ] Architecture makes sense
- [ ] Documentation is sufficient
- [ ] Performance is acceptable
- [ ] Approved for merge

---

## 📝 Test Results Summary

**Date:** _________________  
**Tester:** _________________  
**Branch:** `feature/message-endpoints-with-cache`  
**Commit:** _________________

### Overall Results

- **Tests Passed:** _____ / 25
- **Tests Failed:** _____
- **Tests Skipped:** _____
- **Blockers Found:** _____

### Critical Issues (if any)

1. _________________________________
2. _________________________________
3. _________________________________

### Recommendations

- [ ] Approve and merge
- [ ] Approve with minor fixes
- [ ] Request changes
- [ ] Need more testing

### Comments

_____________________________________________
_____________________________________________
_____________________________________________

---

## 🚀 Post-Merge Verification

After merging, verify:

- [ ] CI/CD pipeline passes
- [ ] Staging deployment successful
- [ ] Smoke tests pass in staging
- [ ] Monitoring shows no errors
- [ ] Performance metrics within targets

---

**Testing completed by:** _________________  
**Date:** _________________  
**Signature:** _________________