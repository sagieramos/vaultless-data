# Vaultless Developer Portal - UI/UX Design Specification

> **Version:** 1.1
> **Last Updated:** 2025-12-31
> **Status:** Draft

---

## Table of Contents

1. [Project Overview](#1-project-overview)
2. [Target Users](#2-target-users)
3. [Authentication & User Management](#3-authentication--user-management)
4. [Application Management](#4-application-management)
5. [Information Architecture](#5-information-architecture)
6. [Design Requirements](#6-design-requirements)
7. [Deliverables](#7-deliverables)
8. [Technical Context](#8-technical-context)

---

## 1. Project Overview

Design a complete developer portal for **Vaultless** - a secure, privacy-first messaging platform. The portal includes authentication flows, application management, API key handling, usage analytics, and billing monitoring.

### Goals

- Provide a seamless developer experience for managing applications
- Ensure secure handling of sensitive API keys
- Deliver clear, actionable insights through analytics
- Maintain professional, developer-focused aesthetics

---

## 2. Target Users

| User Type | Description | Primary Goals |
|-----------|-------------|---------------|
| **Software Developers** | Integrating Vaultless into their products | Quick setup, clear documentation, easy key management |
| **Technical Product Managers** | Overseeing API usage across teams | Usage monitoring, cost tracking, quota management |
| **DevOps Engineers** | Monitoring application health | Real-time metrics, alerting, trend analysis |

### User Personas

#### Persona 1: Alex - Full-Stack Developer
- **Experience:** 5 years
- **Goals:** Quickly integrate secure messaging, manage multiple app environments
- **Pain Points:** Complex setup processes, unclear error messages, poor documentation

#### Persona 2: Jordan - Engineering Manager
- **Experience:** 8 years
- **Goals:** Monitor team's API usage, manage budgets, ensure compliance
- **Pain Points:** Lack of visibility into costs, difficulty tracking usage across apps

---

## 3. Authentication & User Management

### 3.1 Registration Page

**Endpoint:** `POST /dev/auth/register`

#### Form Fields

| Field | Type | Required | Validation |
|-------|------|----------|------------|
| Email | email | Yes | Valid email format |
| Password | password | Yes | Min 8 chars, 1 uppercase, 1 number, 1 special |
| Confirm Password | password | Yes | Must match password |
| Terms Checkbox | checkbox | Yes | Must be checked |

#### UI States

```
┌─────────────────────────────────────────┐
│           Create your account           │
├─────────────────────────────────────────┤
│                                         │
│  Email                                  │
│  ┌─────────────────────────────────┐   │
│  │ developer@example.com           │   │
│  └─────────────────────────────────┘   │
│                                         │
│  Password                               │
│  ┌─────────────────────────────────┐   │
│  │ ••••••••••••                    │   │
│  └─────────────────────────────────┘   │
│  ████████░░ Strong                      │
│                                         │
│  Confirm Password                       │
│  ┌─────────────────────────────────┐   │
│  │ ••••••••••••                    │   │
│  └─────────────────────────────────┘   │
│  ✓ Passwords match                      │
│                                         │
│  ☑ I agree to the Terms of Service     │
│                                         │
│  ┌─────────────────────────────────┐   │
│  │        Create Account           │   │
│  └─────────────────────────────────┘   │
│                                         │
│  Already have an account? Log in        │
│                                         │
└─────────────────────────────────────────┘
```

#### Success State
- Display: "Verification email sent to {email}"
- Action: Link to login page
- Note: "Check your spam folder if you don't see it"

---

### 3.2 Login Page

**Endpoint:** `POST /dev/auth/login`

#### Form Fields

| Field | Type | Required |
|-------|------|----------|
| Email | email | Yes |
| Password | password | Yes |
| Remember Me | checkbox | No |

#### UI States

```
┌─────────────────────────────────────────┐
│            Welcome back                 │
├─────────────────────────────────────────┤
│                                         │
│  Email                                  │
│  ┌─────────────────────────────────┐   │
│  │                                 │   │
│  └─────────────────────────────────┘   │
│                                         │
│  Password                               │
│  ┌─────────────────────────────────┐   │
│  │                                 │ 👁 │
│  └─────────────────────────────────┘   │
│                                         │
│  ☐ Remember me      Forgot password?   │
│                                         │
│  ┌─────────────────────────────────┐   │
│  │           Log in                │   │
│  └─────────────────────────────────┘   │
│                                         │
│  Don't have an account? Register        │
│                                         │
└─────────────────────────────────────────┘
```

#### Error States

| Error | Message | Action |
|-------|---------|--------|
| Invalid credentials | "Invalid email or password" | Clear password field |
| Email not verified | "Please verify your email first" | Show resend link |
| Account locked | "Account temporarily locked" | Show unlock timer |
| Rate limited | "Too many attempts. Try again in X minutes" | Disable form |

---

### 3.3 Email Verification

**Endpoints:**
- `GET /dev/auth/verify-email` (link click)
- `POST /dev/auth/verify-email` (manual code)

#### Flow: Email Link Click

```
User clicks email link
        │
        ▼
┌─────────────────┐
│ Verification    │
│ Processing...   │
└────────┬────────┘
         │
    ┌────┴────┐
    ▼         ▼
Success    Failed
    │         │
    ▼         ▼
"Email     "Link expired"
verified!" [Resend Email]
[Login]
```

#### Success State
```
┌─────────────────────────────────────────┐
│              ✓ Success!                 │
├─────────────────────────────────────────┤
│                                         │
│         Your email is verified          │
│                                         │
│    You can now log in to your account   │
│                                         │
│  ┌─────────────────────────────────┐   │
│  │         Go to Login             │   │
│  └─────────────────────────────────┘   │
│                                         │
└─────────────────────────────────────────┘
```

---

### 3.4 Resend Verification Email

**Endpoint:** `POST /dev/auth/resend-verification-email`

#### UI Design

```
┌─────────────────────────────────────────┐
│       Resend Verification Email         │
├─────────────────────────────────────────┤
│                                         │
│  Enter your email address and we'll     │
│  send a new verification link.          │
│                                         │
│  Email                                  │
│  ┌─────────────────────────────────┐   │
│  │                                 │   │
│  └─────────────────────────────────┘   │
│                                         │
│  ┌─────────────────────────────────┐   │
│  │        Send Email               │   │
│  └─────────────────────────────────┘   │
│                                         │
│  ← Back to Login                        │
│                                         │
└─────────────────────────────────────────┘
```

#### Security Note
- Always show same success message regardless of email existence
- Rate limit: 1 request per 60 seconds

---

### 3.5 Password Reset Request

**Endpoint:** `POST /dev/auth/request-password-reset`

#### Form

| Field | Type | Required |
|-------|------|----------|
| Email | email | Yes |

#### Success Message
> "If an account exists with this email, you'll receive password reset instructions shortly."

---

### 3.6 Password Reset

**Endpoint:** `POST /dev/auth/reset-password`

#### Form Fields

| Field | Type | Required | Validation |
|-------|------|----------|------------|
| New Password | password | Yes | Same as registration |
| Confirm Password | password | Yes | Must match |
| Reset Token | hidden | Yes | From URL parameter |

#### States

| State | UI |
|-------|-----|
| Valid token | Show password form |
| Expired token | "This link has expired. Request a new one." |
| Invalid token | "Invalid reset link." |
| Success | "Password updated! Redirecting to login..." |

---

### 3.7 User Profile

**Endpoint:** `GET /dev/auth/me`

#### Display Information

```
┌─────────────────────────────────────────┐
│              Your Profile               │
├─────────────────────────────────────────┤
│                                         │
│  ┌─────┐                                │
│  │     │  developer@example.com         │
│  │  👤 │  ✓ Email verified              │
│  │     │                                │
│  └─────┘                                │
│                                         │
├─────────────────────────────────────────┤
│                                         │
│  Account Created    Jan 15, 2025        │
│  Last Login         2 hours ago         │
│  Active Sessions    3 devices           │
│                                         │
├─────────────────────────────────────────┤
│                                         │
│  [Change Password]  [Manage Sessions]   │
│                                         │
└─────────────────────────────────────────┘
```

---

### 3.8 Logout

**Endpoint:** `POST /dev/auth/logout`

#### Behavior
- No confirmation required
- Clear all local tokens
- Redirect to login page
- Optional toast: "You've been logged out"

---

### 3.9 Token Refresh

**Endpoint:** `POST /dev/auth/refresh-token`

#### UX Considerations
- Silent background operation (invisible to user)
- On failure: Redirect to login with "Session expired" message
- Optional: Show warning modal 5 minutes before expiry

---

## 4. Application Management

### 4.1 Applications List

**Endpoint:** `GET /dev/applications`

#### Data Per Application

| Field | Display |
|-------|---------|
| Name | Text, clickable |
| Description | Truncated text |
| Status | Badge (Active/Inactive) |
| Tier | Badge (Free/Pro/Enterprise) |
| Quota Usage | Progress bar with percentage |
| Keys Count | Number |
| Webhooks Count | Number |
| Created | Relative timestamp |

#### Wireframe

```
┌─────────────────────────────────────────────────────────────────────┐
│  Applications                                    [+ Create App]     │
├─────────────────────────────────────────────────────────────────────┤
│  🔍 Search applications...                    Filter ▼   Sort ▼    │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ My Production App                           [Active] [Pro]    │ │
│  │ Production environment for main product                       │ │
│  │                                                               │ │
│  │ Quota: ████████████░░░░░░░░ 65% (65,000 / 100,000)           │ │
│  │                                                               │ │
│  │ 🔑 2 keys    🔗 3 webhooks    📅 Created 30 days ago         │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ Staging Environment                      [Active] [Free]      │ │
│  │ Testing and development                                       │ │
│  │                                                               │ │
│  │ Quota: ████░░░░░░░░░░░░░░░░ 20% (200 / 1,000)                │ │
│  │                                                               │ │
│  │ 🔑 1 key     🔗 1 webhook     📅 Created 45 days ago         │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│  Showing 1-2 of 2 applications          ◀ 1 ▶      20 per page ▼  │
└─────────────────────────────────────────────────────────────────────┘
```

#### Empty State

```
┌─────────────────────────────────────────┐
│                                         │
│            📦                           │
│                                         │
│    No applications yet                  │
│                                         │
│    Create your first application to     │
│    get started with Vaultless           │
│                                         │
│    [+ Create Application]               │
│                                         │
└─────────────────────────────────────────┘
```

---

### 4.2 Create Application

**Endpoint:** `POST /dev/applications`

#### Step 1: Application Details

```
┌─────────────────────────────────────────────────────────────────┐
│  Create New Application                                    ✕    │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Application Name *                                             │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ My Awesome App                                          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  Description                                                    │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ Production application for secure messaging             │   │
│  │                                                         │   │
│  │                                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│                              [Cancel]  [Create Application]     │
└─────────────────────────────────────────────────────────────────┘
```

#### Step 2: API Keys Generated (CRITICAL UX)

```
┌─────────────────────────────────────────────────────────────────┐
│  🎉 Application Created!                                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ⚠️  IMPORTANT: Save your secret key now!                       │
│      You won't be able to see it again.                         │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Secret Key                                                     │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ sk_live_abc123xyz789def456uvw...                   [📋] │   │
│  └─────────────────────────────────────────────────────────┘   │
│  ✓ Copied to clipboard                                          │
│                                                                 │
│  Publishable Key                                                │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ pk_live_def456uvw123abc789xyz...                   [📋] │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ☑ I have saved my secret key securely                         │
│                                                                 │
│                                    [Go to Application →]        │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

#### Key Display Rules
- Secret key: Show once, require checkbox confirmation
- Publishable key: Can be retrieved later
- Copy button with visual feedback
- Countdown timer before allowing dismissal (optional)

---

### 4.3 Application Detail

**Endpoint:** `GET /dev/applications/{id}/with_keys`

#### Layout

```
┌─────────────────────────────────────────────────────────────────────┐
│  ← Applications    My Production App              [Edit] [⚙️]      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  [Overview]  [Analytics]  [API Keys]  [Webhooks]  [Settings]       │
│  ─────────                                                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────────────┐  ┌─────────────────────────────────┐  │
│  │ Status                  │  │ Quick Stats                     │  │
│  │ ● Active                │  │                                 │  │
│  │                         │  │ Messages Today    12,456        │  │
│  │ Tier                    │  │ Bandwidth         234 MB        │  │
│  │ Pro Plan                │  │ Active Clients    89            │  │
│  │                         │  │                                 │  │
│  │ Created                 │  │ [View Full Analytics →]         │  │
│  │ Jan 15, 2025            │  │                                 │  │
│  └─────────────────────────┘  └─────────────────────────────────┘  │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ Quota Usage This Month                                        │ │
│  │                                                               │ │
│  │ ████████████████████░░░░░░░░░░ 65,000 / 100,000 messages     │ │
│  │                                                               │ │
│  │ Resets in 15 days                    [Upgrade Plan]           │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 4.4 Application Analytics

**Endpoint:** `GET /dev/applications/{id}/analytics`

#### Metrics Dashboard

```
┌─────────────────────────────────────────────────────────────────────┐
│  Analytics                                     Last 30 days ▼      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌────────────┐ │
│  │ Messages     │ │ Bandwidth    │ │ Storage      │ │ Cost       │ │
│  │   245,678    │ │   12.4 GB    │ │   2.1 GB     │ │   $45.23   │ │
│  │ ↑ 12%        │ │ ↑ 8%         │ │ ↓ 3%         │ │ ↑ 15%      │ │
│  └──────────────┘ └──────────────┘ └──────────────┘ └────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                     Messages Over Time                        │ │
│  │                                                               │ │
│  │     │                                    ╭─╮                  │ │
│  │     │                              ╭────╯  ╰──╮               │ │
│  │     │         ╭──────╮       ╭────╯          ╰╮              │ │
│  │     │    ╭───╯      ╰──────╯                  ╰──            │ │
│  │     │───╯                                                    │ │
│  │     └────────────────────────────────────────────────────    │ │
│  │       Jan 1        Jan 8       Jan 15      Jan 22    Jan 29  │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 4.5 Usage Charts

**Endpoint:** `GET /dev/applications/{id}/chart`

#### Chart Controls

| Control | Options |
|---------|---------|
| Granularity | Daily, Weekly |
| Metric | Messages, Bandwidth, Storage, Proofs, Rate Limits, Cost, All |
| Date Range | Custom picker (max 100 daily / 160 weekly buckets) |

#### Chart Types

| Metric | Recommended Chart |
|--------|-------------------|
| Messages over time | Line chart |
| Bandwidth | Area chart |
| Cost breakdown | Stacked bar |
| Comparisons | Grouped bar |

---

### 4.6 Usage Summary

**Endpoint:** `GET /dev/applications/usage-summary`

#### Dashboard Cards

```
┌─────────────────────────────────────────────────────────────────────┐
│  Usage Summary                                    This Month        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐ │
│  │ Total Apps        │ │ Total Messages    │ │ Total Bandwidth   │ │
│  │        5          │ │     1,234,567     │ │      45.6 GB      │ │
│  │ 4 active          │ │ ↑ 23% vs last mo  │ │ ↑ 12% vs last mo  │ │
│  └───────────────────┘ └───────────────────┘ └───────────────────┘ │
│                                                                     │
│  ┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐ │
│  │ Quota Used        │ │ Total Cost        │ │ Rate Limits Hit   │ │
│  │       67%         │ │     $234.56       │ │        23         │ │
│  │ ████████░░░░      │ │ Est. $280 final   │ │ ↓ 45% vs last mo  │ │
│  └───────────────────┘ └───────────────────┘ └───────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 4.7 Quota Warnings

**Endpoint:** `GET /dev/applications/quota-warnings`

#### Warning Levels

| Level | Threshold | Color | Action |
|-------|-----------|-------|--------|
| Normal | < 60% | Green | None |
| Warning | 60-80% | Yellow | Informational |
| High | 80-95% | Orange | Recommend upgrade |
| Critical | > 95% | Red | Urgent upgrade |

#### UI Design

```
┌─────────────────────────────────────────────────────────────────────┐
│  Quota Warnings                           Threshold: 80% ▼         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ⚠️  2 applications need attention                                  │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ 🔴 My Production App                              CRITICAL     │ │
│  │    Usage: 98% (98,000 / 100,000 messages)                     │ │
│  │    Estimated to exceed quota in 2 days                        │ │
│  │                                                               │ │
│  │    [View Details]  [Upgrade Now]                              │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │ 🟠 Staging Environment                            WARNING      │ │
│  │    Usage: 85% (850 / 1,000 messages)                          │ │
│  │    On track to exceed quota                                   │ │
│  │                                                               │ │
│  │    [View Details]  [Upgrade Now]                              │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│  Showing 1-2 of 2 warnings              ◀ 1 ▶      20 per page ▼  │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 4.8 Quota Status

**Endpoint:** `GET /dev/applications/{id}/quota-status`

#### Response Data

| Field | Type | Description |
|-------|------|-------------|
| application_id | UUID | Application identifier |
| messages_used | integer | Messages used this month |
| messages_limit | integer | Monthly message quota |
| usage_percentage | float | Current usage as percentage |
| is_over_quota | boolean | Whether quota is exceeded |
| overage_count | integer | Messages over quota |
| resets_at | timestamp | When quota resets |
| alert_level | string | null, "info", "warning", or "critical" |

#### UI Integration

Use this endpoint to display real-time quota status in:
- Application overview cards
- Quota warning banners
- Usage meters and progress bars

---

### 4.9 Cost Breakdown

**Endpoint:** `GET /dev/applications/{id}/costs`

#### Display

```
┌─────────────────────────────────────────────────────────────────────┐
│  Cost Breakdown - January 2025                                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Total Cost: $45.23                      Projected: $52.00          │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                                                               │ │
│  │    Messages          ████████████████████░░░░  $28.50  63%   │ │
│  │    Bandwidth         ████████░░░░░░░░░░░░░░░░  $10.23  23%   │ │
│  │    Storage           ████░░░░░░░░░░░░░░░░░░░░   $4.50  10%   │ │
│  │    Proofs            ██░░░░░░░░░░░░░░░░░░░░░░   $2.00   4%   │ │
│  │                                                               │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### 4.10 Trends Analysis

**Endpoint:** `GET /dev/applications/{id}/trends`

#### Insights Display

| Insight Type | Example |
|--------------|---------|
| Peak Usage | "Highest traffic: Tuesdays 2-4 PM UTC" |
| Growth | "Message volume up 23% month-over-month" |
| Anomaly | "Unusual spike detected on Jan 15" |

---

### 4.11 Export Functionality

**Endpoint:** `GET /dev/applications/{id}/export`

#### Export Options

```
┌─────────────────────────────────────────┐
│  Export Usage Data                      │
├─────────────────────────────────────────┤
│                                         │
│  Date Range                             │
│  ┌─────────────┐  ┌─────────────┐      │
│  │ 2025-01-01  │  │ 2025-01-31  │      │
│  └─────────────┘  └─────────────┘      │
│                                         │
│  Format                                 │
│  ○ CSV                                  │
│  ● JSON                                 │
│                                         │
│  Include Metrics                        │
│  ☑ Messages                             │
│  ☑ Bandwidth                            │
│  ☐ Storage                              │
│  ☑ Costs                                │
│                                         │
│  [Cancel]  [Download Export]            │
│                                         │
└─────────────────────────────────────────┘
```

---

## 5. Information Architecture

### 5.1 Navigation Structure

```
VAULTLESS DEVELOPER PORTAL
│
├── 🏠 Dashboard
│   └── Overview widgets, quick stats, recent activity
│
├── 📦 Applications
│   ├── All Applications (list view)
│   ├── + Create Application
│   └── [Application Name]
│       ├── Overview
│       ├── Analytics
│       ├── API Keys
│       ├── Webhooks
│       └── Settings
│
├── 📊 Usage Summary
│   └── Aggregated stats across all apps
│
├── ⚠️ Quota Warnings
│   └── Applications approaching/exceeding limits
│
├── 📚 Documentation (external)
│
└── 👤 Account (dropdown)
    ├── Profile
    ├── Account Settings
    ├── Billing
    └── Logout
```

### 5.2 Page Flow Diagram

```
                         ┌─────────────┐
                         │   Landing   │
                         │    Page     │
                         └──────┬──────┘
                                │
                 ┌──────────────┼──────────────┐
                 ▼              ▼              ▼
           ┌─────────┐    ┌─────────┐    ┌─────────┐
           │ Register│    │  Login  │    │  Docs   │
           └────┬────┘    └────┬────┘    └─────────┘
                │              │
                ▼              │
         ┌────────────┐        │
         │  Verify    │        │
         │   Email    │        │
         └─────┬──────┘        │
               │               │
               └───────┬───────┘
                       ▼
                ┌────────────┐
                │ Dashboard  │◄────────────────────┐
                └─────┬──────┘                     │
                      │                            │
         ┌────────────┼────────────┐               │
         ▼            ▼            ▼               │
    ┌────────┐  ┌──────────┐  ┌─────────┐          │
    │  Apps  │  │  Usage   │  │ Quota   │          │
    │  List  │  │ Summary  │  │Warnings │          │
    └───┬────┘  └──────────┘  └─────────┘          │
        │                                          │
        ▼                                          │
   ┌───────────────┐                               │
   │  App Detail   │                               │
   ├───────────────┤                               │
   │ • Overview    │                               │
   │ • Analytics   │                               │
   │ • API Keys    │                               │
   │ • Webhooks    │                               │
   │ • Settings    │                               │
   └───────────────┘                               │
                                                   │
   ┌───────────────┐                               │
   │    Profile    │───────────────────────────────┘
   │   (Logout)    │
   └───────────────┘
```

---

## 6. Design Requirements

### 6.1 Visual Design Guidelines

#### Color Palette

| Use | Light Mode | Dark Mode |
|-----|------------|-----------|
| Background | #FFFFFF | #1A1A2E |
| Surface | #F8F9FA | #252542 |
| Primary | #2563EB | #3B82F6 |
| Success | #10B981 | #34D399 |
| Warning | #F59E0B | #FBBF24 |
| Error | #EF4444 | #F87171 |
| Text Primary | #111827 | #F9FAFB |
| Text Secondary | #6B7280 | #9CA3AF |

#### Typography

| Element | Font | Size | Weight |
|---------|------|------|--------|
| H1 | Inter | 32px | 700 |
| H2 | Inter | 24px | 600 |
| H3 | Inter | 20px | 600 |
| Body | Inter | 16px | 400 |
| Small | Inter | 14px | 400 |
| Code | JetBrains Mono | 14px | 400 |

#### Spacing Scale

```
4px  - xs
8px  - sm
16px - md
24px - lg
32px - xl
48px - 2xl
```

### 6.2 Component Library

#### Buttons

| Variant | Use Case |
|---------|----------|
| Primary | Main actions (Create, Save, Submit) |
| Secondary | Alternative actions (Cancel, Back) |
| Danger | Destructive actions (Delete, Deactivate) |
| Ghost | Tertiary actions, links |
| Icon | Icon-only buttons |

#### Form Inputs

- Text input with label
- Password input with show/hide toggle
- Textarea
- Select dropdown
- Checkbox
- Radio group
- Date picker
- File upload

#### Feedback Components

- Toast notifications (success, error, warning, info)
- Alert banners
- Progress indicators
- Loading skeletons
- Empty states
- Error states

#### Data Display

- Data tables with sorting/filtering
- Cards
- Badges/Tags
- Progress bars
- Charts (line, bar, area, pie)
- Stats cards

### 6.3 Responsive Breakpoints

| Breakpoint | Width | Target |
|------------|-------|--------|
| Mobile | < 640px | Phone |
| Tablet | 640-1024px | Tablet |
| Desktop | 1024-1440px | Laptop |
| Large | > 1440px | Desktop |

### 6.4 Accessibility Requirements

- WCAG 2.1 AA compliance
- Keyboard navigation for all interactive elements
- Focus indicators visible
- Screen reader support (ARIA labels)
- Color contrast ratio ≥ 4.5:1
- Charts must have data table alternatives
- Reduced motion option
- High contrast mode support

### 6.5 Security UX Patterns

| Pattern | Implementation |
|---------|----------------|
| API Key Display | Masked by default, click to reveal (5s timeout) |
| Copy to Clipboard | Visual confirmation, optional auto-clear |
| Destructive Actions | Confirmation modal with typed confirmation |
| Session Timeout | Warning modal 5 min before expiry |
| Password Entry | Show/hide toggle, strength indicator |

---

## 7. Deliverables

### 7.1 Required Deliverables

| Deliverable | Description |
|-------------|-------------|
| Wireframes | Low-fidelity layouts for all pages |
| Hi-Fi Mockups | Full designs in light and dark mode |
| Prototype | Interactive Figma/similar prototype |
| Component Library | Reusable component specifications |
| Design Tokens | Colors, typography, spacing as code |
| Handoff Docs | Specs for engineering implementation |

### 7.2 Key User Flows to Prototype

1. **Onboarding Flow**
   - Register → Verify Email → Login → Dashboard

2. **Application Creation**
   - Dashboard → Create App → Save API Keys → View App

3. **Analytics Exploration**
   - App Detail → Analytics → Change Date Range → Export

4. **Quota Management**
   - Quota Warning → View Details → Upgrade Plan

5. **Password Recovery**
   - Login → Forgot Password → Email → Reset → Login

### 7.3 Design Review Checklist

- [ ] All states designed (default, hover, active, disabled, error, loading)
- [ ] Empty states for all lists
- [ ] Error handling for all forms
- [ ] Mobile responsive variants
- [ ] Dark mode variants
- [ ] Accessibility annotations
- [ ] Micro-interactions documented

---

## 8. Technical Context

### 8.1 API Information

| Aspect | Details |
|--------|---------|
| Base URL | `/dev/auth/*` for auth, `/dev/applications/*` for apps, `/api/*` for client APIs |
| Authentication | JWT Bearer tokens with PASETO |
| Token Refresh | Automatic via refresh token |
| Response Times | Design for 200-500ms typical |
| Pagination | `page` + `page_size` parameters |
| Rate Limits | Per-IP login attempts, per-key message rate limiting |

### 8.2 API Endpoints Reference

#### Authentication Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/dev/auth/register` | Register new user account |
| POST | `/dev/auth/login` | User login |
| POST | `/dev/auth/logout` | User logout |
| POST | `/dev/auth/refresh-token` | Refresh access token |
| GET | `/dev/auth/me` | Get current user profile |
| POST | `/dev/auth/verify-email` | Verify email with token |
| POST | `/dev/auth/resend-verification` | Resend verification email |
| POST | `/dev/auth/request-password-reset` | Request password reset |
| POST | `/dev/auth/reset-password` | Reset password with token |
| GET/POST | `/dev/auth/google` | Google OAuth initiation/callback |

#### Application Management Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/dev/applications` | List user's applications |
| POST | `/dev/applications` | Create new application |
| GET | `/dev/applications/{app_id}` | Get application details |
| PATCH | `/dev/applications/{app_id}` | Update application |
| DELETE | `/dev/applications/{app_id}` | Delete application |

#### API Key Management Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/dev/applications/{app_id}/keys/secret/rotate` | Rotate secret key |
| POST | `/dev/applications/{app_id}/keys/publishable/rotate` | Rotate publishable key |
| POST | `/dev/applications/{app_id}/keys/publishable/add` | Add publishable key |
| POST | `/dev/applications/{app_id}/keys/publishable/deactivate` | Deactivate publishable key |

#### Analytics & Usage Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/dev/applications/{app_id}/chart` | Get chart data |
| GET | `/dev/applications/{app_id}/quota-status` | Get quota status |
| GET | `/dev/applications/usage-summary` | Get usage summary across all apps |
| GET | `/dev/applications/quota-warnings` | Get quota warning list |
| GET | `/dev/applications/{app_id}/costs` | Get cost breakdown |
| GET | `/dev/applications/{app_id}/trends` | Get usage trends |
| GET | `/dev/applications/{app_id}/export` | Export usage data |

#### Client API Endpoints (for SDKs)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/messages/send` | Send instant message |
| GET | `/api/messages/inbox` | Fetch inbox messages |
| POST | `/api/messages/{id}/read` | Mark message as read |
| GET | `/api/messages/health` | Message service health |
| GET | `/api/messages/ws` | WebSocket upgrade |

### 8.3 Data Formats

| Data Type | Format | Example |
|-----------|--------|---------|
| IDs | UUID | `550e8400-e29b-41d4-a716-446655440000` |
| Timestamps | ISO 8601 | `2025-01-15T10:30:00Z` |
| API Keys | Prefixed | `sk_live_xxx`, `pk_live_xxx` |
| Currency | USD | `$45.23` |
| Key Prefixes | Format | `sk_live_xxxx`, `pk_live_xxxx` |
| Session Tokens | PASETO v2 | `v2.public.xxx...` |

### 8.4 API Key Security

| Key Type | Prefix | Exposure | Usage |
|----------|--------|----------|-------|
| Secret Key | `sk_live_` | Server-side only | Server authentication, signing |
| Publishable Key | `pk_live_` | Client-safe | Client authentication, rate limiting |

#### Key Management UX

- **Secret Key Display**: Shown once on creation/rotation, then hidden
- **Copy Functionality**: One-click copy with visual confirmation
- **Key Rotation**: Creates new key, deactivates old after grace period
- **Multiple Keys**: Support for multiple publishable keys per app

### 8.5 Constraints

- Maximum 200 items per page
- Chart data: Max 100 daily or 160 weekly buckets
- Session timeout: Configurable (default 30 min)
- Rate limiting: Show feedback when hit
- Message expiry: 7 days (configurable per app)
- Max message size: 10MB

---

## Appendix A: Glossary

| Term | Definition |
|------|------------|
| Secret Key | Server-side API key (`sk_live_xxx`), never exposed to client applications |
| Publishable Key | Client-side API key (`pk_live_xxx`), safe to expose in client code |
| PASETO | Platform-Agnostic SEcurity TOKn - modern token format replacing JWT |
| Quota | Monthly message/usage limit based on subscription tier |
| Tier | Subscription level (Free, Pro, Enterprise) |
| Webhook | HTTP callback for event notifications |
| Session Token | PASETO token for authenticating client users |
| JTI | JWT ID - unique identifier for message idempotency |
| Rate Limit | Per-minute message limit per API key |
| Key Rotation | Process of creating new API keys and deactivating old ones |

---

## Appendix B: Related Documents

- API Documentation (`/docs/API.md`)
- Brand Guidelines
- Engineering Handoff Guide
- QA Test Cases
- Database Schema (`/migrations/`)

---

*End of Document - Version 1.1 (2025-12-31)*
