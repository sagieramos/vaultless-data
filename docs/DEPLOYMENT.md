# 🚀 Deployment Guide - Vaultless Data API

This guide covers deploying the Vaultless Data API to production environments.

---

## 📋 Prerequisites

### Required Services
- PostgreSQL 15+ with TimescaleDB extension
- Dragonfly or Redis 7+
- Rust 1.75+ (for building)

### Recommended Infrastructure
- **Compute:** 2 vCPU, 4GB RAM (minimum)
- **Database:** PostgreSQL with SSD storage
- **Cache:** Dragonfly with 2GB memory
- **Load Balancer:** HTTPS termination
- **Monitoring:** Prometheus + Grafana

---

## 🏗️ Architecture Options

### Option 1: Single Server (Small Scale)
```
┌─────────────────────────────────────┐
│          Load Balancer              │
│         (HTTPS/SSL)                 │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│      Application Server             │
│                                     │
│  ┌──────────────────────────────┐   │
│  │  Vaultless API (Axum)        │   │
│  └──────────┬───────────────────┘   │
│             │                       │
│  ┌──────────▼───────┐  ┌─────────┐  │
│  │   PostgreSQL     │  │Dragonfly│  │
│  │  + TimescaleDB   │  │         │  │
│  └──────────────────┘  └─────────┘  │
└─────────────────────────────────────┘
```

**Best for:** <1,000 req/min, MVP, development

---

### Option 2: Distributed (Production Scale)
```
┌─────────────────────────────────────┐
│     Load Balancer (HTTPS)           │
└──┬──────────────┬───────────────┬───┘
   │              │               │
┌──▼────┐  ┌──▼────┐  ┌──▼────┐
│ API   │  │ API   │  │ API   │
│Node 1 │  │Node 2 │  │Node 3 │
└───┬───┘  └───┬───┘  └───┬───┘
    │          │          │
    └──────┬───┴──────────┘
           │
    ┌──────▼──────┐  ┌─────────────┐
    │ PostgreSQL  │  │  Dragonfly  │
    │  Primary    │  │   Cluster   │
    └──────┬──────┘  └─────────────┘
           │
    ┌──────▼──────┐
    │ PostgreSQL  │
    │  Replica    │
    └─────────────┘
```

**Best for:** >10,000 req/min, production, high availability

---

## 🐳 Docker Deployment

### Production Docker Compose

```yaml
version: '3.8'

services:
  api:
    build:
      context: .
      dockerfile: Dockerfile
    image: vaultless-api:latest
    ports:
      - "8080:8080"
    environment:
      - DATABASE_URL=postgresql://vaultless:${DB_PASSWORD}@postgres:5432/vaultless_db
      - CACHE_URL=redis://dragonfly:6379
      - RUST_LOG=info,vaultless_api=info
      - API_KEY_SALT=${API_KEY_SALT}
    depends_on:
      postgres:
        condition: service_healthy
      dragonfly:
        condition: service_started
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 40s

  postgres:
    image: timescale/timescaledb:latest-pg15
    environment:
      - POSTGRES_DB=vaultless_db
      - POSTGRES_USER=vaultless
      - POSTGRES_PASSWORD=${DB_PASSWORD}
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./migrations:/docker-entrypoint-initdb.d
    ports:
      - "5432:5432"
    restart: unless-stopped
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U vaultless"]
      interval: 10s
      timeout: 5s
      retries: 5

  dragonfly:
    image: docker.dragonflydb.io/dragonflydb/dragonfly:latest
    command: --maxmemory 2gb --proactor_threads 4
    ports:
      - "6379:6379"
    volumes:
      - dragonfly_data:/data
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5

volumes:
  postgres_data:
  dragonfly_data:
```

---

### Dockerfile (Multi-stage Build)

```dockerfile
# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY vaultless-core/Cargo.toml vaultless-core/
COPY vaultless-api/Cargo.toml vaultless-api/
COPY vaultless-sdk/Cargo.toml vaultless-sdk/

# Cache dependencies
RUN mkdir vaultless-core/src && echo "fn main() {}" > vaultless-core/src/lib.rs && \
    mkdir vaultless-api/src && echo "fn main() {}" > vaultless-api/src/main.rs && \
    mkdir vaultless-sdk/src && echo "fn main() {}" > vaultless-sdk/src/lib.rs && \
    cargo build --release && \
    rm -rf vaultless-*/src

# Copy source code
COPY vaultless-core ./vaultless-core
COPY vaultless-api ./vaultless-api
COPY vaultless-sdk ./vaultless-sdk

# Build application
RUN cargo build --release --bin vaultless-api

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /app/target/release/vaultless-api /usr/local/bin/vaultless-api

# Copy migrations
COPY vaultless-api/migrations ./migrations

# Create non-root user
RUN useradd -m -u 1000 vaultless && chown -R vaultless:vaultless /app
USER vaultless

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
  CMD curl -f http://localhost:8080/health || exit 1

CMD ["vaultless-api"]
```

---

## ☁️ Cloud Platform Deployment

### AWS (Elastic Beanstalk + RDS + ElastiCache)

```bash
# Install EB CLI
pip install awsebcli

# Initialize
eb init vaultless-api --region us-east-1

# Create environment
eb create vaultless-prod \
  --instance-type t3.medium \
  --database \
  --database.engine postgres \
  --database.version 15 \
  --elb-type application

# Set environment variables
eb setenv \
  DATABASE_URL="postgresql://..." \
  CACHE_URL="redis://..." \
  API_KEY_SALT="your-secure-salt" \
  RUST_LOG="info"

# Deploy
eb deploy
```

---

### Google Cloud (Cloud Run + Cloud SQL + Memorystore)

```bash
# Build and push container
gcloud builds submit --tag gcr.io/PROJECT_ID/vaultless-api

# Deploy to Cloud Run
gcloud run deploy vaultless-api \
  --image gcr.io/PROJECT_ID/vaultless-api \
  --platform managed \
  --region us-central1 \
  --allow-unauthenticated \
  --set-env-vars DATABASE_URL="..." \
  --set-env-vars CACHE_URL="..." \
  --set-cloudsql-instances PROJECT_ID:REGION:INSTANCE \
  --memory 2Gi \
  --cpu 2 \
  --max-instances 10
```

---

### DigitalOcean (App Platform)

Create `app.yaml`:

```yaml
name: vaultless-api
services:
  - name: api
    dockerfile_path: Dockerfile
    github:
      repo: your-username/vaultless-data
      branch: main
      deploy_on_push: true
    instance_count: 2
    instance_size_slug: professional-xs
    http_port: 8080
    health_check:
      http_path: /health
    envs:
      - key: DATABASE_URL
        scope: RUN_TIME
        value: ${db.DATABASE_URL}
      - key: CACHE_URL
        scope: RUN_TIME
        value: ${cache.REDIS_URL}
      - key: API_KEY_SALT
        scope: RUN_TIME
        type: SECRET

databases:
  - name: db
    engine: PG
    version: "15"
    production: true
    cluster_name: vaultless-db

services:
  - name: cache
    image:
      registry_type: DOCKER_HUB
      repository: dragonflydb/dragonfly
      tag: latest
```

Deploy:
```bash
doctl apps create --spec app.yaml
```

---

## 🔐 Security Hardening

### 1. Environment Variables

**Never commit these to git:**

```bash
# Production .env (use secrets manager instead)
DATABASE_URL=postgresql://user:${SECURE_PASSWORD}@host:5432/db
API_KEY_SALT=${RANDOM_64_CHAR_STRING}
CACHE_URL=redis://:${REDIS_PASSWORD}@host:6379
```

**Use AWS Secrets Manager / GCP Secret Manager / Vault:**

```bash
# Fetch from secrets manager on startup
export DATABASE_URL=$(aws secretsmanager get-secret-value --secret-id prod/database-url --query SecretString --output text)
```

---

### 2. Database Security

**Enable SSL:**
```bash
DATABASE_URL=postgresql://user:pass@host:5432/db?sslmode=require
```

**Use connection pooling:**
```bash
DATABASE_MAX_CONNECTIONS=10  # Adjust based on load
```

**Rotate passwords regularly:**
```sql
ALTER USER vaultless WITH PASSWORD 'new-secure-password';
```

---

### 3. API Security

**Rate limiting (implement in next PR):**
- Use Redis for distributed rate limiting
- Implement per-IP and per-API-key limits

**HTTPS only:**
```rust
// Add HSTS header
response.headers_mut().insert(
    "Strict-Transport-Security",
    "max-age=31536000; includeSubDomains".parse().unwrap()
);
```

**Remove admin endpoints in production:**
```rust
#[cfg(not(feature = "admin-api"))]
let admin_routes = Router::new(); // Empty

#[cfg(feature = "admin-api")]
let admin_routes = /* admin routes with auth */;
```

---

### 4. Monitoring & Alerts

**Add health check monitoring:**
```bash
# Use UptimeRobot, Pingdom, or custom
curl -f https://api.vaultless.io/health || alert
```

**Database monitoring:**
- Query performance
- Connection pool usage
- Disk space
- Replication lag

**Application metrics:**
- Request latency (P50, P95, P99)
- Error rates
- Cache hit rates
- Messages per second

---

## 📊 Performance Tuning

### Database Optimization

```sql
-- Increase shared buffers
ALTER SYSTEM SET shared_buffers = '4GB';

-- Increase work memory
ALTER SYSTEM SET work_mem = '64MB';

-- Enable parallel queries
ALTER SYSTEM SET max_parallel_workers_per_gather = 4;

-- Reload configuration
SELECT pg_reload_conf();

-- Analyze tables
ANALYZE messages;
ANALYZE usage_metrics;

-- Vacuum regularly
VACUUM ANALYZE;
```

---

### TimescaleDB Tuning

```sql
-- Adjust chunk interval (default is 7 days)
SELECT set_chunk_time_interval('usage_metrics', INTERVAL '1 day');

-- Update compression policy
SELECT add_compression_policy('usage_metrics', INTERVAL '3 days');

-- Refresh continuous aggregates more frequently
SELECT alter_job((SELECT job_id FROM timescaledb_information.jobs WHERE proc_name = 'policy_refresh_continuous_aggregate'), schedule_interval => INTERVAL '30 minutes');
```

---

### Application Tuning

```toml
# Cargo.toml - Release optimizations
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
panic = 'abort'
```

```bash
# Environment tuning
TOKIO_WORKER_THREADS=4  # Match CPU cores
DATABASE_MAX_CONNECTIONS=20
CACHE_MAX_POOL_SIZE=50
```

---

## 🔄 Deployment Checklist

### Pre-Deployment

- [ ] All tests pass
- [ ] Security audit completed
- [ ] Environment variables configured
- [ ] Database migrations tested
- [ ] Backup strategy in place
- [ ] Monitoring configured
- [ ] Load testing completed
- [ ] Rollback plan documented

---

### Deployment Steps

1. **Backup database:**
```bash
pg_dump $DATABASE_URL > backup-$(date +%Y%m%d).sql
```

2. **Deploy new version:**
```bash
# Blue-green deployment
docker tag vaultless-api:latest vaultless-api:$(git rev-parse --short HEAD)
docker push vaultless-api:$(git rev-parse --short HEAD)
kubectl set image deployment/vaultless-api api=vaultless-api:$(git rev-parse --short HEAD)
```

3. **Run migrations:**
```bash
kubectl exec -it deployment/vaultless-api -- /app/vaultless-api migrate
```

4. **Verify health:**
```bash
curl https://api.vaultless.io/health
```

5. **Monitor logs:**
```bash
kubectl logs -f deployment/vaultless-api
```

6. **Check metrics:**
- Error rates
- Response times
- Database performance

---

### Post-Deployment

- [ ] Health checks passing
- [ ] Metrics within normal range
- [ ] No error spikes
- [ ] Performance acceptable
- [ ] Users can authenticate
- [ ] Messages sending successfully
- [ ] Analytics loading correctly

---

## 🚨 Troubleshooting

### Issue: High Database CPU

**Diagnosis:**
```sql
-- Check slow queries
SELECT query, mean_exec_time, calls
FROM pg_stat_statements
ORDER BY mean_exec_time DESC
LIMIT 10;
```

**Fix:**
- Add missing indexes
- Optimize query plans
- Increase connection pool

---

### Issue: Cache Connection Failures

**Diagnosis:**
```bash
redis-cli -h dragonfly-host ping
```

**Fix:**
- Check network connectivity
- Verify credentials
- Increase connection pool size
- Add retry logic

---

### Issue: Out of Memory

**Diagnosis:**
```bash
docker stats
htop
```

**Fix:**
- Increase instance size
- Reduce connection pool
- Enable swap
- Optimize query memory usage

---

## 📈 Scaling Strategy

### Vertical Scaling (Up to 10k req/min)
- Increase CPU: 2 → 4 → 8 cores
- Increase RAM: 4GB → 8GB → 16GB
- Use faster disk (SSD/NVMe)

### Horizontal Scaling (Beyond 10k req/min)
1. Add more API instances
2. Use read replicas for database
3. Shard cache by key
4. Consider CDN for static content

---

## 🎯 Success Metrics

### Performance Targets
- API latency P95 < 200ms
- Database queries < 100ms
- Cache hit rate > 80%
- Uptime > 99.9%

### Business Metrics
- Messages per second
- Active API keys
- Revenue per user
- Support ticket rate

---

**Deployment guide maintained by:** DevOps Team  
**Last updated:** October 15, 2025  
**Next review:** November 15, 2025