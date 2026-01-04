# Vaultless API Integration

This document describes the integration between the Vaultless Next.js frontend and the Rust API backend.

## Overview

The integration connects the Next.js frontend (`vaultless-next`) with the Rust API (`vaultless-api/src/handlers/developer` routes) through a type-safe API client layer.

## Architecture

```
vaultless-next/
├── src/
│   ├── lib/
│   │   ├── apiClient.ts          # Base API client with error handling
│   │   └── api/
│   │       ├── auth.ts           # Authentication endpoints
│   │       ├── applications.ts   # Application CRUD & key management
│   │       ├── analytics.ts      # Analytics & metrics endpoints
│   │       ├── notifications.ts  # Notification management
│   │       └── index.ts          # API exports
│   ├── types/
│   │   └── api.ts                # TypeScript types matching Rust DTOs
│   └── contexts/
│       └── AuthContext.tsx       # Authentication state management
```

## Setup

### 1. Environment Configuration

Create a `.env.local` file in the `vaultless-next` directory:

```env
NEXT_PUBLIC_API_URL=http://localhost:8080
```

### 2. Provider Setup

The `AuthProvider` is already added to your `src/components/Providers.tsx`:

```tsx
import { AuthProvider } from '../contexts/AuthContext';

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <ThemeProvider attribute="class" defaultTheme="light" enableSystem>
      <AuthProvider>
        {children}
        <Toaster />
      </AuthProvider>
    </ThemeProvider>
  );
}
```

## Usage

### Authentication

#### Login

```tsx
'use client';
import { useAuth } from '@/contexts/AuthContext';

export function LoginForm() {
  const { login, isLoading } = useAuth();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      await login({
        email: 'user@example.com',
        password: 'password123',
      });
      // Redirect to dashboard
    } catch (error) {
      console.error('Login failed', error);
    }
  };

  return (
    <form onSubmit={handleSubmit}>
      <button disabled={isLoading}>
        {isLoading ? 'Logging in...' : 'Login'}
      </button>
    </form>
  );
}
```

#### Register

```tsx
const { register } = useAuth();

await register({
  email: 'user@example.com',
  password: 'password123',
  name: 'John Doe',
});
```

#### Logout

```tsx
const { logout } = useAuth();

await logout();
```

#### Get Current User

```tsx
const { user, isAuthenticated, isLoading } = useAuth();

if (isLoading) return <div>Loading...</div>;
if (!isAuthenticated) return <div>Not logged in</div>;

return <div>Welcome, {user?.name || user?.email}</div>;
```

### Protected Routes

Use the `useRequireAuth` hook to protect routes:

```tsx
'use client';
import { useRequireAuth } from '@/contexts/AuthContext';

export function DashboardPage() {
  const { isAuthenticated, isLoading } = useRequireAuth();

  if (isLoading) return <div>Loading...</div>;

  return <div>Dashboard content</div>;
}
```

### Applications

#### List Applications

```tsx
import { applicationsApi } from '@/lib/api';

const apps = await applicationsApi.list({ page: 1, pageSize: 20 });
console.log(apps.data);
```

#### Create Application

```tsx
import { applicationsApi } from '@/lib/api';

const newApp = await applicationsApi.create({
  name: 'My App',
  description: 'Application description',
});

console.log(newApp.secretKey); // Save this!
console.log(newApp.publishableKey);
```

#### Get Application with Keys

```tsx
import { applicationsApi } from '@/lib/api';

const app = await applicationsApi.getWithKeys(applicationId);
console.log(app.publishableKeys);
console.log(app.currentMonth);
```

#### Update Application

```tsx
await applicationsApi.update(applicationId, {
  name: 'Updated Name',
  description: 'Updated description',
});
```

#### Deactivate Application

```tsx
await applicationsApi.deactivate(applicationId);
```

### Key Management

#### Rotate Secret Key

```tsx
const result = await applicationsApi.rotateSecretKey(applicationId);
console.log(result.newSecretKey); // Save this!
```

#### Rotate Publishable Key

```tsx
await applicationsApi.rotatePublishableKey(applicationId, {
  publishableKey: 'pk_live_...', // Optional: rotate specific key
});
```

#### Add Publishable Key

```tsx
const result = await applicationsApi.addPublishableKey(applicationId);
console.log(result.newPublishableKey);
```

#### Deactivate Publishable Key

```tsx
await applicationsApi.deactivatePublishableKey(applicationId, {
  publishableKey: 'pk_live_...',
});
```

### Analytics

#### Get Quota Status

```tsx
import { analyticsApi } from '@/lib/api';

const status = await analyticsApi.getQuotaStatus(applicationId);
console.log(status.usagePercentage);
console.log(status.isOverQuota);
```

#### Get Cost Breakdown

```tsx
const breakdown = await analyticsApi.getCostBreakdown(applicationId);
console.log(breakdown.totalCostCents);
console.log(breakdown.breakdown);
```

#### Get Trends

```tsx
const trends = await analyticsApi.getTrends(applicationId);
console.log(trends.dailyAverageMessages);
console.log(trends.quotaTrend);
```

#### Export Data

```tsx
// Export as CSV (downloads file)
await analyticsApi.exportUsageAsCsv(applicationId, appName);

// Export as JSON
const data = await analyticsApi.exportUsage(applicationId, 'json');
```

### Notifications

#### List Notifications

```tsx
import { notificationsApi } from '@/lib/api';

const notifications = await notificationsApi.list({
  isRead: false,
  page: 1,
  pageSize: 20,
});
```

#### Get Unread Count

```tsx
const { unreadCount } = await notificationsApi.getUnreadCount();
```

#### Mark as Read

```tsx
await notificationsApi.markAsRead(notificationId);
```

#### Mark All as Read

```tsx
const result = await notificationsApi.markAllAsRead();
console.log(result.count); // Number of notifications marked
```

#### Delete Notification

```tsx
await notificationsApi.delete(notificationId);
```

## API Endpoints Reference

### Authentication (`/dev/auth`)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/dev/auth/register` | Register new user |
| POST | `/dev/auth/login` | Login user |
| POST | `/dev/auth/logout` | Logout user |
| POST | `/dev/auth/refresh-token` | Refresh access token |
| GET | `/dev/auth/me` | Get current user |
| POST | `/dev/auth/verify-email` | Verify email (POST) |
| GET | `/dev/auth/verify-email` | Verify email (GET) |
| POST | `/dev/auth/resend-verification-email` | Resend verification email |
| POST | `/dev/auth/request-password-reset` | Request password reset |
| POST | `/dev/auth/reset-password` | Reset password |

### Google OAuth (`/auth/google`)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/auth/google` | Initiate OAuth flow (redirect) |
| GET | `/auth/google/url` | Get auth URL (JSON) |
| GET | `/auth/google/callback` | Handle OAuth callback |

### Applications (`/dev/applications`)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/dev/applications` | List applications |
| POST | `/dev/applications` | Create application |
| GET | `/dev/applications/{id}/with_keys` | Get with keys |
| PATCH | `/api/applications/{id}` | Update application |
| DELETE | `/api/applications/{id}` | Deactivate application |
| GET | `/dev/applications/usage-summary` | Get usage summary |
| GET | `/dev/applications/quota-warnings` | Get quota warnings |
| GET | `/dev/applications/{id}/analytics` | Get analytics |
| POST | `/dev/applications/{id}/keys/secret/rotate` | Rotate secret key |
| POST | `/dev/applications/{id}/keys/publishable/rotate` | Rotate publishable key |
| POST | `/dev/applications/{id}/keys/publishable` | Add publishable key |
| POST | `/dev/applications/{id}/keys/publishable/deactivate` | Deactivate publishable key |

### Analytics

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/dev/applications/{id}/quota-status` | Get quota status |
| GET | `/dev/applications/{id}/costs` | Get cost breakdown |
| GET | `/dev/applications/{id}/export` | Export usage data |
| GET | `/dev/applications/{id}/trends` | Get usage trends |

### Notifications (`/dev/notifications`)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/dev/notifications` | List notifications |
| GET | `/dev/notifications/{id}` | Get notification |
| GET | `/dev/notifications/unread-count` | Get unread count |
| GET | `/dev/notifications/summary` | Get notification summary |
| POST | `/dev/notifications/{id}/read` | Mark as read |
| POST | `/dev/notifications/read-all` | Mark all as read |
| DELETE | `/dev/notifications/{id}` | Delete notification |
| DELETE | `/dev/notifications/read` | Delete all read |

## Error Handling

All API methods throw `ApiException` errors:

```tsx
import { ApiException } from '@/types/api';

try {
  await applicationsApi.create({ name: 'Test' });
} catch (error) {
  if (error instanceof ApiException) {
    console.error('Status:', error.status);
    console.error('Code:', error.code);
    console.error('Message:', error.message);
  }
}
```

## Token Management

The `AuthContext` automatically handles:

- Storing access and refresh tokens in `localStorage`
- Attaching the access token to API requests
- Refreshing tokens when needed
- Logging out on token refresh failure

## Type Safety

All API responses are fully typed. Import types from `@/types/api`:

```tsx
import type { ApplicationResponse, UserResponse } from '@/types/api';

const app: ApplicationResponse = await applicationsApi.getWithKeys(id);
```

## Examples

### Complete Application List Page

```tsx
'use client';
import { useEffect, useState } from 'react';
import { useAuth } from '@/contexts/AuthContext';
import { applicationsApi } from '@/lib/api';
import type { ApplicationWithUsage } from '@/types/api';

export function ApplicationsList() {
  const { isAuthenticated } = useAuth();
  const [apps, setApps] = useState<ApplicationWithUsage[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (isAuthenticated) {
      loadApps();
    }
  }, [isAuthenticated]);

  const loadApps = async () => {
    try {
      const response = await applicationsApi.list({ page: 1, pageSize: 20 });
      const detailedApps = await Promise.all(
        response.data.map((app) =>
          applicationsApi.getWithKeys(app.id)
        )
      );
      setApps(detailedApps);
    } catch (error) {
      console.error('Failed to load apps', error);
    } finally {
      setLoading(false);
    }
  };

  if (loading) return <div>Loading...</div>;

  return (
    <div>
      {apps.map((app) => (
        <div key={app.applicationId}>
          <h3>{app.name}</h3>
          <p>Messages: {app.currentMonthMessagesSent}</p>
        </div>
      ))}
    </div>
  );
}
```

## Development

### Running the API

Make sure your Rust API is running on port 8080:

```bash
cd vaultless-api
cargo run
```

### Running Next.js

```bash
cd vaultless-next
npm run dev
```

The frontend will connect to the API using the `NEXT_PUBLIC_API_URL` environment variable.

## Troubleshooting

### CORS Issues

If you encounter CORS errors, make sure your Rust API is configured to allow requests from your Next.js frontend domain. Check the CORS middleware in your API configuration.

### Token Errors

If you get 401 Unauthorized errors:

1. Check that `access_token` exists in `localStorage`
2. Verify the token hasn't expired
3. Ensure the API is running and accessible

### Type Mismatches

If TypeScript complains about type mismatches, ensure the Rust DTOs and TypeScript types are in sync. The types in `src/types/api.ts` should match the structures returned by the Rust API.

## Contributing

When adding new API endpoints:

1. Add TypeScript types to `src/types/api.ts`
2. Add API methods to the appropriate file in `src/lib/api/`
3. Update this documentation with the new endpoints

## License

This integration is part of the Vaultless project.
