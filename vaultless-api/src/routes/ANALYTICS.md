| Header                          | Description                                           | Required |
| ------------------------------- | ----------------------------------------------------- | -------- |
| `Authorization: Bearer <token>` | User’s session JWT or OAuth token                     | ✅        |
| `X-Api-Key-Id`                  | UUID of the API key whose analytics you want to query | ✅        |

**If either header is missing or invalid, the request will be rejected with 401 Unauthorized or 403 Forbidden**

---

| Method | Endpoint                   | Description                                                                               |
| ------ | -------------------------- | ----------------------------------------------------------------------------------------- |
| `GET`  | `/analytics/overview`      | Returns summarized usage metrics (requests, errors, bandwidth) for the specified API key. |
| `GET`  | `/analytics/daily`         | Returns time-series usage data (per day) for charting and trend analysis.                 |
| `GET`  | `/analytics/top-endpoints` | Lists the top requested endpoints for the API key.                                        |
| `GET`  | `/analytics/errors`        | Returns error statistics grouped by error type or status code.                            |

___

### 🧠 Example Request
```json
curl -X GET https://api.example.com/analytics/overview 
  -H "Authorization: Bearer eyJhbGciOi..." 
  -H "X-Api-Key-Id: 550e8400-e..."
```

### ✅ Example Response
```json
{
  "success": true,
  "data": {
    "overview": {
      "total_messages_sent": 15200,
      "total_messages_received": 14890,
      "total_proofs_verified": 224,
      "total_bytes_stored": 1940500,
      "total_rate_limit_hits": 12,
      "period_start": "2025-10-01T00:00:00Z",
      "period_end": "2025-10-19T00:00:00Z"
    },
    "trends": {
      "daily_usage": [
        { "day": "2025-10-10T00:00:00Z", "total_messages_sent": 600 },
        { "day": "2025-10-11T00:00:00Z", "total_messages_sent": 720 },
        { "day": "2025-10-12T00:00:00Z", "total_messages_sent": 820 }
      ]
    },
    "cost_breakdown": {
      "messages_cost_cents": 124,
      "storage_cost_cents": 85,
      "verification_cost_cents": 12,
      "total_cost_cents": 221,
      "overage_cost_cents": 0
    },
    "tier_info": {
      "current_tier": "starter",
      "monthly_quota": 100000,
      "rate_limit_per_minute": 120,
      "retention_days": 30,
      "features": [
        "basic_analytics",
        "standard_support"
      ]
    },
    "quota_status": {
      "messages_used": 52000,
      "messages_limit": 100000,
      "usage_percentage": 52.0,
      "is_over_quota": false,
      "overage_count": 0,
      "resets_at": "2025-11-01T00:00:00Z"
    },
    "recent_activity": [
      {
        "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
        "day": "2025-10-18T00:00:00Z",
        "total_messages_sent": 540,
        "total_messages_received": 530,
        "total_proofs_verified": 6,
        "total_bytes_stored": 14500,
        "total_rate_limit_hits": 0,
        "total_estimated_cost_cents": 9
      },
      {
        "api_key_id": "550e8400-e29b-41d4-a716-446655440000",
        "day": "2025-10-19T00:00:00Z",
        "total_messages_sent": 610,
        "total_messages_received": 602,
        "total_proofs_verified": 7,
        "total_bytes_stored": 15200,
        "total_rate_limit_hits": 1,
        "total_estimated_cost_cents": 11
      }
    ]
  },
  "upgrade_message": null
}
```

