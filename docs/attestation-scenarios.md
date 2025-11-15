# Attestation Configuration Scenarios

## Overview

The `integrity_config` JSONB field in the `applications` table controls which platforms can register clients and whether attestation is required.

---

## Scenario 1: Development/Testing Mode (No Attestation Required)

**Configuration:**
```json
{
  "allow_unauthenticated": true,
  "ios": {},
  "android": {},
  "web": {}
}
```

**Result:**
- ✅ iOS clients can register **WITHOUT** attestation
- ✅ Android clients can register **WITHOUT** attestation
- ✅ Web clients can register **WITHOUT** attestation
- ⚠️ **WARNING**: This should ONLY be used in development/testing environments

**Use Case:**
- Local development
- Testing without real devices
- Rapid prototyping

---

## Scenario 2: iOS + Android Required (Production)

**Configuration:**
```json
{
  "allow_unauthenticated": false,
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": ["com.example.app"],
    "reject_untrusted_device": true
  },
  "android": {
    "allowed_certificate_sha256": "def456...",
    "allowed_bundle_ids": ["com.example.app"],
    "reject_untrusted_device": false
  },
  "web": {}
}
```

**Result:**
- ✅ iOS clients **MUST** provide valid attestation
- ✅ Android clients **MUST** provide valid attestation
- ❌ Web clients **CANNOT** register (no web config)
- ❌ Clients without attestation **REJECTED**

**Use Case:**
- Mobile-only applications
- High security requirements
- Production mobile apps

---

## Scenario 3: iOS + Android + Web (Multi-Platform)

**Configuration:**
```json
{
  "allow_unauthenticated": false,
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": ["com.example.app"]
  },
  "android": {
    "allowed_certificate_sha256": "def456...",
    "allowed_bundle_ids": ["com.example.app"]
  },
  "web": {
    "authorized_origins": ["https://app.example.com", "https://staging.example.com"]
  }
}
```

**Result:**
- ✅ iOS clients with attestation → Accepted
- ✅ Android clients with attestation → Accepted
- ✅ Web clients from authorized origins → Accepted (no attestation needed)
- ❌ iOS/Android clients without attestation → **REJECTED**
- ❌ Web clients from unauthorized origins → **REJECTED**

**Use Case:**
- Full cross-platform applications
- Web + Mobile apps
- Standard production setup

---

## Scenario 4: Web Only

**Configuration:**
```json
{
  "allow_unauthenticated": false,
  "ios": {},
  "android": {},
  "web": {
    "authorized_origins": ["https://app.example.com"]
  }
}
```

**Result:**
- ❌ iOS clients **REJECTED** (not configured)
- ❌ Android clients **REJECTED** (not configured)
- ✅ Web clients from authorized origins → Accepted
- ❌ Web clients from other origins → **REJECTED**

**Use Case:**
- Web-only applications
- Progressive Web Apps (PWA)
- Browser-based services

---

## Scenario 5: iOS Only (Strict Security)

**Configuration:**
```json
{
  "allow_unauthenticated": false,
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": ["com.example.app"],
    "min_version_code": 100,
    "reject_untrusted_device": true
  },
  "android": {},
  "web": {}
}
```

**Result:**
- ✅ iOS clients with valid attestation, correct bundle ID, version ≥ 100, on trusted devices → Accepted
- ❌ iOS clients on jailbroken/untrusted devices → **REJECTED**
- ❌ iOS clients with old version → **REJECTED**
- ❌ Android clients → **REJECTED** (not configured)
- ❌ Web clients → **REJECTED** (not configured)

**Use Case:**
- iOS-exclusive applications
- High security iOS apps (banking, healthcare)
- Enterprise iOS apps

---

## Scenario 6: Gradual Rollout (iOS Strict, Android Permissive)

**Configuration:**
```json
{
  "allow_unauthenticated": false,
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": ["com.example.app"],
    "reject_untrusted_device": true
  },
  "android": {
    "allowed_certificate_sha256": "def456...",
    "allowed_bundle_ids": ["com.example.app"],
    "reject_untrusted_device": false
  },
  "web": {}
}
```

**Result:**
- ✅ iOS clients on trusted devices only
- ✅ Android clients on both trusted and untrusted devices (e.g., rooted phones allowed)
- ❌ Web clients rejected

**Use Case:**
- Different security policies per platform
- Gradual security rollout
- Android testing with rooted devices

---

## Scenario 7: Multiple Bundle IDs (Debug + Release)

**Configuration:**
```json
{
  "allow_unauthenticated": false,
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": [
      "com.example.app",
      "com.example.app.staging",
      "com.example.app.debug"
    ]
  },
  "android": {
    "allowed_certificate_sha256": "def456...",
    "allowed_bundle_ids": [
      "com.example.app",
      "com.example.app.debug"
    ]
  },
  "web": {}
}
```

**Result:**
- ✅ Multiple bundle IDs accepted (production, staging, debug)
- ✅ Useful for development with different build variants

**Use Case:**
- Supporting multiple build variants
- Staging environments
- Internal testing builds

---

## Helper Methods

### Check if Attestation is Required
```rust
let app = Application::find_by_id(&pool, app_id).await?;

// Check specific platform
if app.requires_attestation(Platform::IOS) {
    println!("iOS attestation required");
}

// Check if unauthenticated allowed
if app.allows_unauthenticated() {
    println!("Dev mode enabled - no attestation required");
}

// Get full requirements summary
let requirements = app.get_integrity_requirements();
println!("iOS required: {}", requirements.ios_attestation_required);
println!("Android required: {}", requirements.android_attestation_required);
println!("Allow unauthenticated: {}", requirements.allow_unauthenticated);
```

### Create Configurations Programmatically
```rust
// Development mode
let config = IntegrityConfig::dev_mode();

// Production iOS only
let config = IntegrityConfig::ios_only(
    "abc123...".into(),
    vec!["com.example.app".into()],
    true, // reject_untrusted
);

// Production Android only
let config = IntegrityConfig::android_only(
    "def456...".into(),
    vec!["com.example.app".into()],
    false, // allow untrusted for testing
);

// Web only
let config = IntegrityConfig::web_only(
    vec!["https://app.example.com".into()],
);
```

---

## Migration from Development to Production

### Step 1: Development (No Attestation)
```json
{
  "allow_unauthenticated": true,
  "ios": {},
  "android": {},
  "web": {}
}
```

### Step 2: Add Certificate Hashes (Still Allow Unauthenticated)
```json
{
  "allow_unauthenticated": true,
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": ["com.example.app"]
  },
  "android": {
    "allowed_certificate_sha256": "def456...",
    "allowed_bundle_ids": ["com.example.app"]
  },
  "web": {}
}
```

### Step 3: Enforce Attestation (Production)
```json
{
  "allow_unauthenticated": false,  // ← Change to false
  "ios": {
    "allowed_certificate_sha256": "abc123...",
    "allowed_bundle_ids": ["com.example.app"],
    "reject_untrusted_device": true
  },
  "android": {
    "allowed_certificate_sha256": "def456...",
    "allowed_bundle_ids": ["com.example.app"],
    "reject_untrusted_device": true
  },
  "web": {
    "authorized_origins": ["https://app.example.com"]
  }
}
```

---

## Security Best Practices

### ✅ DO:
- Use `allow_unauthenticated: true` only in development/testing
- Set `reject_untrusted_device: true` in production
- Specify `allowed_bundle_ids` to prevent unauthorized apps
- Use `min_version_code` to enforce minimum app versions
- Regularly update certificate hashes when releasing new versions
- Monitor attestation failures for potential attacks

### ❌ DON'T:
- Don't use `allow_unauthenticated: true` in production
- Don't leave `allowed_bundle_ids` empty in production
- Don't ignore attestation warnings
- Don't skip certificate hash validation
- Don't allow untrusted devices for sensitive applications

---

## Error Messages Reference

| Error | Cause | Solution |
|-------|-------|----------|
| "Platform attestation is required" | Client didn't provide attestation when required | Add attestation to registration request |
| "Platform X attestation is not configured" | Client provided attestation for unconfigured platform | Configure the platform or remove attestation |
| "No platforms are configured" | Empty config without `allow_unauthenticated` | Add platform configs or enable `allow_unauthenticated` |
| "Certificate hash mismatch" | Wrong app signing certificate | Use correct certificate or update config |
| "Bundle ID not in allowed list" | Unauthorized bundle ID | Add bundle ID to config or use correct bundle |
| "Untrusted devices are not allowed" | Device failed integrity check | Use trusted device or set `reject_untrusted_device: false` |
