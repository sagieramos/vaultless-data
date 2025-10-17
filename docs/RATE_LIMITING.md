# 🚦 Rate Limiting Documentation

## Overview

Vaultless Data uses a distributed, Redis-backed sliding window rate limiter to protect infrastructure and enforce tier-based usage limits.

---

## 🎯 Rate Limit Tiers

| Tier           | Requests/Minute | Burst Allowance | Monthly Messages |
| -------------- | --------------- | --------------- | ---------------- |
| **Free**       | 60              | 72              | 1,000            |
| **Starter**    | 300             | 360             | 50,000           |
| **Pro**        | 1,000           | 1,200           | 500,000          |
| **Enterprise** | 10,000          | 12,000          | Unlimited        |

**Burst Allowance:** 20% above base limit for short periods

---

## 🔍 How It Works

### Sliding Window Algorithm

```
Time:     [-------- 60 seconds --------]
          |                            |
Requests: • • •     •     • •          •
          1 2 3     4     5 6          7

Window slides continuously, counting requests in last 60 seconds
```

**Benefits:**
- More accurate than fixed windows
- Prevents burst abuse at window boundaries
- Fair distribution over time

---

## 📊 Rate Limit Headers

Every API response includes rate limit information:

```http
HTTP/1.1 200 OK
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1697385660
```

### When Rate Limited

```http
HTTP/1.1 429 Too Many Requests
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1697385660
Retry-After: 30

{
  "error": {
    "message": "Rate limit exceeded. Limit: 1000 requests per minute. Try again in 30 seconds.",
    "code": "RATE_LIMIT_EXCEEDED",
    "status": 429
  }
}
```

---

## 🛡️ Rate Limit Types

### 1. Global API Key Limit

Applied to all authenticated endpoints.

**Limit:** Based on subscription tier  
**Scope:** Per API key  
**Window:** 60 seconds  

**Example:**
```bash
# Pro tier: 1000 req/min across all endpoints
curl -H "Authorization: vlt_pro_key" https://api.vaultless.io/api/v1/messages/send
```

---

### 2. Endpoint-Specific Limits

Stricter limits for expensive operations.

**Send Message:** 50% of global limit  
**Analytics:** 150% of global limit  

**Example:**
```bash
# Pro tier: 500 req/min for sending messages
curl -X POST -H "Authorization: vlt_pro_key" \
  https://api.vaultless.io/api/v1/messages/send \
  -d '{...}'
```

---

### 3. IP-Based Limits

For unauthenticated endpoints (health checks, admin).

**Limit:** 100 req/min per IP  
**Scope:** Per IP address  
**Window:** 60 seconds  

**Example:**
```bash
# Health endpoint: 100 req/min per IP
curl https://api.vaultless.io/health
```

---

## 🔧 API Endpoints

### Get Your Rate Limit Status

**GET** `/api/v1/rate-limit/status`

**Authentication:** Required

**Response:**
```json
{
  "current_limit": {
    "requests_per_minute": 1000,
    "current_usage": 45,
    "remaining": 955,
    "usage_percentage": 4.5,
    "window_start": 1697385600,
    "window_end": 1697385660
  },
  "violations": {
    "count_24h": 2,
    "severity": "low",
    "warning": null
  },
  "recommendations": [
    "Your rate limit usage looks healthy!"
  ]
}
```

---

### Get Rate Limit History

**GET** `/api/v1/rate-limit/history`

**Authentication:** Required

**Response:**
```json
{
  "hourly_data": [
    {
      "hour": "2025-10-15T10:00:00Z",
      "total_requests": 450,
      "rate_limit_hits": 2
    },
    {
      "hour": "2025-10-15T09:00:00Z",
      "total_requests": 380,
      "rate_limit_hits": 0
    }
  ],
  "summary": {
    "total_requests": 830,
    "total_violations": 2,
    "hours_tracked": 2
  }
}
```

---

### Admin: Get Key Rate Limit Status

**GET** `/api/v1/admin/keys/:key_id/rate-limit`

**Authentication:** None (dev), Admin required (prod)

**Response:**
```json
{
  "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
  "rate_limit_per_minute": 1000,
  "current_usage": 45,
  "remaining": 955,
  "violations_24h": 2,
  "window_start": 1697385600,
  "window_end": 1697385660
}
```

---

### Admin: Reset Rate Limit

**POST** `/api/v1/admin/keys/:key_id/rate-limit/reset`

**Authentication:** None (dev), Admin required (prod)

**Response:** `204 No Content`

**Use case:** Emergency reset for legitimate high-traffic events

---

## 🎨 Client Implementation

### Best Practices

#### 1. Respect Rate Limit Headers

```javascript
async function makeRequest(url, options) {
  const response = await fetch(url, options);
  
  const limit = parseInt(response.headers.get('X-RateLimit-Limit'));
  const remaining = parseInt(response.headers.get('X-RateLimit-Remaining'));
  const reset = parseInt(response.headers.get('X-RateLimit-Reset'));
  
  console.log(`Rate limit: ${remaining}/${limit} remaining`);
  console.log(`Resets at: ${new Date(reset * 1000)}`);
  
  if (response.status === 429) {
    const retryAfter = parseInt(response.headers.get('Retry-After'));
    console.log(`Rate limited! Retry after ${retryAfter} seconds`);
    await sleep(retryAfter * 1000);
    return makeRequest(url, options); // Retry
  }
  
  return response;
}
```

---

#### 2. Implement Exponential Backoff

```javascript
async function requestWithBackoff(url, options, maxRetries = 3) {
  for (let i = 0; i < maxRetries; i++) {
    const response = await fetch(url, options);
    
    if (response.status !== 429) {
      return response;
    }
    
    const retryAfter = parseInt(response.headers.get('Retry-After') || '1');
    const backoffTime = Math.min(retryAfter * Math.pow(2, i), 60);
    
    console.log(`Attempt ${i + 1}: Backing off for ${backoffTime}s`);
    await sleep(backoffTime * 1000);
  }
  
  throw new Error('Max retries exceeded');
}
```

---

#### 3. Batch Requests

Instead of sending messages one-by-one:

```javascript
// ❌ Bad: 100 individual requests
for (const message of messages) {
  await sendMessage(message);
}

// ✅ Good: Batch into groups
const batches = chunk(messages, 10);
for (const batch of batches) {
  await sendBatch(batch);
  await sleep(1000); // Pace requests
}
```

---

#### 4. Monitor Usage Proactively

```javascript
async function checkRateLimit() {
  const response = await fetch('/api/v1/rate-limit/status', {
    headers: { 'Authorization': apiKey }
  });
  
  const status = await response.json();
  
  if (status.current_limit.usage_percentage > 80) {
    console.warn('⚠️  Approaching rate limit!');
    // Slow down requests
  }
  
  if (status.violations.severity === 'high') {
    console.error('🚨 Frequent rate limiting detected!');
    // Consider upgrading plan
  }
}

// Check every minute
setInterval(checkRateLimit, 60000);
```

---

## 🚨 Troubleshooting

### Issue: Frequent 429 Errors

**Diagnosis:**
```bash
curl -H "Authorization: YOUR_KEY" \
  https://api.vaultless.io/api/v1/rate-limit/history
```

**Solutions:**
1. Implement exponential backoff
2. Batch requests
3. Cache responses when possible
4. Upgrade to higher tier

---

### Issue: Unexpectedly Low Remaining Count

**Possible Causes:**
- Multiple clients using same API key
- Automated scripts running in background
- Forgot to implement backoff

**Solution:**
```bash
# Check violations
curl -H "Authorization: YOUR_KEY" \
  https://api.vaultless.io/api/v1/rate-limit/status
```

---

### Issue: Need Temporary Limit Increase

**For legitimate high-traffic events:**

```bash
# Contact support or use admin endpoint
curl -X POST https://api.vaultless.io/api/v1/admin/keys/YOUR_KEY_ID/rate-limit/reset
```

**Note:** This only resets current window, doesn't change limit

---

## 💰 Upgrading for Higher Limits

### When to Upgrade

- Using >80% of rate limit regularly
- Experiencing frequent 429 errors
- Need to support more concurrent users
- Running batch jobs

### Tier Comparison

```
Free → Starter:  5x rate limit increase  ($29/month)
Starter → Pro:   3.3x increase           ($149/month)
Pro → Enterprise: 10x increase           (Custom pricing)
```

### Upgrade Process

1. Go to dashboard: `https://dashboard.vaultless.io`
2. Select your API key
3. Choose new tier
4. Confirm payment
5. Limit increases immediately

---

## 🔍 Monitoring & Alerts

### Recommended Monitoring

**Alert when:**
- Violation count > 10 in 1 hour
- Usage > 80% for 5 minutes
- Rate limit hit > 100 times in 24 hours

**Metrics to Track:**
- Requests per minute (current)
- Rate limit violations per hour
- Usage percentage trend
- 429 error rate

---

## 📈 Performance Impact

### Overhead

- **Latency:** <1ms per request
- **Memory:** ~100 bytes per request in window
- **Network:** 1 Redis roundtrip per request

### Optimization

**Redis Connection Pool:**
```
Max connections: 50
Idle timeout: 30s
```

**Lua Scripts:**
- All rate limit logic runs in Redis
- Atomic operations prevent race conditions
- No network overhead for calculations

---

## 🎯 FAQ

### Q: Can I have different limits for different endpoints?

A: Yes! Endpoint-specific limits are automatically applied:
- Message sending: 50% of global limit
- Analytics: 150% of global limit

---

### Q: What happens if I briefly exceed the limit?

A: The sliding window allows 20% burst capacity. If you exceed this, requests return 429 until the window slides forward.

---

### Q: Can I increase my limit temporarily?

A: Contact support for temporary increases during legitimate high-traffic events. For permanent increases, upgrade your tier.

---

### Q: Do rate limits apply to failed requests?

A: Yes. All requests count toward your limit, including 400/404/500 errors.

---

### Q: How do I avoid rate limiting?

1. Implement exponential backoff
2. Cache responses
3. Batch operations
4. Monitor usage proactively
5. Upgrade tier if needed

---

## 🔗 Related Documentation

- [API Documentation](./API_DOCUMENTATION.md)
- [Error Handling](./ERROR_HANDLING.md)
- [Best Practices](./BEST_PRACTICES.md)

---

**Last Updated:** October 15, 2025  
**Version:** 1.0.0