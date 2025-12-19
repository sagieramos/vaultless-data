# Vaultless Developer Dashboard - UI Design Guide

## Overview

A dashboard for developers to manage their Vaultless applications, view analytics, and handle notifications.

---

## Pages

### 1. Login Page
**What it does:** Let users sign in

**Elements:**
- Email field
- Password field
- "Login" button
- "Forgot Password?" link
- "Sign up" link
- "Continue with Google" button

---

### 2. Register Page
**What it does:** Create new account

**Elements:**
- Email field
- Password field (min 8 characters)
- Name field (optional)
- "Create Account" button
- "Sign up with Google" button

---

### 3. Forgot Password Page
**What it does:** Request password reset email

**Elements:**
- Email field
- "Send Reset Link" button

---

### 4. Reset Password Page
**What it does:** Set new password (from email link)

**Elements:**
- New password field
- Confirm password field
- "Reset Password" button

---

### 5. Email Verification Page
**What it does:** Verify email (from email link)

**Elements:**
- Success/error message
- Link to login

---

### 6. Dashboard Home
**What it does:** Overview of user's applications

**Elements:**
- Welcome message
- Stats cards (total apps, API calls)
- "Create Application" button
- List of applications
- Notification bell with unread count

---

### 7. Applications List
**What it does:** Show all user's applications

**Elements:**
- "Create Application" button
- Table/cards showing:
  - App name
  - Status (active/inactive)
  - Created date
  - View/Edit/Delete buttons
- Pagination

---

### 8. Application Details
**What it does:** View single application with API keys

**Elements:**
- App name & description
- Status badge
- API Keys section:
  - Public Key (with copy button)
  - Secret Key (hidden, click to reveal)
- Usage chart
- Edit/Deactivate buttons

---

### 9. Analytics Page
**What it does:** View usage stats for an application

**Elements:**
- Date range picker
- Usage chart (line graph)
- Stats: total requests, unique users
- Quota warnings (if any)
- Export button (CSV/JSON)

---

### 10. Notifications Page
**What it does:** View system notifications

**Elements:**
- Filter tabs: All / Unread
- Filter by type dropdown
- Notification list showing:
  - Icon
  - Title
  - Message preview
  - Time
  - Read/unread dot
- "Mark All Read" button
- Pagination

---

### 11. Profile Page
**What it does:** View/edit user info

**Elements:**
- Avatar
- Name
- Email (with "verified" badge)
- "Logout" button

---

## Navigation

```
Top Bar:
[Logo]  [Dashboard]  [Applications]  [Notifications 🔔]  [Profile ▼]
```

---

## Notification Types

| Type | Color |
|------|-------|
| Quota Warning | Orange |
| Security Alert | Red |
| System Update | Blue |
| Billing | Purple |
| Feature Announcement | Green |

---

## Notes for Designer

1. **Google Login** - Use standard Google sign-in button style
2. **API Keys** - Secret key should be hidden by default (show •••••)
3. **Charts** - Simple line chart for usage over time
4. **Mobile** - All pages should work on mobile (single column layout)
