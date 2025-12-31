## Project Brief

**Vaultless** is a secure, privacy-first messaging platform with a developer portal for managing applications, API keys, usage analytics, and billing. Our goal is to make developers fall in love with our platform through exceptional UX.

**Core Value Propositions to Communicate:**
- ⚡ **Speed**: 1 Redis roundtrip for message sending (vs competitors' multiple calls)
- 🔒 **Security**: PASETO tokens, envelope encryption, client attestation
- 💰 **Cost-Effective**: Clear pricing, no surprise bills, generous free tier
- 🎯 **Developer Experience**: Instant integration, WebSocket support, comprehensive SDKs

---

## Target Audience

### Primary Personas

**1. The Indie Hacker (Alex)**
- Building solo, wants to ship fast
- Pain points: Complex setup, unclear docs, expensive pricing
- Needs: Quick start guide, copy-paste code samples, free tier

**2. The Startup Engineer (Jordan)**
- Building at a fast-growing startup
- Pain points: Scaling costs, reliability, security compliance
- Needs: Clear analytics, rate limit visibility, enterprise features

**3. The Enterprise Architect (Sam)**
- Evaluating for large-scale deployment
- Pain points: Security audits, compliance, vendor lock-in
- Needs: Audit logs, key rotation, SOC2 compliance indicators

---

## Design Principles for High Adoption

### 1. **Reduce Time to First Message (TTFM)**
- Goal: Developer should send their first message in < 5 minutes
- Implementation: Progressive disclosure, smart defaults, inline code examples

### 2. **Show, Don't Tell**
- Use interactive demos on landing page
- Live API playground in documentation
- One-click "Try with my project" buttons

### 3. **Trust Signals**
- Security badges and compliance certifications visible
- "X companies trust us" social proof
- Real-time system status dashboard

### 4. **Progressive Disclosure**
- Simple view for beginners, advanced view for power users
- Tooltips with explanations for complex features
- "Learn more" expandable sections

### 5. **Feedback Loops**
- Success animations on key actions
- Clear progress indicators
- Celebration moments (first message sent, first user onboarded)

---

## Page-by-Page Requirements

### 1. Landing Page (Marketing)

**Hero Section:**
- Headline: "Ship Secure Messaging in Minutes, Not Days"
- Subhead: "The fastest way to add end-to-end encrypted messaging to your app"
- CTA Primary: "Start Building Free" (prominent, contrasting color)
- CTA Secondary: "Watch Demo" (video modal)
- Trust badges: "SOC2 Compliant", "256-bit Encryption", "99.99% Uptime"

**Social Proof Strip:**
- "Trusted by 10,000+ developers"
- Logo carousel of tech companies

**Value Props Grid:**
- ⚡ "1-Click Integration" - Copy-paste code samples
- 🔒 "Zero-Knowledge Security" - Encryption explained simply
- 💰 "Predictable Pricing" - No surprise bills
- 📊 "Real-Time Analytics" - Usage you can see

**Interactive Demo:**
- Live playground: Send a test message right on the page
- Shows latency metrics in real-time
- "Wow, that was fast!" micro-interaction

**Code Preview:**
```javascript
// Send a message in 3 lines of code
const vaultless = new Vaultless('pk_live_xxx');
await vaultless.messages.send({
  to: 'user_id',
  ciphertext: '...'
});
console.log('Message sent in 12ms!');
```

**Testimonials:**
3-4 developer quotes with photos

**FAQ Accordion:**
- "Is my data really encrypted?"
- "What happens if I exceed my quota?"
- "Can I migrate from another service?"

**Footer:**
- Documentation link
- Status page link
- Pricing, Terms, Privacy
- GitHub, Twitter links

---

### 2. Registration Flow

**Design Requirements:**
- Single-page, distraction-free
- Progress indicator: "Step 1 of 2"
- Real-time validation with friendly error messages
- Password strength meter with visual feedback
- "Show password" toggle
- "I agree to Terms" checkbox with link
- Loading state on submit button
- Success page with "Verify Email" CTA
- "Already have an account? Log in" link

**Error States:**
- Email already exists: "An account exists with this email. Log in instead?"
- Weak password: "Password must include uppercase, number, special char"
- Passwords don't match: "Passwords must match"

**Success State:**
- "Check your email!" with email address shown
- "Didn't receive it?" with resend link
- "Check spam folder" helpful tip

---

### 3. Login Page

**Design Requirements:**
- Clean, focused layout
- Email + password fields
- "Remember me" checkbox
- "Forgot password?" link
- "Log in" button
- "Don't have an account? Register" link
- "Continue with Google" SSO option

**Error Handling:**
- Invalid credentials: "Invalid email or password" (don't reveal which is wrong)
- Account locked: Show unlock timer
- Email not verified: Show resend verification link
- Rate limiting: "Too many attempts. Try again in X minutes"

**Success State:**
- Smooth transition to dashboard
- Loading animation
- "Welcome back, [Name]!"

---

### 4. Dashboard (Post-Login Home)

**Header:**
- Logo left
- Nav: Dashboard, Apps, Docs, Support
- User menu: Avatar dropdown (Profile, Settings, Billing, Logout)
- Notifications bell with badge

**Welcome Banner:**
- "Welcome back, [Name]!"
- "You have 3 active applications"
- Quick action: "+ New Application" button

**Stats Row:**
- Total Messages (this month)
- Active Applications
- Quota Used (across all apps)
- Total Cost (this month)

**Recent Activity:**
- Recent messages sent
- Application created
- Key rotated
- 7-day trend sparkline

**Quick Actions Grid:**
- [+ New App] - Large, inviting button
- [View Analytics] - Charts icon
- [Manage Keys] - Key icon
- [Documentation] - Book icon

**Empty State (no apps):**
- Friendly illustration
- "Ready to build something great?"
- "+ Create Your First Application" button
- "No coding required - 2 minutes to setup"

---

### 5. Application List

**Header:**
- Title: "Applications"
- Search bar
- Filter dropdown (All, Active, Inactive)
- Sort dropdown (Name, Created, Usage)
- "+ Create App" button

**Application Cards (Grid Layout):**

Each card shows:
- App name with status badge (Active = green dot, Inactive = gray)
- Tier badge (Free, Pro, Enterprise)
- Description (truncated)
- **Quota Progress Bar**: Visual with percentage and numbers
- Quick stats row: "🔑 2 keys", "🔗 3 webhooks"
- "Created [relative time]"
- Hover actions: [Edit] [Analytics] [Settings]

**Empty State:**
- Illustration
- "No applications yet"
- "Create your first app to get started"
- [+ Create Application] button

**Pagination:**
- Bottom of list
- "Showing 1-10 of 47 applications"
- Page numbers with ellipsis
- "20 per page" dropdown

---

### 6. Create Application Flow

**Step 1: App Details**
- App Name (with character counter)
- Description (optional, textarea)
- "Next" button
- "Cancel" link

**Step 2: Keys Generated (CRITICAL UX MOMENT)**

**Design Requirements:**
- Full-screen modal or centered card
- Background pattern or gradient
- **Huge success state**: "🎉 Application Created!"
- Warning banner: "⚠️ IMPORTANT: Save your secret key now! You won't see it again."

**Key Display Section:**
```
┌─────────────────────────────────────────────┐
│  SECRET KEY (COPY NOW)                      │
│  ┌───────────────────────────────────────┐  │
│  │ sk_live_abc123xyz789def456...    [📋] │  │
│  └───────────────────────────────────────┘  │
│  ✓ Copied to clipboard!                     │
│                                             │
│  PUBLISHABLE KEY                            │
│  ┌───────────────────────────────────────┐  │
│  │ pk_live_def456uvw789abc123...    [📋] │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

**Key UX Requirements:**
- Secret key masked by default
- "Reveal" button with countdown (5s auto-mask)
- Copy button with animation and "Copied!" tooltip
- "I've saved my secret key" checkbox (required)
- "Continue to Dashboard" button (disabled until checkbox checked)

**Copy Animation:**
- Button changes to checkmark
- Toast notification: "Secret key copied!"
- Sound effect (optional, off by default)

---

### 7. Application Detail View

**Page Header:**
```
┌─────────────────────────────────────────────────────┐
│  ← Back to Apps     My Production App    [Edit] ⚙️  │
├─────────────────────────────────────────────────────┤
│  [Overview] [Analytics] [Keys] [Webhooks] [Settings]│
└─────────────────────────────────────────────────────┘
```

**Tabs:**
- **Overview**: Quick stats, quota, recent activity
- **Analytics**: Charts, trends, exports
- **Keys**: Key management, rotation, audit log
- **Webhooks**: Add, edit, test webhooks
- **Settings**: App configuration, deletion

**Overview Tab Content:**
- Status card: "● Active" | "Tier: Pro" | "Created: Jan 15"
- 4 KPI cards: Messages Today, Bandwidth, Active Clients, Cost
- **Quota Usage Meter**: Large, colorful progress bar
  - Current / Limit (e.g., "65,000 / 100,000 messages")
  - Percentage: "65%"
  - "Resets in 15 days"
  - [Upgrade Plan] button (if > 80%)
- "Upgrade to Pro" banner (if free tier)
- Recent Activity feed

---

### 8. Analytics Dashboard

**Header Controls:**
- Date range picker (with presets: 7d, 30d, 90d, YTD, Custom)
- Granularity: "Daily" | "Weekly"
- Metric selector: Messages, Bandwidth, Storage, Cost, All
- [Export] button (CSV/JSON)

**Charts Section:**
- Main line chart (messages over time)
- Area chart (bandwidth)
- Stacked bar (cost breakdown)
- Sparklines for quick trends

**Metrics Cards:**
- This month vs last month comparison
- Percentage change indicators (↑ green, ↓ red)
- "Projected end of month" estimates

**Export Modal:**
- Format: CSV | JSON
- Date range: From / To
- Metrics: Checkboxes (Messages, Bandwidth, Cost)
- [Download] button

---

### 9. API Keys Management

**Header:**
- Title: "API Keys"
- [+ Add Key] button
- "Rotate" tooltips on each key

**Key List Table:**

| Key Type | Prefix | Created | Last Used | Status | Actions |
|----------|--------|---------|-----------|--------|---------|
| Secret | sk_live_abc... | Jan 15 | - | Active | [Rotate] |
| Publishable | pk_live_def... | Jan 15 | 2 hours ago | Active | [Rotate] [Copy] |

**Key Actions:**
- **Copy**: One-click copy with toast notification
- **Rotate**: Opens modal, explains rotation, shows new key once
- **Deactivate**: Only for publishable keys, confirms with input
- **Reveal**: For secret keys (if shown), auto-masks after 5s

**Rotation Modal:**
- Warning: "This will invalidate all existing sessions using this key"
- Confirmation: Type "ROTATE" to confirm
- Result: Shows new key once, requires save confirmation

---

### 10. Quota & Billing

**Quota Status Card:**
- Large gauge/ring chart
- "65,000 / 100,000 messages"
- "65% used"
- "Resets in 15 days"
- Warning colors: Green (<60%), Yellow (60-80%), Orange (80-95%), Red (>95%)

**Usage This Month:**
- Messages: 65,000 / 100,000
- Bandwidth: 12.4 GB / 50 GB
- Storage: 2.1 GB / 10 GB
- Webhooks: 3 / 10

**Cost Breakdown:**
- This month: $45.23
- Projected: $52.00
- Visual breakdown pie chart

**Upgrade Options:**
- Free: 1,000 msgs/mo, $0
- Pro: 100,000 msgs/mo, $29/mo
- Enterprise: Unlimited, $99/mo

**Billing History:**
- Table: Date, Description, Amount, Status
- [Download Invoice] link

---

### 11. Documentation Hub

**Sidebar Navigation:**
- Getting Started
- Authentication
- API Reference
- SDKs
- Webhooks
- Security

**Content Area:**
- Search bar (cmd+k shortcut)
- Table of contents sidebar
- Code tabs (curl, JavaScript, Python, Go)
- Copy code button
- "Run in your terminal" button
- "Edit this page" link (GitHub)

**Interactive API Playground:**
- Pre-filled with user's API key (from localStorage)
- Try-it-right-now request builder
- Response viewer with syntax highlighting
- "Share" button for examples

---

### 12. Error Pages

**404 Not Found:**
- Friendly illustration
- "This page doesn't exist"
- "Go back home" button
- Search bar

**403 Forbidden:**
- "You don't have access to this"
- "Contact the app owner" link

**500 Error:**
- "Something went wrong"
- "Our team has been notified"
- "Refresh" button
- Status page link

**Rate Limited:**
- "Too many requests"
- "Slow down! Try again in X seconds"
- Visual countdown
- "Learn about rate limits" link

---

## Component Specifications

### Buttons

| Variant | Use Case | Style |
|---------|----------|-------|
| Primary CTA | "Start Building Free", "Create App" | Large, full-width, brand color |
| Secondary | "Cancel", "Back" | Outline, subtle |
| Destructive | "Delete App", "Deactivate Key" | Red, confirmation required |
| Icon Only | Copy, Settings | 44x44px touch target |
| Social | Google login | Brand colors with logo |

### Form Inputs

- **Text**: With label, placeholder, helper text, error message below
- **Password**: Show/hide toggle, strength meter
- **Select**: Clean dropdown, search filter (for many options)
- **Date Picker**: Calendar popup, date range mode
- **Toggle**: Switch style for boolean values

### Feedback Components

- **Toast**: Bottom-right, auto-dismiss, 3s duration
  - Success: Green checkmark
  - Error: Red X
  - Warning: Yellow triangle
  - Info: Blue i
- **Loading**: Skeleton screens for data tables, spinners for actions
- **Empty State**: Illustration + message + action button
- **Modal**: Centered, backdrop blur, escape to close

### Charts

- Line chart: Messages over time
- Area chart: Bandwidth usage
- Stacked bar: Cost breakdown
- Donut: Quota usage
- Sparklines: Trend indicators

---

## Color Palette

### Light Mode
| Role | Hex | Usage |
|------|-----|-------|
| Primary | #2563EB | Buttons, links, accents |
| Primary Hover | #1D4ED8 | Button hover states |
| Success | #10B981 | Success states, active badges |
| Warning | #F59E0B | Warning banners, caution |
| Error | #EF4444 | Errors, destructive actions |
| Background | #FFFFFF | Page backgrounds |
| Surface | #F8F9FA | Cards, panels |
| Text Primary | #111827 | Headings, primary text |
| Text Secondary | #6B7280 | Body text, labels |
| Border | #E5E7EB | Dividers, inputs |

### Dark Mode
| Role | Hex | Usage |
|------|-----|-------|
| Primary | #3B82F6 | Buttons, links, accents |
| Primary Hover | #60A5FA | Button hover states |
| Success | #34D399 | Success states |
| Warning | #FBBF24 | Warnings |
| Error | #F87171 | Errors |
| Background | #0F172A | Page backgrounds |
| Surface | #1E293B | Cards, panels |
| Text Primary | #F9FAFB | Headings |
| Text Secondary | #94A3B8 | Body text |
| Border | #334155 | Dividers |

---

## Typography

| Element | Font | Size | Weight | Line Height |
|---------|------|------|--------|-------------|
| H1 | Inter | 32px | 700 | 1.2 |
| H2 | Inter | 24px | 600 | 1.3 |
| H3 | Inter | 20px | 600 | 1.4 |
| Body | Inter | 16px | 400 | 1.5 |
| Small | Inter | 14px | 400 | 1.5 |
| Code | JetBrains Mono | 14px | 400 | 1.6 |
| Code Block | JetBrains Mono | 13px | 400 | 1.6 |

---

## Interactions & Animations

### Micro-interactions
- **Hover**: Subtle lift on cards, color change on buttons
- **Focus**: Visible ring on interactive elements
- **Click**: Scale down slightly, then bounce back
- **Success**: Checkmark animation, confetti on first message
- **Copy**: Button morphs to checkmark, then back

### Page Transitions
- Fade in on navigation
- Slide in for modals
- Staggered list animations
- Loading skeletons for async content

### Performance
- Smooth 60fps animations
- No layout thrashing
- Optimistic UI updates
- Instant feedback (< 100ms)

---

## Accessibility Requirements

- **WCAG 2.1 AA** compliance
- **Keyboard navigation** for all interactive elements
- **Focus indicators** visible on all interactive elements
- **Screen reader** support with ARIA labels
- **Color contrast** ratio ≥ 4.5:1
- **Reduced motion** option support
- **Skip to content** link
- **Form labels** always visible
- **Error messages** linked to inputs with aria-describedby

---

## Success Metrics (for Iteration)

### Adoption Metrics
- Time to first message (target: < 5 minutes)
- Registration completion rate (target: > 70%)
- Activation rate (target: > 50% within 24 hours)
- Return visitor rate (target: > 60% weekly)

### Engagement Metrics
- API key created per registration (target: > 80%)
- Message sent per key (target: > 3)
- Dashboard visits per user (target: > 5/month)
- Documentation page views (target: > 10/user)

### Satisfaction Metrics
- NPS score (target: > 50)
- Support ticket volume (target: decreasing)
- Churn rate (target: < 5% monthly)

---

## Deliverables

Please provide:

1. **Design System**
   - Color tokens (light/dark)
   - Typography scale
   - Spacing system
   - Component library (Figma/Sketch)

2. **Mockups** (Light + Dark)
   - Landing page (desktop + mobile)
   - Registration flow
   - Login page
   - Dashboard
   - Application list
   - Create app flow (key reveal moment)
   - Application detail
   - Analytics dashboard
   - API key management
   - Quota & billing
   - Documentation hub

3. **Interactive Prototype**
   - Clickable prototype in Figma
   - All key flows functional
   - Animations included

4. **Handoff Specs**
   - CSS variables
   - Component props
   - Animation specifications
   - Accessibility notes

---

## Evaluation Criteria

Your design will be evaluated on:

1. **Clarity**: Can a new developer understand the value proposition in 5 seconds?
2. **Speed**: Does the design encourage quick onboarding?
3. **Trust**: Does it communicate security and reliability?
4. **Beauty**: Is it visually appealing and professional?
5. **Completeness**: Are all user flows covered?
6. **Accessibility**: Can everyone use it?
7. **Differentiation**: How does it compare to competitors (Twilio, SendGrid, Stream)?

---

## Additional Context

**Competitors to Research:**
- Twilio - Enterprise focus, complex pricing
- Stream - Developer-friendly, chat SDKs
- SendBird - Enterprise chat
- Supabase - Developer experience leader

**Vaultless Differentiators to Highlight:**
1. **Simpler pricing** - No per-message tier confusion
2. **Better DX** - Instant integration, not 2 weeks
3. **Stronger security** - Zero-knowledge, envelope encryption
4. **WebSocket native** - Real-time from day one
5. **Open source SDKs** - Transparency and community

**Brand Personality:**
- Professional but friendly
- Security-focused but accessible
- Powerful but simple
- Transparent and honest


