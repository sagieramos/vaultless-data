# Vaultless Developer API Reference

This document provides detailed documentation for the Developer Portal API endpoints, including request/response structures, query parameters, and UI implementation guidance.

---

## Table of Contents

1. [Authentication Endpoints](#1-authentication-endpoints)
2. [Google OAuth Endpoints](#2-google-oauth-endpoints)
3. [Application Management](#3-application-management)
4. [Analytics & Usage](#4-analytics--usage)
5. [Notifications](#5-notifications)
6. [Common Response Patterns](#6-common-response-patterns)

---

## Base URL

```
Production: https://api.vaultless.io
Development: http://localhost:3000
```

## Authentication Header

Most endpoints require authentication:
```
Authorization: Bearer <access_token>
```

---

# 1. Authentication Endpoints

Base path: `/dev/auth`

---

## 1.1 Register User

Creates a new developer account.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/register` |
| **Auth Required** | No |

### Request Body

```json
{
  "email": "developer@example.com",
  "password": "securepassword123",
  "name": "John Doe"
}
```

| Field | Type | Required | Validation | Description |
|-------|------|----------|------------|-------------|
| `email` | string | **Yes** | Valid email format | User's email address |
| `password` | string | **Yes** | Min 8 characters | Account password |
| `name` | string | No | 2-255 characters | Display name (optional) |

### Response (201 Created)

```json
{
  "email": "developer@example.com",
  "message": "Registration successful. Please check your email to verify your account."
}
```

### Error Responses

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Invalid input (validation failed) | Show field-level errors |
| 409 | Email already registered | Show "Account exists" message with login link |
| 500 | Server error | Show generic error, retry option |

### UI Implementation

**Page:** Registration Form

**Elements:**
- Email input field with validation indicator
- Password input with strength meter (min 8 chars)
- Name input field (show "optional" label)
- "Create Account" button (disabled until email + password valid)
- Success: Redirect to "Check your email" page

**UX Notes:**
- Show real-time validation as user types
- Password: Show/hide toggle button
- After success, show prominent "Check your inbox" message

---

## 1.2 User Login

Authenticates user and returns access tokens.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/login` |
| **Auth Required** | No |

### Request Body

```json
{
  "email": "developer@example.com",
  "password": "securepassword123"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `email` | string | **Yes** | Registered email |
| `password` | string | **Yes** | Account password |

### Response (200 OK)

```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIs...",
  "refresh_token": "dGhpcyBpcyBhIHJlZnJl...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "user": {
    "email": "developer@example.com",
    "name": "John Doe",
    "email_verified": true,
    "is_admin": false
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `access_token` | string | JWT for API authentication (use in Authorization header) |
| `refresh_token` | string | Token to get new access_token when expired |
| `token_type` | string | Always "Bearer" |
| `expires_in` | integer | Seconds until access_token expires |
| `user.email` | string | User's email |
| `user.name` | string/null | User's display name |
| `user.email_verified` | boolean | Whether email is verified |
| `user.is_admin` | boolean | Admin privileges flag |

### Error Responses

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Invalid request format | Show validation errors |
| 401 | Wrong email/password | Show "Invalid credentials" message |
| 401 | Email not verified | Show "Please verify your email" with resend link |
| 429 | Too many attempts | Show "Too many attempts. Try again in X minutes" |
| 500 | Server error | Show generic error |

### UI Implementation

**Page:** Login Form

**Elements:**
- Email input
- Password input (with show/hide toggle)
- "Login" button
- "Forgot Password?" link
- "Create Account" link
- "Continue with Google" button (social login alternative)

**Token Storage:**
- Store `access_token` in memory or sessionStorage (NOT localStorage for security)
- Store `refresh_token` in httpOnly cookie if possible, or secure storage
- Track `expires_in` to proactively refresh before expiration

**UX Notes:**
- Show loading spinner during authentication
- If `email_verified: false`, show banner with "Resend verification email" button
- Rate limit message should show countdown timer

---

## 1.3 Refresh Access Token

Gets new access token using refresh token.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/refresh-token` |
| **Auth Required** | No |

### Request Body

```json
{
  "refresh_token": "dGhpcyBpcyBhIHJlZnJl..."
}
```

### Response (200 OK)

```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIs...",
  "refresh_token": "bmV3IHJlZnJlc2ggdG9r...",
  "token_type": "Bearer",
  "expires_in": 3600
}
```

### UI Implementation

**Automatic Handling (No UI):**
- Call this endpoint when access_token is about to expire (e.g., 5 minutes before)
- Use axios/fetch interceptor to handle 401 responses
- If refresh fails, redirect to login page

---

## 1.4 Verify Email (GET)

Verifies email from link in verification email.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/auth/verify-email` |
| **Auth Required** | No |

### Query Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `token` | string | **Yes** | Verification token from email link |

### Example Request

```
GET /dev/auth/verify-email?token=abc123xyz789
```

### Response (200 OK)

```json
{
  "status": "success",
  "message": "Email verified successfully"
}
```

### Error Responses

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Token missing or invalid | Show "Invalid link" message |
| 404 | Token expired or not found | Show "Link expired" with resend option |

### UI Implementation

**Page:** Email Verification Result

**Elements:**
- Success state: Green checkmark, "Email Verified!" heading, "Go to Login" button
- Error state: Red X icon, error message, "Resend Verification Email" button

---

## 1.5 Verify Email (POST)

Alternative method to verify email via API call.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/verify-email` |
| **Auth Required** | No |

### Request Body

```json
{
  "token": "abc123xyz789"
}
```

### Response (200 OK)

```json
{
  "message": "Email verified successfully",
  "email": "developer@example.com"
}
```

---

## 1.6 Resend Verification Email

Sends a new verification email.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/resend-verification-email` |
| **Auth Required** | No |

### Request Body

```json
{
  "email": "developer@example.com"
}
```

### Response (200 OK)

```json
{
  "message": "Verification email sent",
  "email": "developer@example.com"
}
```

### Error Responses

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Invalid email format | Show validation error |
| 404 | Email not found | Show "No account found" message |
| 429 | Rate limited | Show "Please wait X minutes before requesting again" |

### UI Implementation

**Component:** Resend Verification Banner/Button

**UX Notes:**
- Disable button for 60 seconds after successful send
- Show "Email sent!" confirmation message
- Include spam folder reminder in UI

---

## 1.7 Request Password Reset

Sends password reset email.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/request-password-reset` |
| **Auth Required** | No |

### Request Body

```json
{
  "email": "developer@example.com"
}
```

### Response (200 OK)

```json
{
  "message": "If an account exists with this email, a password reset link has been sent."
}
```

**Security Note:** Response is intentionally vague to prevent email enumeration.

### UI Implementation

**Page:** Forgot Password Form

**Elements:**
- Email input field
- "Send Reset Link" button
- "Back to Login" link

**After Submit:**
- Always show success message (even if email doesn't exist - security)
- "Check your email for reset instructions"
- "Didn't receive it? Check spam or try again"

---

## 1.8 Reset Password

Sets new password using reset token.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/reset-password` |
| **Auth Required** | No |

### Request Body

```json
{
  "token": "reset_token_from_email",
  "new_password": "newSecurePassword123"
}
```

| Field | Type | Required | Validation |
|-------|------|----------|------------|
| `token` | string | **Yes** | From email link |
| `new_password` | string | **Yes** | Min 8 characters |

### Response (200 OK)

```json
{
  "message": "Password reset successful. All sessions have been revoked."
}
```

### Error Responses

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Invalid password format | Show "Password must be at least 8 characters" |
| 404 | Token invalid/expired | Show "Link expired" with "Request new link" button |

### UI Implementation

**Page:** Reset Password Form (accessed from email link)

**Elements:**
- New password input with strength indicator
- Confirm password input
- "Reset Password" button

**After Success:**
- Show "Password updated!" message
- Auto-redirect to login page after 3 seconds
- Note: User will need to log in again (all sessions revoked)

---

## 1.9 Logout

Logs out user and revokes all tokens.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/auth/logout` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "message": "Logged out successfully"
}
```

### UI Implementation

**Action:** Logout Button (in profile dropdown)

**On Logout:**
1. Call this endpoint
2. Clear all stored tokens
3. Clear any cached user data
4. Redirect to login page

---

## 1.10 Get Current User

Returns authenticated user's profile.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/auth/me` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "email": "developer@example.com",
  "name": "John Doe",
  "avatar_url": "https://storage.example.com/avatars/123.jpg",
  "email_verified": true,
  "is_active": true,
  "created_at": "2024-01-15T10:30:00Z",
  "updated_at": "2024-06-20T14:45:00Z",
  "last_login_at": "2024-12-20T08:00:00Z"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `email` | string | User's email address |
| `name` | string/null | Display name (can be null) |
| `avatar_url` | string/null | Profile picture URL (can be null) |
| `email_verified` | boolean | Email verification status |
| `is_active` | boolean | Account active status |
| `created_at` | ISO 8601 | Account creation timestamp |
| `updated_at` | ISO 8601 | Last profile update |
| `last_login_at` | ISO 8601/null | Last login timestamp |

### UI Implementation

**Page:** Profile Page / User Menu

**Elements:**
- Avatar (show initials if `avatar_url` is null)
- Name (show email if `name` is null)
- Email with verified badge (green checkmark if `email_verified: true`)
- "Member since" date from `created_at`
- "Last login" from `last_login_at`

---

# 2. Google OAuth Endpoints

Enables "Sign in with Google" functionality.

---

## 2.1 Initiate Google OAuth (Redirect)

Redirects user to Google's consent screen.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/auth/google` |
| **Auth Required** | No |

### Query Parameters (Optional)

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `redirect_after` | string | No | URL to redirect to after successful auth |

### Example

```
GET /auth/google?redirect_after=https://dashboard.vaultless.io/applications
```

### Response

- **Browser:** HTTP 302 redirect to Google consent screen
- **API Client:** Returns JSON (based on Accept header)

### UI Implementation

**Component:** "Continue with Google" Button

**On Click:**
- Open `/auth/google` in same window (full redirect)
- OR open in popup window for better UX
- Pass `redirect_after` to return user to intended page

---

## 2.2 Get Google OAuth URL (JSON)

Returns Google OAuth URL without redirecting. Useful for SPAs.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/auth/google/url` |
| **Auth Required** | No |

### Query Parameters (Optional)

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `redirect_after` | string | No | Post-auth redirect URL |

### Response (200 OK)

```json
{
  "auth_url": "https://accounts.google.com/o/oauth2/v2/auth?client_id=...",
  "state": "csrf_state_token_xyz"
}
```

### UI Implementation

**SPA Usage:**
1. Call this endpoint to get `auth_url`
2. Store `state` in sessionStorage for CSRF validation
3. Redirect user to `auth_url` or open popup

---

## 2.3 Google OAuth Callback

Handles Google's response after user authorizes.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/auth/google/callback` |
| **Auth Required** | No |

### Query Parameters (from Google)

| Parameter | Type | Description |
|-----------|------|-------------|
| `code` | string | Authorization code from Google |
| `state` | string | CSRF state token |
| `error` | string | Error code (if auth failed) |
| `error_description` | string | Error details |

### Response (200 OK)

```json
{
  "access_token": "eyJhbGciOiJIUzI1NiIs...",
  "refresh_token": "dGhpcyBpcyBhIHJlZnJl...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "user": {
    "email": "developer@gmail.com",
    "name": "John Doe",
    "email_verified": true,
    "is_admin": false
  },
  "is_new_user": true,
  "redirect_after": "https://dashboard.vaultless.io/applications"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `is_new_user` | boolean | `true` if new account was created |
| `redirect_after` | string/null | Original redirect URL from init step |

### UI Implementation

**Page:** OAuth Callback Handler

**Flow:**
1. Parse query params from Google
2. If `error` present, show error page
3. Extract and store tokens
4. If `is_new_user: true`, optionally show welcome modal
5. Redirect to `redirect_after` or dashboard

---

# 3. Application Management

Base path: `/dev/applications`

---

## 3.1 Create Application

Creates a new application with API keys.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/applications` |
| **Auth Required** | **Yes** |

### Request Body

```json
{
  "name": "My Payment App",
  "description": "Production payment processing application"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | **Yes** | Application name |
| `description` | string | No | Application description |

### Response (201 Created)

```json
{
  "application": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "My Payment App",
    "description": "Production payment processing application",
    "is_active": true,
    "created_at": "2024-12-20T10:00:00Z",
    "updated_at": "2024-12-20T10:00:00Z",
    "max_ttl_seconds": 86400,
    "is_key_rotation_forced": false,
    "deletion_requested_at": null,
    "internal_notes": null,
    "integrity_config": {}
  },
  "secret_key": "sk_live_abc123xyz789...",
  "publishable_key": "pk_live_def456uvw...",
  "message": "IMPORTANT: Save your secret key now. It will not be shown again!"
}
```

### UI Implementation

**Page:** Create Application Modal/Form

**Elements:**
- Name input (required)
- Description textarea (optional)
- "Create Application" button

**After Success - CRITICAL:**
- Show modal with **secret key prominently displayed**
- Warning icon + message: "Copy and save your secret key now. It will NEVER be shown again!"
- "Copy to Clipboard" button
- Checkbox: "I have saved my secret key"
- "Continue" button (disabled until checkbox checked)

**Visual Design for Secret Key:**
```
┌────────────────────────────────────────────────────────────┐
│  ⚠️  SAVE YOUR SECRET KEY                                  │
│                                                            │
│  This is your only chance to copy this key.               │
│  Store it securely - it cannot be retrieved later.        │
│                                                            │
│  ┌──────────────────────────────────────────────────────┐ │
│  │ sk_live_abc123xyz789...                         [📋] │ │
│  └──────────────────────────────────────────────────────┘ │
│                                                            │
│  ☐ I have saved my secret key                             │
│                                                            │
│                                      [Continue →]          │
└────────────────────────────────────────────────────────────┘
```

---

## 3.2 List Applications

Returns paginated list of user's applications.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications` |
| **Auth Required** | **Yes** |
| **Caching** | ETag supported |

### Query Parameters

| Parameter | Type | Required | Default | Max | Description |
|-----------|------|----------|---------|-----|-------------|
| `page` | integer | No | 1 | - | Page number |
| `page_size` | integer | No | 20 | 200 | Items per page |

### Example Request

```
GET /dev/applications?page=1&page_size=10
```

### Response (200 OK)

```json
{
  "data": [
    {
      "application_id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "My Payment App",
      "description": "Production payment processing",
      "is_active": true,
      "created_at": "2024-12-20T10:00:00Z",
      "updated_at": "2024-12-20T10:00:00Z",
      "tier": "pro",
      "monthly_message_quota": 100000,
      "publishable_key_count": 2,
      "webhook_count": 1,
      "quota_usage_percentage": 45.5
    }
  ],
  "total_count": 3,
  "page": 1,
  "page_size": 10,
  "total_pages": 1
}
```

| Field | Type | Description |
|-------|------|-------------|
| `data[].application_id` | UUID | Unique application identifier |
| `data[].name` | string | Application name |
| `data[].description` | string/null | Application description |
| `data[].is_active` | boolean | Active status |
| `data[].tier` | string | Subscription tier (free/pro/enterprise) |
| `data[].monthly_message_quota` | integer | Monthly message limit |
| `data[].publishable_key_count` | integer | Number of publishable keys |
| `data[].webhook_count` | integer | Number of configured webhooks |
| `data[].quota_usage_percentage` | float | Current month usage % (0-100+) |
| `total_count` | integer | Total applications across all pages |
| `page` | integer | Current page number |
| `page_size` | integer | Items per page |
| `total_pages` | integer | Total number of pages |

### Response Headers

```
ETag: "abc123"
Cache-Control: private, max-age=60
```

### UI Implementation

**Page:** Applications List / Dashboard

**Elements:**
- "Create Application" button (top right)
- Application cards/table showing:
  - App name
  - Status badge (green "Active" / red "Inactive")
  - Tier badge (color-coded: Free=gray, Pro=blue, Enterprise=purple)
  - Usage progress bar (`quota_usage_percentage`)
  - Created date
  - View/Edit/Delete actions
- Pagination controls at bottom

**Visual Design for Application Card:**
```
┌─────────────────────────────────────────────────────────────┐
│  My Payment App                    [Active] [Pro]           │
│  Production payment processing                              │
│                                                             │
│  Usage: ████████████░░░░░░ 45.5% (45,500 / 100,000)       │
│                                                             │
│  Created: Dec 20, 2024  │  Keys: 2  │  Webhooks: 1         │
│                                                             │
│                              [View] [Edit] [Delete]         │
└─────────────────────────────────────────────────────────────┘
```

**Quota Warning Colors:**
- 0-70%: Green progress bar
- 70-90%: Orange progress bar
- 90%+: Red progress bar with warning icon

---

## 3.3 Get Application with Keys

Returns application details including API keys.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/{application_id}/with_keys` |
| **Auth Required** | **Yes** |
| **Caching** | ETag supported |

### Path Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `application_id` | UUID | **Yes** | Application ID |

### Query Parameters (Optional)

| Parameter | Type | Description |
|-----------|------|-------------|
| `If-None-Match` | string | ETag for conditional request (returns 304 if unchanged) |

### Response (200 OK)

```json
{
  "application": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "name": "My Payment App",
    "description": "Production payment processing",
    "is_active": true,
    "created_at": "2024-12-20T10:00:00Z",
    "updated_at": "2024-12-20T10:00:00Z",
    "max_ttl_seconds": 86400,
    "is_key_rotation_forced": false,
    "deletion_requested_at": null,
    "internal_notes": null,
    "integrity_config": {}
  },
  "publishable_keys": [
    {
      "id": "key_123",
      "key": "pk_live_abc123...",
      "name": "Production Key",
      "is_active": true,
      "created_at": "2024-12-20T10:00:00Z"
    }
  ],
  "webhooks": [
    {
      "id": "wh_456",
      "url": "https://myapp.com/webhooks/vaultless",
      "is_active": true,
      "events": ["message.created", "message.delivered"]
    }
  ]
}
```

### UI Implementation

**Page:** Application Details

**Sections:**

1. **Header:**
   - Application name (editable)
   - Status toggle
   - Edit/Delete buttons

2. **API Keys Section:**
   ```
   ┌─────────────────────────────────────────────────────────┐
   │  🔑 API Keys                                            │
   │                                                         │
   │  Publishable Key (safe to use in frontend)             │
   │  ┌─────────────────────────────────────────────────┐   │
   │  │ pk_live_abc123xyz789...                    [📋] │   │
   │  └─────────────────────────────────────────────────┘   │
   │                                                         │
   │  Secret Key (keep secure - server-side only)           │
   │  ┌─────────────────────────────────────────────────┐   │
   │  │ ••••••••••••••••••••••••••              [👁️] [🔄] │   │
   │  └─────────────────────────────────────────────────┘   │
   │  ⚠️ Secret key cannot be revealed. Generate new key   │
   │     if you've lost it. This will invalidate the old    │
   │     key.                                                │
   └─────────────────────────────────────────────────────────┘
   ```

3. **Webhooks Section:** List configured webhooks with status

---

## 3.4 Update Application

Updates application details including webhook configuration.

| Property | Value |
|----------|-------|
| **Method** | `PATCH` |
| **Path** | `/api/applications/{app_id}` |
| **Auth Required** | **Yes** |

### Path Parameters

| Parameter | Type | Required |
|-----------|------|----------|
| `app_id` | UUID | **Yes** |

### Request Body

```json
{
  "name": "Updated App Name",
  "description": "Updated description",
  "is_active": true,
  "max_ttl_seconds": 3600,
  "is_key_rotation_forced": false,
  "internal_notes": "Internal notes for team reference",
  "webhooks": [
    {
      "id": null,
      "url": "https://example.com/webhooks/auth",
      "event_type": "client.signup",
      "is_active": true
    },
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "url": "https://example.com/webhooks/auth",
      "event_type": "client.signin",
      "is_active": true
    }
  ],
  "integrity_config": {
    "browser": {
      "authorized_origins": ["https://example.com"]
    }
  }
}
```

### Request Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | No | Application name (1-255 chars) |
| `description` | string | No | Application description (max 1000 chars) |
| `is_active` | boolean | No | Enable/disable the application |
| `max_ttl_seconds` | integer | No | Maximum TTL for messages |
| `is_key_rotation_forced` | boolean | No | Force key rotation policy |
| `internal_notes` | string | No | Internal notes (max 1000 chars) |
| `webhooks` | array | No | Webhook configuration (see below) |
| `integrity_config` | object | No | Platform integrity settings |

All fields are optional - only include fields to update.

### Webhook Management

Webhooks are managed through the `webhooks` field in the update request. This provides a **declarative** approach where you specify the desired final state.

#### Webhook Input Object

```json
{
  "id": "uuid or null",
  "url": "https://example.com/webhook",
  "event_type": "message.created",
  "is_active": true
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | UUID/null | No | Webhook ID. `null` = create new, UUID = update existing |
| `url` | string | **Yes** | Webhook endpoint URL (must be HTTPS in production, max 2048 chars) |
| `event_type` | string | **Yes** | Event type that triggers this webhook (1-100 chars) |
| `is_active` | boolean | No | Whether webhook is active (default: `true`) |

#### Available Event Types

**Client Events** - Authentication and lifecycle:

| Event Type | Description |
|------------|-------------|
| `client.signup` | Triggered when a new client registers/signs up under an application |
| `client.signin` | Triggered when an existing client signs in/authenticates |
| `client.revoked` | Triggered when a client is deactivated or revoked |
| `client.attestation_changed` | Triggered when a client's platform attestation status changes |

**Security Events** - Important security-related notifications:

| Event Type | Description |
|------------|-------------|
| `security.rate_limited` | Triggered when rate limiting is applied to a client |
| `security.suspicious_activity` | Triggered when suspicious activity is detected |

**Quota Events** - Application-level quota notifications:

| Event Type | Description |
|------------|-------------|
| `quota.warning` | Triggered when quota usage reaches warning threshold (e.g., 80%) |
| `quota.exceeded` | Triggered when quota is exceeded |

> **Note:** Message events (created, delivered, read, expired) are not available as webhook triggers because they are high-frequency hotpath operations where webhook overhead would add unacceptable latency.

#### Webhook Behavior

| Scenario | Action |
|----------|--------|
| `webhooks` field **omitted** | No changes to existing webhooks |
| `webhooks: []` (empty array) | **Delete all** existing webhooks |
| Webhook with `id: null` | **Create** new webhook |
| Webhook with `id: <uuid>` | **Update** existing webhook |
| Existing webhook not in list | **Delete** that webhook |

#### Limits & Validation

- **Maximum 5 webhooks** per application
- Each `(url, event_type)` combination must be unique within the application
- URL must be valid and preferably HTTPS
- Duplicate `(url, event_type)` pairs in request will fail validation

### Example: Add a Webhook for Client Signups

```json
{
  "webhooks": [
    {
      "id": null,
      "url": "https://myapp.com/webhooks/vaultless",
      "event_type": "client.signup",
      "is_active": true
    }
  ]
}
```

### Example: Update an Existing Webhook

```json
{
  "webhooks": [
    {
      "id": "existing-webhook-uuid",
      "url": "https://myapp.com/webhooks/updated-endpoint",
      "event_type": "client.signin",
      "is_active": false
    }
  ]
}
```

### Example: Delete All Webhooks

```json
{
  "webhooks": []
}
```

### Example: Multiple Webhooks for Different Events

```json
{
  "webhooks": [
    {
      "id": null,
      "url": "https://myapp.com/webhooks/auth",
      "event_type": "client.signup",
      "is_active": true
    },
    {
      "id": null,
      "url": "https://myapp.com/webhooks/auth",
      "event_type": "client.signin",
      "is_active": true
    },
    {
      "id": null,
      "url": "https://myapp.com/webhooks/security",
      "event_type": "security.rate_limited",
      "is_active": true
    },
    {
      "id": null,
      "url": "https://myapp.com/webhooks/quota",
      "event_type": "quota.warning",
      "is_active": true
    }
  ]
}
```

### Error Responses

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Validation failed (max webhooks exceeded, invalid URL, duplicate) | Show specific error |
| 401 | Unauthorized | Redirect to login |
| 403 | Forbidden (not owner) | Show "Access Denied" |
| 404 | Application or webhook not found | Show "Not Found" |
| 500 | Server error | Show generic error |

### Response (200 OK)

Returns updated `ApplicationResponse` object.

### UI Implementation

**Page:** Application Settings / Webhook Management

**Webhook List Section:**
```
┌─────────────────────────────────────────────────────────────┐
│  🔗 Webhooks (2/5)                          [+ Add Webhook] │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ 🟢 message.created                                   │   │
│  │ https://myapp.com/webhooks/created                  │   │
│  │                                    [Edit] [Delete]   │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ 🔴 message.delivered (Disabled)                     │   │
│  │ https://myapp.com/webhooks/delivered                │   │
│  │                                    [Edit] [Delete]   │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Add/Edit Webhook Modal:**
```
┌─────────────────────────────────────────────────────────┐
│  Add Webhook                                        [x] │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  Endpoint URL *                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ https://                                         │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  Event Type *                                           │
│  ┌─────────────────────────────────────────────────┐   │
│  │ message.created                              ▼   │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  ☑ Active                                               │
│                                                         │
│  ⚠️ Maximum 5 webhooks per application                 │
│                                                         │
│                              [Cancel]  [Save Webhook]   │
└─────────────────────────────────────────────────────────┘
```

**UX Notes:**
- Show webhook count (e.g., "2/5") to indicate limit
- Disable "Add Webhook" button when at max (5)
- Use color indicators: 🟢 Active, 🔴 Disabled
- Confirm before deleting webhooks
- Show validation errors inline

---

## 3.5 Deactivate Application

Soft-deletes an application.

| Property | Value |
|----------|-------|
| **Method** | `DELETE` |
| **Path** | `/api/applications/{app_id}` |
| **Auth Required** | **Yes** |

### Response (204 No Content)

No response body.

### UI Implementation

**Component:** Delete Confirmation Modal

**Design:**
```
┌─────────────────────────────────────────────────────────┐
│  ⚠️ Deactivate Application?                             │
│                                                         │
│  This will:                                             │
│  • Disable all API keys                                 │
│  • Stop all webhook deliveries                          │
│  • Data will be retained for 30 days                    │
│                                                         │
│  Type the application name to confirm:                  │
│  ┌─────────────────────────────────────────────────┐   │
│  │                                                  │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│                    [Cancel]  [Deactivate]               │
└─────────────────────────────────────────────────────────┘
```

---

# 4. Analytics & Usage

---

## 4.1 Get Quota Status

Returns current quota usage for an application.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/{application_id}/quota-status` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "application_id": "550e8400-e29b-41d4-a716-446655440000",
  "messages_used": 45500,
  "messages_limit": 100000,
  "usage_percentage": 45.5,
  "is_over_quota": false,
  "overage_count": 0,
  "resets_at": "2025-01-01T00:00:00Z",
  "alert_level": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `messages_used` | integer | Messages used this billing period |
| `messages_limit` | integer | Total allowed messages |
| `usage_percentage` | float | Used/Limit * 100 |
| `is_over_quota` | boolean | True if over limit |
| `overage_count` | integer | Messages over quota |
| `resets_at` | ISO 8601 | When quota resets |
| `alert_level` | string/null | `"info"` (70%), `"warning"` (90%), `"critical"` (100%+), or `null` |

### UI Implementation

**Component:** Quota Usage Card

**Design:**
```
┌─────────────────────────────────────────────────────────┐
│  📊 Monthly Usage                                       │
│                                                         │
│  45,500 / 100,000 messages                             │
│  ████████████████████░░░░░░░░░░░░░░░ 45.5%            │
│                                                         │
│  Resets: January 1, 2025 (12 days)                     │
│                                                         │
│                              [View Details] [Upgrade]   │
└─────────────────────────────────────────────────────────┘
```

**Alert Level Styling:**
- `null`: Normal (no alert)
- `"info"`: Yellow banner - "Approaching quota limit"
- `"warning"`: Orange banner - "90% of quota used"
- `"critical"`: Red banner - "Quota exceeded! Messages may be blocked"

---

## 4.2 Get Cost Breakdown

Returns cost breakdown by category.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/{application_id}/costs` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "total_cost_cents": 4550,
  "breakdown": [
    {
      "category": "Messages",
      "amount_cents": 3500,
      "unit": "message",
      "quantity": 35000
    },
    {
      "category": "Bandwidth",
      "amount_cents": 500,
      "unit": "GB",
      "quantity": 50
    },
    {
      "category": "Storage",
      "amount_cents": 300,
      "unit": "GB-month",
      "quantity": 30
    },
    {
      "category": "Proofs",
      "amount_cents": 250,
      "unit": "proof",
      "quantity": 2500
    }
  ]
}
```

### UI Implementation

**Component:** Cost Breakdown Chart

**Design:**
```
┌─────────────────────────────────────────────────────────┐
│  💰 Current Month Costs: $45.50                        │
│                                                         │
│  Messages      ████████████████████  $35.00 (77%)      │
│  Bandwidth     ████                   $5.00 (11%)      │
│  Storage       ███                    $3.00  (7%)      │
│  Proofs        ██                     $2.50  (5%)      │
│                                                         │
│                              [Export Report]            │
└─────────────────────────────────────────────────────────┘
```

---

## 4.3 Get Application Trends

Returns usage trends and projections.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/{application_id}/trends` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "daily_average_messages": 1500,
  "weekly_average_messages": 10500,
  "growth_percentage_7d": 12.5,
  "projected_monthly_cost_cents": 5200,
  "quota_trend": "increasing"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `daily_average_messages` | integer | Average messages per day |
| `weekly_average_messages` | integer | Average messages per week |
| `growth_percentage_7d` | float | Week-over-week growth % |
| `projected_monthly_cost_cents` | integer | Projected cost at current rate |
| `quota_trend` | string | `"increasing"` or `"stable"` |

### UI Implementation

**Component:** Trends Summary

**Design:**
```
┌─────────────────────────────────────────────────────────┐
│  📈 Usage Trends                                        │
│                                                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐    │
│  │    1,500    │  │   +12.5%    │  │    $52      │    │
│  │  msgs/day   │  │   growth    │  │  projected  │    │
│  └─────────────┘  └─────────────┘  └─────────────┘    │
│                                                         │
│  ⚠️ At current rate, you'll hit quota in 15 days      │
└─────────────────────────────────────────────────────────┘
```

---

## 4.4 Get Chart Data

Returns time-series data for charts.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/api/applications/{app_id}/chart` |
| **Auth Required** | **Yes** |

### Query Parameters

| Parameter | Type | Required | Options | Description |
|-----------|------|----------|---------|-------------|
| `granularity` | string | **Yes** | `daily`, `weekly` | Data bucketing |
| `metric` | string | **Yes** | `messages`, `bandwidth`, `storage`, `proofs`, `rate_limits`, `cost`, `all` | Which metric(s) |
| `start` | string | **Yes** | YYYY-MM-DD | Start date |
| `end` | string | **Yes** | YYYY-MM-DD | End date |

### Validation Rules

- Max 100 daily buckets
- Max 160 weekly buckets
- `start` must be before `end`

### Example Request

```
GET /api/applications/123/chart?granularity=daily&metric=messages&start=2024-12-01&end=2024-12-20
```

### Response (200 OK)

```json
{
  "data": [
    {
      "date": "2024-12-01",
      "messages": 1200
    },
    {
      "date": "2024-12-02",
      "messages": 1450
    }
  ],
  "metadata": {
    "granularity": "daily",
    "metric": "messages",
    "start": "2024-12-01",
    "end": "2024-12-20"
  }
}
```

### UI Implementation

**Component:** Analytics Chart

**Elements:**
- Date range picker (preset: 7d, 30d, 90d, custom)
- Granularity toggle (Daily / Weekly)
- Metric selector dropdown
- Line chart visualization
- Hover tooltips with exact values

---

## 4.5 Export Application Usage

Downloads usage data as JSON or CSV.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/{application_id}/export` |
| **Auth Required** | **Yes** |

### Query Parameters

| Parameter | Type | Required | Options |
|-----------|------|----------|---------|
| `format` | string | **Yes** | `json`, `csv` |

### Response

- **JSON format:** Application JSON object
- **CSV format:** File download with `Content-Type: text/csv`

### UI Implementation

**Component:** Export Button/Dropdown

**Design:**
```
[Export ▼]
  ├─ Download as JSON
  └─ Download as CSV
```

---

## 4.6 Get Quota Warnings

Returns all applications approaching/exceeding quota.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/quota-warnings` |
| **Auth Required** | **Yes** |
| **Caching** | ETag supported |

### Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `threshold` | float | No | 80.0 | Minimum usage % to include |
| `page` | integer | No | 1 | Page number |
| `page_size` | integer | No | 20 | Items per page |

### Response (200 OK)

```json
{
  "data": [
    {
      "application_id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "My Payment App",
      "usage_percentage": 92.5,
      "messages_used": 92500,
      "messages_limit": 100000
    }
  ],
  "page": 1,
  "page_size": 20,
  "total_count": 1,
  "total_pages": 1
}
```

### UI Implementation

**Component:** Dashboard Warning Banner

**Design:**
```
┌─────────────────────────────────────────────────────────┐
│  ⚠️ 1 application approaching quota limit              │
│                                                         │
│  My Payment App: 92.5% used (92,500 / 100,000)         │
│                                                         │
│                              [View] [Upgrade Plan]      │
└─────────────────────────────────────────────────────────┘
```

---

## 4.7 Get Usage Summary

Returns aggregated usage across all applications.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/usage-summary` |
| **Auth Required** | **Yes** |
| **Caching** | ETag supported |

### Query Parameters (Optional)

| Parameter | Type | Description |
|-----------|------|-------------|
| `If-None-Match` | string | ETag for conditional request |

### Response (200 OK)

```json
{
  "total_applications": 5,
  "active_applications": 4,
  "total_messages_this_month": 150000,
  "total_cost_this_month_cents": 15000,
  "applications_near_quota": 1
}
```

### UI Implementation

**Component:** Dashboard Summary Cards

**Design:**
```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│      5       │  │   150,000    │  │    $150      │  │      1       │
│   Total      │  │   Messages   │  │    Cost      │  │   ⚠️ Near    │
│   Apps       │  │  This Month  │  │  This Month  │  │   Quota      │
└──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘
```

---

## 4.8 Get Application Analytics (Full Dashboard)

Returns comprehensive analytics for a single application.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/applications/{application_id}/analytics` |
| **Auth Required** | **Yes** |
| **Caching** | ETag supported |

### Response (200 OK)

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "My Payment App",
  "desc": "Production payment processing",
  "active": true,
  "created": "2024-12-20T10:00:00Z",
  "updated": "2024-12-20T10:00:00Z",
  "max_ttl": 86400,
  "rotation_forced": false,
  "deleted_at": null,
  "meta": {},
  "tier": "pro",
  "monthly_quota": 100000,
  "rate_limit": 1000,
  "retention_seconds": 2592000,
  "keys": [
    {
      "id": "key_123",
      "key": "pk_live_abc123...",
      "name": "Production Key",
      "is_active": true,
      "created_at": "2024-12-20T10:00:00Z"
    }
  ],
  "webhooks": [],
  "quota_usage_pct": 45.5,
  "current_month": {
    "msg_sent": 30000,
    "msg_received": 15500,
    "msg_proof": 2500,
    "msg_stored": 45500,
    "bytes_sent": 150000000,
    "bytes_received": 75000000,
    "rate_hits": 50,
    "cost": 4550
  },
  "lifetime": {
    "msg_sent": 500000,
    "msg_received": 250000,
    "msg_proof": 50000,
    "msg_stored": 750000,
    "bytes_sent": 2500000000,
    "bytes_received": 1250000000,
    "rate_hits": 1200,
    "cost": 75000
  },
  "last_7d": {
    "msg_sent": 10500,
    "bytes_sent": 52500000,
    "bytes_received": 26250000,
    "cost": 1050
  },
  "last_30d": {
    "msg_sent": 45000,
    "bytes_sent": 225000000,
    "bytes_received": 112500000,
    "cost": 4500
  }
}
```

### UI Implementation

**Page:** Application Analytics Dashboard

This endpoint provides all data needed for a comprehensive analytics page with:

1. **Header:** App name, status, tier badge
2. **Quick Stats:** Current month usage cards
3. **Time Period Tabs:** This Month | Last 7 Days | Last 30 Days | Lifetime
4. **Detailed Metrics:**
   - Messages sent/received
   - Bandwidth usage
   - Proof generations
   - Rate limit hits
   - Cost

---

# 5. Notifications

Base path: `/dev/notifications`

---

## 5.1 List Notifications

Returns paginated notifications with filtering.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/notifications` |
| **Auth Required** | **Yes** |

### Query Parameters

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `is_read` | boolean | No | - | Filter by read status |
| `notification_type` | string | No | - | Filter by type |
| `severity` | string | No | - | Filter by severity |
| `page` | integer | No | 1 | Page number |
| `page_size` | integer | No | 20 | Items per page (max 100) |

### Example Request

```
GET /dev/notifications?is_read=false&page=1&page_size=10
```

### Response (200 OK)

```json
{
  "data": [
    {
      "id": "notif_123",
      "user_id": "user_456",
      "title": "Quota Warning",
      "message": "My Payment App has used 90% of its monthly quota",
      "notification_type": "quota_warning",
      "severity": "warning",
      "action_url": "/applications/550e8400-e29b-41d4-a716-446655440000",
      "metadata": {
        "application_id": "550e8400-e29b-41d4-a716-446655440000",
        "usage_percentage": 90
      },
      "is_read": false,
      "read_at": null,
      "created_at": "2024-12-20T08:00:00Z",
      "updated_at": "2024-12-20T08:00:00Z",
      "expires_at": "2025-01-20T08:00:00Z"
    }
  ],
  "total_count": 15,
  "page": 1,
  "page_size": 10,
  "total_pages": 2,
  "unread_count": 5
}
```

| Field | Type | Description |
|-------|------|-------------|
| `notification_type` | string | Type identifier for filtering/styling |
| `severity` | string | `info`, `warning`, `error`, `success` |
| `action_url` | string/null | Link to relevant page |
| `metadata` | object/null | Additional context data |
| `is_read` | boolean | Read status |
| `read_at` | ISO 8601/null | When marked as read |
| `expires_at` | ISO 8601/null | Auto-delete date |
| `unread_count` | integer | Total unread across all pages |

### UI Implementation

**Page:** Notifications List

**Elements:**
- Filter tabs: All | Unread
- Filter dropdown: All Types | Quota | Security | System | Billing
- "Mark All Read" button
- Notification list items
- Pagination

**Notification Item Design:**
```
┌─────────────────────────────────────────────────────────────┐
│  🟡 ● Quota Warning                           2 hours ago   │
│                                                             │
│  My Payment App has used 90% of its monthly quota          │
│                                                             │
│  [View Application]                          [Mark as Read] │
└─────────────────────────────────────────────────────────────┘
```

**Severity Colors:**
- `info`: Blue
- `warning`: Orange
- `error`: Red
- `success`: Green

**Notification Types & Icons:**
| Type | Icon | Color |
|------|------|-------|
| quota_warning | 📊 | Orange |
| security_alert | 🔒 | Red |
| system_update | ⚙️ | Blue |
| billing | 💳 | Purple |
| feature | ✨ | Green |

---

## 5.2 Get Specific Notification

Returns single notification details.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/notifications/{notification_id}` |
| **Auth Required** | **Yes** |

### Path Parameters

| Parameter | Type | Required |
|-----------|------|----------|
| `notification_id` | UUID | **Yes** |

### Response (200 OK)

Full notification object (same structure as list item).

---

## 5.3 Get Unread Count

Returns count of unread notifications.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/notifications/unread-count` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "unread_count": 5
}
```

### UI Implementation

**Component:** Notification Bell Icon

**Design:**
```
    🔔
   (5)
```

Poll this endpoint periodically (e.g., every 60 seconds) or use WebSocket for real-time updates.

---

## 5.4 Get Notification Summary

Returns notification counts by type and severity.

| Property | Value |
|----------|-------|
| **Method** | `GET` |
| **Path** | `/dev/notifications/summary` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
[
  {
    "notification_type": "quota_warning",
    "severity": "warning",
    "total_count": 3,
    "unread_count": 2,
    "latest_notification": "2024-12-20T08:00:00Z"
  },
  {
    "notification_type": "security_alert",
    "severity": "error",
    "total_count": 1,
    "unread_count": 1,
    "latest_notification": "2024-12-19T15:30:00Z"
  }
]
```

### UI Implementation

**Component:** Notification Summary Dropdown

**Design:**
```
┌───────────────────────────────────────┐
│  Notifications                    [x] │
├───────────────────────────────────────┤
│  🟠 Quota Warnings         3 (2 new)  │
│  🔴 Security Alerts        1 (1 new)  │
│  🔵 System Updates         5          │
├───────────────────────────────────────┤
│                     [View All →]      │
└───────────────────────────────────────┘
```

---

## 5.5 Mark Notification as Read

Marks a single notification as read.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/notifications/{notification_id}/read` |
| **Auth Required** | **Yes** |

### Response (200 OK)

Returns updated notification object with `is_read: true`.

---

## 5.6 Mark All Notifications as Read

Marks all notifications as read.

| Property | Value |
|----------|-------|
| **Method** | `POST` |
| **Path** | `/dev/notifications/read-all` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "success": true,
  "count": 5,
  "message": "5 notifications marked as read"
}
```

---

## 5.7 Delete Notification

Deletes a single notification.

| Property | Value |
|----------|-------|
| **Method** | `DELETE` |
| **Path** | `/dev/notifications/{notification_id}` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "success": true,
  "message": "Notification deleted successfully"
}
```

---

## 5.8 Delete All Read Notifications

Deletes all read notifications.

| Property | Value |
|----------|-------|
| **Method** | `DELETE` |
| **Path** | `/dev/notifications/read` |
| **Auth Required** | **Yes** |

### Response (200 OK)

```json
{
  "success": true,
  "count": 10,
  "message": "10 read notifications deleted"
}
```

### UI Implementation

**Component:** "Clear Read" Button

Show this option in notification settings or as secondary action.

---

# 6. Common Response Patterns

---

## Pagination Response

All paginated endpoints return:

```json
{
  "data": [...],
  "total_count": 100,
  "page": 1,
  "page_size": 20,
  "total_pages": 5
}
```

### UI Implementation

**Component:** Pagination Controls

```
[< Prev]  Page 1 of 5  [Next >]

Or:

[1] [2] [3] ... [5]
```

---

## Error Response Format

All errors follow this format:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Human readable error message",
    "details": {
      "field": "email",
      "reason": "Invalid email format"
    }
  }
}
```

### Common HTTP Status Codes

| Status | Meaning | UI Action |
|--------|---------|-----------|
| 400 | Bad Request | Show validation errors |
| 401 | Unauthorized | Redirect to login |
| 403 | Forbidden | Show "Access Denied" |
| 404 | Not Found | Show "Not Found" page |
| 409 | Conflict | Show specific conflict message |
| 429 | Too Many Requests | Show rate limit message with retry time |
| 500 | Server Error | Show generic error with retry option |

---

## Caching (ETag Support)

Endpoints marked with "ETag supported" return:

**Response Headers:**
```
ETag: "abc123"
Cache-Control: private, max-age=60
```

**Conditional Request:**
```
GET /dev/applications
If-None-Match: "abc123"
```

**If data unchanged:** Returns `304 Not Modified` (no body)

### UI Implementation

Store ETag values and use them for subsequent requests to reduce bandwidth and improve performance.

---

## Quick Reference: All Endpoints

| Category | Method | Path | Auth |
|----------|--------|------|------|
| **Auth** | POST | `/dev/auth/register` | No |
| | POST | `/dev/auth/login` | No |
| | POST | `/dev/auth/refresh-token` | No |
| | GET | `/dev/auth/verify-email` | No |
| | POST | `/dev/auth/verify-email` | No |
| | POST | `/dev/auth/resend-verification-email` | No |
| | POST | `/dev/auth/request-password-reset` | No |
| | POST | `/dev/auth/reset-password` | No |
| | POST | `/dev/auth/logout` | Yes |
| | GET | `/dev/auth/me` | Yes |
| **OAuth** | GET | `/auth/google` | No |
| | GET | `/auth/google/url` | No |
| | GET | `/auth/google/callback` | No |
| **Apps** | POST | `/dev/applications` | Yes |
| | GET | `/dev/applications` | Yes |
| | GET | `/dev/applications/{id}/with_keys` | Yes |
| | PATCH | `/api/applications/{id}` | Yes |
| | DELETE | `/api/applications/{id}` | Yes |
| **Analytics** | GET | `/dev/applications/{id}/quota-status` | Yes |
| | GET | `/dev/applications/{id}/costs` | Yes |
| | GET | `/dev/applications/{id}/trends` | Yes |
| | GET | `/api/applications/{id}/chart` | Yes |
| | GET | `/dev/applications/{id}/export` | Yes |
| | GET | `/dev/applications/quota-warnings` | Yes |
| | GET | `/dev/applications/usage-summary` | Yes |
| | GET | `/dev/applications/{id}/analytics` | Yes |
| **Notifications** | GET | `/dev/notifications` | Yes |
| | GET | `/dev/notifications/{id}` | Yes |
| | GET | `/dev/notifications/unread-count` | Yes |
| | GET | `/dev/notifications/summary` | Yes |
| | POST | `/dev/notifications/{id}/read` | Yes |
| | POST | `/dev/notifications/read-all` | Yes |
| | DELETE | `/dev/notifications/{id}` | Yes |
| | DELETE | `/dev/notifications/read` | Yes |
