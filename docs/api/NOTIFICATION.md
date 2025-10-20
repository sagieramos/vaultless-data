# 🔔 Vaultless Data Notification System

## 📋 Overview

The notification system provides **automated alerts, usage reports, and promotional messages** to users. Users can **read, filter, and delete** notifications, but cannot create them (system-generated only).

---

## 🎯 Features

### ✅ User Capabilities
- ✅ List notifications with filters (type, severity, read status)
- ✅ View individual notification details
- ✅ Mark notifications as read (individual or bulk)
- ✅ Delete notifications (individual or bulk delete read)
- ✅ Get unread count (for UI badge)
- ✅ Get notification statistics
- ✅ **Real-time stream via SSE (Pro+ feature)**

### 🚫 User Restrictions
- ❌ Cannot create notifications (system-only)
- ❌ Cannot update notification content
- ❌ Cannot view other users' notifications

---

## 📡 API Endpoints

### List Notifications
```bash
GET /notifications?notification_type=quota_warning&is_read=false&limit=20&offset=0
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "data": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "user_id": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
      "title": "Quota Warning",
      "message": "You've used 85.5% (42750/50000) of your monthly message quota.",
      "notification_type": "quota_warning",
      "severity": "warning",
      "action_url": "/dashboard/usage",
      "metadata": {
        "usage_percentage": 85.5,
        "messages_used": 42750,
        "messages_limit": 50000
      },
      "is_read": false,
      "created_at": "2025-01-19T10:30:00Z",
      "read_at": null,
      "expires_at": "2025-01-26T10:30:00Z"
    }
  ],
  "pagination": {
    "total": 15,
    "limit": 20,
    "offset": 0,
    "has_more": false
  }
}
```

### Get Notification by ID
```bash
GET /notifications/:id
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "data": { /* notification object */ }
}
```

### Mark as Read
```bash
PATCH /notifications/:id/read
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "data": { /* updated notification */ }
}
```

### Mark All as Read
```bash
POST /notifications/mark-all-read
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "affected_count": 12,
  "message": "Marked 12 notification(s) as read"
}
```

### Delete Notification
```bash
DELETE /notifications/:id
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "message": "Notification deleted successfully"
}
```

### Delete All Read Notifications
```bash
DELETE /notifications/read
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "affected_count": 8,
  "message": "Deleted 8 read notification(s)"
}
```

### Get Unread Count
```bash
GET /notifications/unread/count
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "unread_count": 5
}
```

### Get Statistics
```bash
GET /notifications/stats
Authorization: Bearer <jwt_token>

Response:
{
  "success": true,
  "data": {
    "total": 25,
    "unread": 5,
    "critical": 2,
    "warnings": 8,
    "last_24h": 3
  }
}
```

### Real-Time Stream (Pro+)
```bash
GET /notifications/stream
Authorization: Bearer <jwt_token>

# Server-Sent Events (SSE) stream
# Pushes new notifications in real-time every 5 seconds

data: [{"id":"...","title":"New Message",...}]

data: heartbeat

data: [{"id":"...","title":"Quota Alert",...}]
```

---

## 🏷️ Notification Types

| Type               | Description               | Severity | Auto-Generated   |
| ------------------ | ------------------------- | -------- | ---------------- |
| `quota_warning`    | 80% or 90% quota usage    | Warning  | ✅ Hourly check   |
| `quota_exceeded`   | Over monthly quota        | Critical | ✅ Hourly check   |
| `billing_alert`    | Payment failures          | Critical | ✅ Stripe webhook |
| `security_alert`   | Suspicious activity       | Critical | ✅ Manual/Auto    |
| `system_update`    | New features/maintenance  | Info     | ✅ Manual         |
| `marketing_offer`  | Promotional offers        | Info     | ✅ Manual         |
| `api_key_expiring` | Key expires in 7/3/1 days | Warning  | ✅ Daily check    |
| `usage_report`     | Monthly usage summary     | Info     | ✅ 1st of month   |

---

## 🎨 Frontend Integration

### React Component Example

```typescript
import { useEffect, useState } from 'react';
import { useAuth } from './auth-context';

interface Notification {
  id: string;
  title: string;
  message: string;
  notification_type: string;
  severity: 'info' | 'warning' | 'critical';
  is_read: boolean;
  created_at: string;
  action_url?: string;
}

export function NotificationBell() {
  const { token } = useAuth();
  const [unreadCount, setUnreadCount] = useState(0);
  const [notifications, setNotifications] = useState<Notification[]>([]);
  const [isOpen, setIsOpen] = useState(false);

  // Fetch unread count
  useEffect(() => {
    fetch('/notifications/unread/count', {
      headers: { 'Authorization': `Bearer ${token}` }
    })
      .then(res => res.json())
      .then(data => setUnreadCount(data.unread_count));
  }, [token]);

  // Fetch notifications when dropdown opens
  const loadNotifications = async () => {
    const res = await fetch('/notifications?is_read=false&limit=10', {
      headers: { 'Authorization': `Bearer ${token}` }
    });
    const data = await res.json();
    setNotifications(data.data);
    setIsOpen(true);
  };

  // Mark as read
  const markAsRead = async (id: string) => {
    await fetch(`/notifications/${id}/read`, {
      method: 'PATCH',
      headers: { 'Authorization': `Bearer ${token}` }
    });
    
    setNotifications(prev =>
      prev.map(n => n.id === id ? { ...n, is_read: true } : n)
    );
    setUnreadCount(prev => Math.max(0, prev - 1));
  };

  return (
    <div className="notification-bell">
      <button onClick={loadNotifications}>
        🔔 {unreadCount > 0 && <span className="badge">{unreadCount}</span>}
      </button>

      {isOpen && (
        <div className="notification-dropdown">
          {notifications.map(notification => (
            <div
              key={notification.id}
              className={`notification-item severity-${notification.severity}`}
              onClick={() => markAsRead(notification.id)}
            >
              <h4>{notification.title}</h4>
              <p>{notification.message}</p>
              <small>{new Date(notification.created_at).toLocaleString()}</small>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
```

### Real-Time SSE Integration (Pro+)

```typescript
export function useNotificationStream() {
  const { token, tier } = useAuth();
  const [notifications, setNotifications] = useState<Notification[]>([]);

  useEffect(() => {
    if (tier !== 'pro' && tier !== 'enterprise') {
      return; // Feature not available
    }

    const eventSource = new EventSource('/notifications/stream', {
      headers: { 'Authorization': `Bearer ${token}` }
    });

    eventSource.onmessage = (event) => {
      if (event.data === 'heartbeat') return;
      
      const newNotifications = JSON.parse(event.data);
      setNotifications(prev => [...newNotifications, ...prev]);
      
      // Show browser notification
      if (Notification.permission === 'granted') {
        new Notification('New Alert', {
          body: newNotifications[0]?.message,
          icon: '/logo.png'
        });
      }
    };

    return () => eventSource.close();
  }, [token, tier]);

  return notifications;
}
```

---

## 🤖 System-Generated Notifications

### Automated Triggers

**Quota Alerts** (runs hourly)
```rust
// Automatically triggered by background worker
// - 80% usage → Warning notification
// - 90% usage → Critical notification
// - 100%+ usage → Over-quota notification with cost estimate

// Background worker in main.rs:
tokio::spawn(notification_worker(db_pool.clone(), cache_service.clone()));
```

**API Key Expiry Alerts** (runs daily)
```rust
// Checks for keys expiring in 7, 3, and 1 days
// Sends reminder notifications automatically
```

**Monthly Usage Reports** (1st of each month)
```rust
// Aggregates previous month's usage
// Sends summary with costs and upgrade suggestions

tokio::spawn(monthly_report_worker(db_pool.clone(), cache_service.clone()));
```

**Billing Alerts** (triggered by Stripe webhooks)
```rust
// Payment failures, subscription changes
// Automatically created via billing service
```

---

## 💰 Monetization Strategy

### Feature Gating by Tier

| Feature                       | Free   | Starter | Pro     | Enterprise |
| ----------------------------- | ------ | ------- | ------- | ---------- |
| **Basic Notifications**       | ✅      | ✅       | ✅       | ✅          |
| **Notification History**      | 7 days | 30 days | 90 days | Unlimited  |
| **Email Notifications**       | ❌      | ✅       | ✅       | ✅          |
| **Real-time SSE Stream**      | ❌      | ❌       | ✅       | ✅          |
| **Webhook Delivery**          | ❌      | ❌       | ❌       | ✅          |
| **Custom Alerts**             | ❌      | ❌       | 3 rules | Unlimited  |
| **Slack/Discord Integration** | ❌      | ❌       | ❌       | ✅          |

### Revenue Opportunities

1. **Upgrade Prompts in Notifications**
   ```json
   {
     "title": "Quota Warning",
     "message": "You're at 85% usage. Upgrade to Pro for 10x more messages.",
     "action_url": "/dashboard/upgrade?promo=QUOTA85",
     "metadata": {
       "upgrade_discount": 20,
       "promo_code": "QUOTA85"
     }
   }
   ```

2. **Targeted Promotional Notifications**
   - Send offers to Starter users approaching quota
   - Black Friday deals for Free tier users
   - Enterprise upsell for Pro users with high usage

3. **Notification Delivery as a Service**
   - Charge for webhook delivery (Enterprise feature)
   - Charge for SMS/push notifications ($0.05 per notification)
   - Third-party integrations (Slack, Discord, PagerDuty)

---

## 🔧 Implementation Guide

### 1. Add to Services Module

```rust
// vaultless-api/src/services/mod.rs
pub mod analytics;
pub mod cache;
pub mod notification_service;
pub mod rate_limiter;
pub mod token;

pub use notification_service::{NotificationService, notification_worker, monthly_report_worker};
```

### 2. Update Handlers Module

```rust
// vaultless-api/src/handlers/mod.rs
pub mod analytics;
pub mod api_keys;
pub mod auth;
pub mod dto;
pub mod messages;
pub mod notifications;
pub mod proofs;

pub use notifications::*;
```

### 3. Register Routes

```rust
// vaultless-api/src/routes/mod.rs
pub mod analytics;
pub mod health;
pub mod notifications;

use axum::Router;
use crate::state::AppState;

pub fn app_routes(state: AppState) -> Router {
    Router::new()
        .nest("/analytics", analytics_routes())
        .nest("/notifications", notification_routes())
        .route("/health", axum::routing::get(health_check))
        .with_state(state)
}
```

### 4. Start Background Workers

```rust
// vaultless-api/src/main.rs
use services::{notification_worker, monthly_report_worker};

#[tokio::main]
async fn main() -> Result<()> {
    // ... setup db and cache ...
    
    // Start notification background workers
    tokio::spawn(notification_worker(
        db_pool.clone(),
        cache_service.clone()
    ));
    
    tokio::spawn(monthly_report_worker(
        db_pool.clone(),
        cache_service.clone()
    ));
    
    // ... start server ...
}
```

### 5. Update Core Models Module

```rust
// vaultless-core/src/models/mod.rs
pub mod api_key;
pub mod auth;
pub mod message;
pub mod notification;
pub mod proof;
pub mod usage;
pub mod usage_timescale;

pub use notification::{
    Notification, NotificationBuilder, NotificationFilters,
    NotificationSeverity, NotificationStats, NotificationType,
};
```

---

## 🧪 Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_quota_warning() {
        let pool = setup_test_db().await;
        let user_id = create_test_user(&pool).await;

        let notification = NotificationBuilder::quota_warning(
            &pool,
            user_id,
            85.5,
            42750,
            50000,
        )
        .await
        .unwrap();

        assert_eq!(notification.notification_type, NotificationType::QuotaWarning);
        assert_eq!(notification.severity, NotificationSeverity::Warning);
        assert!(notification.message.contains("85.5%"));
    }

    #[tokio::test]
    async fn test_mark_as_read() {
        let pool = setup_test_db().await;
        let user_id = create_test_user(&pool).await;
        
        let notification = create_test_notification(&pool, user_id).await;
        assert!(!notification.is_read);

        let updated = Notification::mark_as_read(&pool, notification.id, user_id)
            .await
            .unwrap();

        assert!(updated.is_read);
        assert!(updated.read_at.is_some());
    }

    #[tokio::test]
    async fn test_user_cannot_access_other_users_notifications() {
        let pool = setup_test_db().await;
        let user1_id = create_test_user(&pool).await;
        let user2_id = create_test_user(&pool).await;

        let notification = create_test_notification(&pool, user1_id).await;

        let result = Notification::find_by_id(&pool, notification.id, user2_id).await;
        assert!(result.is_err()); // Should fail - not authorized
    }

    #[tokio::test]
    async fn test_cleanup_expired_notifications() {
        let pool = setup_test_db().await;
        
        // Create expired notification
        let expired_notification = create_expired_notification(&pool).await;
        
        let deleted_count = Notification::cleanup_expired(&pool).await.unwrap();
        assert_eq!(deleted_count, 1);

        // Verify notification was deleted
        let result = Notification::find_by_id(&pool, expired_notification.id, expired_notification.user_id).await;
        assert!(result.is_err());
    }
}
```

### Integration Tests

```bash
# Test notification listing
curl -H "Authorization: Bearer $JWT_TOKEN" \
  http://localhost:3000/notifications

# Test filtering
curl -H "Authorization: Bearer $JWT_TOKEN" \
  "http://localhost:3000/notifications?notification_type=quota_warning&severity=critical"

# Test mark as read
curl -X PATCH \
  -H "Authorization: Bearer $JWT_TOKEN" \
  http://localhost:3000/notifications/$NOTIFICATION_ID/read

# Test bulk delete
curl -X DELETE \
  -H "Authorization: Bearer $JWT_TOKEN" \
  http://localhost:3000/notifications/read

# Test SSE stream (Pro tier required)
curl -N -H "Authorization: Bearer $JWT_TOKEN" \
  http://localhost:3000/notifications/stream
```

---

## 🚀 Advanced Features (Future Enhancements)

### 1. **Custom Alert Rules** (Pro+ feature)
Users can create custom triggers:
```json
{
  "rule_name": "High Traffic Alert",
  "condition": "messages_sent > 1000 per hour",
  "notification_type": "custom",
  "action": "email_and_sms"
}
```

### 2. **Webhook Delivery** (Enterprise feature)
Forward notifications to user-defined webhooks:
```json
POST https://user-webhook.com/notifications
{
  "event": "notification.created",
  "data": { /* notification object */ }
}
```

### 3. **Digest Mode** (All tiers)
Batch notifications into daily/weekly digests:
```
"You have 15 unread notifications from this week:
- 5 quota warnings
- 3 system updates
- 7 usage reports"
```

### 4. **Smart Notification Grouping**
Group related notifications:
```
"Quota Alerts (3)"
  ├─ 80% usage warning
  ├─ 90% usage critical
  └─ 100% quota exceeded
```

### 5. **In-App Notification Center**
Rich notification UI with:
- Filtering by type/severity
- Search functionality
- Bulk actions (mark all read, delete selected)
- Notification preferences

### 6. **Multi-Channel Delivery**
- Email notifications
- SMS alerts (Twilio integration)
- Push notifications (mobile apps)
- Slack/Discord webhooks
- PagerDuty integration (Enterprise)

---

## 📊 Analytics & Monitoring

### Notification Metrics to Track

```sql
-- Most common notification types
SELECT notification_type, COUNT(*) as count
FROM notifications
WHERE created_at > NOW() - INTERVAL '30 days'
GROUP BY notification_type
ORDER BY count DESC;

-- Average time to read notifications
SELECT 
    notification_type,
    AVG(EXTRACT(EPOCH FROM (read_at - created_at))/3600) as avg_hours_to_read
FROM notifications
WHERE is_read = TRUE AND read_at IS NOT NULL
GROUP BY notification_type;

-- Notification engagement rate
SELECT 
    notification_type,
    COUNT(*) as total,
    COUNT(*) FILTER (WHERE is_read = TRUE) as read_count,
    (COUNT(*) FILTER (WHERE is_read = TRUE)::FLOAT / COUNT(*)) * 100 as read_rate
FROM notifications
GROUP BY notification_type;
```

### Business Intelligence

- Track which notification types drive the most upgrades
- Monitor notification fatigue (declining read rates)
- A/B test notification copy for conversion optimization
- Measure time from quota alert to upgrade

---

## 🔒 Security Considerations

1. **Authorization**
   - Users can only access their own notifications
   - JWT token validation on all endpoints
   - Row-level security enforced at database level

2. **Rate Limiting**
   - Prevent notification spam
   - Limit SSE connections per user
   - Throttle background worker frequency

3. **Data Retention**
   - Auto-delete expired notifications
   - Clean up old read notifications (90-day retention)
   - GDPR compliance for user data deletion

4. **Input Validation**
   - Sanitize notification content
   - Validate action URLs
   - Prevent XSS in metadata JSON

---

## 📝 Checklist for Production

- [ ] Run migration: `sqlx migrate run`
- [ ] Add notification types to enums
- [ ] Deploy background workers
- [ ] Set up monitoring for notification delivery
- [ ] Configure email service (SendGrid/Postmark)
- [ ] Test SSE stream with 100+ concurrent connections
- [ ] Set up Sentry for error tracking
- [ ] Add Prometheus metrics for notification counts
- [ ] Create admin dashboard for broadcasting system updates
- [ ] Document notification webhooks for Enterprise clients

---

## 💡 Business Use Cases

### 1. **Conversion Funnel**
```
Free User → Quota Warning (80%) → "Upgrade for 50K messages" CTA → Starter Tier
```

### 2. **Retention Campaign**
```
Inactive User → "You haven't sent messages in 30 days" → Promotional offer → Re-engagement
```

### 3. **Upsell Opportunities**
```
Starter User → Approaching quota → "Upgrade to Pro for 10x capacity" → Pro Tier
```

### 4. **Customer Success**
```
Enterprise User → API key expiring → Proactive renewal reminder → Prevented churn
```

---

## 🎯 Key Takeaways

✅ **Users have full CRUD control** (read, delete) over their notifications
✅ **System-generated only** - prevents spam and maintains trust
✅ **Tier-gated features** (SSE, webhooks) drive upgrades
✅ **Background workers** automate quota alerts and reports
✅ **Real-time capabilities** (SSE) provide premium experience
✅ **Extensible metadata** allows rich, actionable notifications
✅ **Auto-expiry** keeps notifications relevant and database clean

---

**Need help with implementation?** Check the code artifacts or ask for specific examples! 🚀