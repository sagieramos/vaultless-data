// Integration tests for auth system
// These tests run against a real PostgreSQL database

use std::net::IpAddr;
use uuid::Uuid;
use vaultless_core::models::auth::{LoginAttempt, RefreshToken, User, UserSession};

// ============================================================================
// TEST SETUP
// ============================================================================

async fn setup_test_db() -> sqlx::PgPool {
    // Get database URL from environment or use default
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://vaultless:vaultless_dev_pass@localhost:5432/vaultless_test".to_string()
    });

    println!("🔌 Connecting to test database: {}", database_url);

    // Connect to database
    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .expect("❌ Failed to connect to test database. Make sure PostgreSQL is running!");

    // Run migrations
    println!("🔄 Running migrations...");
    sqlx::migrate!("../vaultless-api/migrations")
        .run(&pool)
        .await
        .expect("❌ Failed to run migrations");

    println!("✅ Database setup complete\n");

    pool
}

async fn cleanup_user(pool: &sqlx::PgPool, email: &str) {
    let _ = sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await;
}

async fn cleanup_ip(pool: &sqlx::PgPool, ip: &str) {
    let _ = sqlx::query("DELETE FROM login_attempts WHERE ip_address = $1::inet")
        .bind(ip)
        .execute(pool)
        .await;
}

// ============================================================================
// USER REGISTRATION & AUTHENTICATION TESTS
// ============================================================================

#[tokio::test]
async fn test_complete_registration_flow() {
    let pool = setup_test_db().await;
    let test_email = "register_flow@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("📝 Testing user registration flow...");

    // Step 1: Create user
    let user = User::create(
        &pool,
        test_email.to_string(),
        "StrongPassword123!".to_string(),
        Some("Test User".to_string()),
    )
    .await
    .expect("Failed to create user");

    println!("✅ User created: {}", user.id);

    assert_eq!(user.email, test_email);
    assert_eq!(user.name, Some("Test User".to_string()));
    assert!(!user.email_verified, "Email should not be verified yet");
    assert!(user.is_active, "User should be active");
    assert!(!user.is_admin, "User should not be admin");
    assert!(
        user.email_verification_token.is_some(),
        "Verification token should exist"
    );

    // Step 2: Verify password
    assert!(
        user.verify_password("StrongPassword123!").unwrap(),
        "Password verification should succeed"
    );
    assert!(
        !user.verify_password("WrongPassword").unwrap(),
        "Wrong password should fail"
    );

    println!("✅ Password verification works");

    // Step 3: Verify email
    let verification_token = user.email_verification_token.clone().unwrap();
    let verified_user = User::verify_email(&pool, &verification_token)
        .await
        .expect("Email verification failed");

    assert!(verified_user.email_verified, "Email should be verified");
    assert!(
        verified_user.email_verification_token.is_none(),
        "Token should be cleared"
    );

    println!("✅ Email verification successful");

    cleanup_user(&pool, test_email).await;
    println!("✅ Registration flow test complete\n");
}

#[tokio::test]
async fn test_duplicate_email_rejection() {
    let pool = setup_test_db().await;
    let test_email = "duplicate@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("🔒 Testing duplicate email rejection...");

    // Create first user
    User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .expect("First user creation failed");

    println!("✅ First user created");

    // Attempt to create duplicate
    let result = User::create(
        &pool,
        test_email.to_string(),
        "Password456".to_string(),
        None,
    )
    .await;

    assert!(result.is_err(), "Duplicate email should be rejected");

    match result {
        Err(vaultless_core::VaultlessError::Conflict(msg)) => {
            println!("✅ Correctly rejected duplicate: {}", msg);
            assert!(msg.contains("already registered"));
        }
        _ => panic!("Expected Conflict error"),
    }

    cleanup_user(&pool, test_email).await;
    println!("✅ Duplicate email test complete\n");
}

#[tokio::test]
async fn test_user_lookup() {
    let pool = setup_test_db().await;
    let test_email = "lookup@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("🔍 Testing user lookup...");

    let created_user = User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        Some("Lookup Test".to_string()),
    )
    .await
    .unwrap();

    // Test find by email
    let found_by_email = User::find_by_email(&pool, test_email)
        .await
        .expect("Find by email failed");

    assert_eq!(found_by_email.id, created_user.id);
    assert_eq!(found_by_email.email, test_email);
    println!("✅ Find by email works");

    // Test find by ID
    let found_by_id = User::find_by_id(&pool, created_user.id)
        .await
        .expect("Find by ID failed");

    assert_eq!(found_by_id.email, test_email);
    assert_eq!(found_by_id.name, Some("Lookup Test".to_string()));
    println!("✅ Find by ID works");

    // Test not found
    let random_email = "nonexistent@vaultless.test";
    let not_found = User::find_by_email(&pool, random_email).await;
    assert!(not_found.is_err(), "Should not find non-existent user");
    println!("✅ Not found handling works");

    cleanup_user(&pool, test_email).await;
    println!("✅ User lookup test complete\n");
}

// ============================================================================
// PASSWORD RESET TESTS
// ============================================================================

#[tokio::test]
async fn test_password_reset_flow() {
    let pool = setup_test_db().await;
    let test_email = "reset@vaultless.test";
    let old_password = "OldPassword123";
    let new_password = "NewPassword456";

    cleanup_user(&pool, test_email).await;

    println!("🔐 Testing password reset flow...");

    // Create user
    let user = User::create(
        &pool,
        test_email.to_string(),
        old_password.to_string(),
        None,
    )
    .await
    .unwrap();

    println!("✅ User created with old password");

    // Request password reset
    let reset_token = User::request_password_reset(&pool, test_email)
        .await
        .expect("Password reset request failed");

    assert!(!reset_token.is_empty(), "Reset token should not be empty");
    println!("✅ Reset token generated: {}...", &reset_token[..10]);

    // Reset password
    let updated_user = User::reset_password(&pool, &reset_token, new_password.to_string())
        .await
        .expect("Password reset failed");

    assert_eq!(updated_user.id, user.id);
    println!("✅ Password reset successful");

    // Verify old password no longer works
    let refetched_user = User::find_by_id(&pool, user.id).await.unwrap();
    assert!(
        !refetched_user.verify_password(old_password).unwrap(),
        "Old password should not work"
    );
    println!("✅ Old password rejected");

    // Verify new password works
    assert!(
        refetched_user.verify_password(new_password).unwrap(),
        "New password should work"
    );
    println!("✅ New password accepted");

    cleanup_user(&pool, test_email).await;
    println!("✅ Password reset flow test complete\n");
}

#[tokio::test]
async fn test_expired_reset_token() {
    let pool = setup_test_db().await;
    let test_email = "expired_reset@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("⏰ Testing expired reset token...");

    User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .unwrap();

    // Try to use an invalid/expired token
    let fake_token = "invalid_token_xyz";
    let result = User::reset_password(&pool, fake_token, "NewPassword456".to_string()).await;

    assert!(result.is_err(), "Expired token should be rejected");
    match result {
        Err(vaultless_core::VaultlessError::Unauthorized(_)) => {
            println!("✅ Expired token correctly rejected");
        }
        _ => panic!("Expected Unauthorized error"),
    }

    cleanup_user(&pool, test_email).await;
    println!("✅ Expired token test complete\n");
}

// ============================================================================
// REFRESH TOKEN TESTS
// ============================================================================

#[tokio::test]
async fn test_refresh_token_lifecycle() {
    let pool = setup_test_db().await;
    let test_email = "refresh@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("🔄 Testing refresh token lifecycle...");

    let user = User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .unwrap();

    let token_hash = "test_refresh_token_hash";
    let token_family = Uuid::new_v4();

    // Create refresh token
    let refresh_token =
        RefreshToken::create(&pool, user.id, token_hash.to_string(), token_family, 30)
            .await
            .expect("Failed to create refresh token");

    assert_eq!(refresh_token.user_id, user.id);
    assert_eq!(refresh_token.token_hash, token_hash);
    assert_eq!(refresh_token.token_family, token_family);
    assert!(!refresh_token.is_used);
    assert!(!refresh_token.is_revoked);
    println!("✅ Refresh token created");

    // Find by hash
    let found_token = RefreshToken::find_by_hash(&pool, token_hash)
        .await
        .expect("Failed to find refresh token");

    assert_eq!(found_token.id, refresh_token.id);
    println!("✅ Refresh token lookup works");

    cleanup_user(&pool, test_email).await;
    println!("✅ Refresh token lifecycle test complete\n");
}

#[tokio::test]
async fn test_refresh_token_rotation() {
    let pool = setup_test_db().await;
    let test_email = "rotation@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("🔁 Testing refresh token rotation...");

    let user = User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .unwrap();

    let old_token_hash = "old_token_hash";
    let new_token_hash = "new_token_hash";
    let token_family = Uuid::new_v4();

    // Create initial token
    let old_token =
        RefreshToken::create(&pool, user.id, old_token_hash.to_string(), token_family, 30)
            .await
            .unwrap();

    println!("✅ Old token created");

    // Rotate token
    let new_token = RefreshToken::rotate(&pool, old_token.id, new_token_hash.to_string())
        .await
        .expect("Token rotation failed");

    println!("✅ Token rotated");

    // Verify old token is marked as used
    let old_token_updated = RefreshToken::find_by_hash(&pool, old_token_hash)
        .await
        .unwrap();

    assert!(
        old_token_updated.is_used,
        "Old token should be marked as used"
    );
    assert!(
        old_token_updated.used_at.is_some(),
        "Used timestamp should be set"
    );
    println!("✅ Old token marked as used");

    // Verify new token is in same family
    assert_eq!(
        new_token.token_family, token_family,
        "Should be same family"
    );
    assert_eq!(
        new_token.parent_token_id,
        Some(old_token.id),
        "Should link to parent"
    );
    assert!(!new_token.is_used, "New token should not be used");
    assert!(!new_token.is_revoked, "New token should not be revoked");
    println!("✅ New token properly linked");

    cleanup_user(&pool, test_email).await;
    println!("✅ Token rotation test complete\n");
}

#[tokio::test]
async fn test_token_family_revocation() {
    let pool = setup_test_db().await;
    let test_email = "family_revoke@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("🚫 Testing token family revocation (theft detection)...");

    let user = User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .unwrap();

    let token_family = Uuid::new_v4();

    // Create multiple tokens in same family (simulating token theft)
    let _token1 = RefreshToken::create(&pool, user.id, "token1".to_string(), token_family, 30)
        .await
        .unwrap();

    let _token2 = RefreshToken::create(&pool, user.id, "token2".to_string(), token_family, 30)
        .await
        .unwrap();

    let _token3 = RefreshToken::create(&pool, user.id, "token3".to_string(), token_family, 30)
        .await
        .unwrap();

    println!("✅ Created 3 tokens in family: {}", token_family);

    // Revoke entire family (theft detected!)
    RefreshToken::revoke_family(&pool, token_family)
        .await
        .expect("Family revocation failed");

    println!("✅ Family revoked");

    // Verify all tokens are revoked
    let token1_updated = RefreshToken::find_by_hash(&pool, "token1").await.unwrap();
    let token2_updated = RefreshToken::find_by_hash(&pool, "token2").await.unwrap();
    let token3_updated = RefreshToken::find_by_hash(&pool, "token3").await.unwrap();

    assert!(token1_updated.is_revoked, "Token 1 should be revoked");
    assert!(token2_updated.is_revoked, "Token 2 should be revoked");
    assert!(token3_updated.is_revoked, "Token 3 should be revoked");

    assert_eq!(
        token1_updated.revoked_reason,
        Some("Token family compromised".to_string())
    );

    println!("✅ All tokens in family revoked");
    println!("💡 This prevents token theft attacks!");

    cleanup_user(&pool, test_email).await;
    println!("✅ Token family revocation test complete\n");
}

// ============================================================================
// SESSION MANAGEMENT TESTS
// ============================================================================

#[tokio::test]
async fn test_user_session_management() {
    let pool = setup_test_db().await;
    let test_email = "session@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("📱 Testing user session management...");

    let user = User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .unwrap();

    let token_hash = "session_token_hash_123";

    // Create session
    let session = UserSession::create(
        &pool,
        user.id,
        token_hash.to_string(),
        Some("messages:read messages:write".to_string()),
        3600, // 1 hour
    )
    .await
    .expect("Failed to create session");

    assert_eq!(session.user_id, user.id);
    assert_eq!(session.access_token_hash, token_hash);
    assert!(session.is_active);
    assert_eq!(
        session.scope,
        Some("messages:read messages:write".to_string())
    );
    println!("✅ Session created");

    // Find session by token hash
    let found_session = UserSession::find_by_token_hash(&pool, token_hash)
        .await
        .expect("Failed to find session");

    assert_eq!(found_session.id, session.id);
    println!("✅ Session lookup works");

    // Revoke session
    UserSession::revoke(&pool, session.id)
        .await
        .expect("Failed to revoke session");

    println!("✅ Session revoked");

    // Verify session cannot be found (inactive)
    let revoked_result = UserSession::find_by_token_hash(&pool, token_hash).await;
    assert!(
        revoked_result.is_err(),
        "Revoked session should not be found"
    );
    println!("✅ Revoked session not found");

    cleanup_user(&pool, test_email).await;
    println!("✅ Session management test complete\n");
}

#[tokio::test]
async fn test_revoke_all_user_sessions() {
    let pool = setup_test_db().await;
    let test_email = "revoke_all@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("🚪 Testing revoke all sessions (logout everywhere)...");

    let user = User::create(
        &pool,
        test_email.to_string(),
        "Password123".to_string(),
        None,
    )
    .await
    .unwrap();

    // Create multiple sessions
    let _session1 = UserSession::create(&pool, user.id, "token_hash_1".to_string(), None, 3600)
        .await
        .unwrap();

    let _session2 = UserSession::create(&pool, user.id, "token_hash_2".to_string(), None, 3600)
        .await
        .unwrap();

    let _session3 = UserSession::create(&pool, user.id, "token_hash_3".to_string(), None, 3600)
        .await
        .unwrap();

    println!("✅ Created 3 sessions");

    // Revoke all sessions for user
    UserSession::revoke_all_for_user(&pool, user.id)
        .await
        .expect("Failed to revoke all sessions");

    println!("✅ All sessions revoked");

    // Verify all sessions are inactive
    let result1 = UserSession::find_by_token_hash(&pool, "token_hash_1").await;
    let result2 = UserSession::find_by_token_hash(&pool, "token_hash_2").await;
    let result3 = UserSession::find_by_token_hash(&pool, "token_hash_3").await;

    assert!(result1.is_err(), "Session 1 should be inactive");
    assert!(result2.is_err(), "Session 2 should be inactive");
    assert!(result3.is_err(), "Session 3 should be inactive");

    println!("✅ All sessions confirmed inactive");

    cleanup_user(&pool, test_email).await;
    println!("✅ Revoke all sessions test complete\n");
}

// ============================================================================
// LOGIN ATTEMPT & RATE LIMITING TESTS
// ============================================================================

#[tokio::test]
async fn test_login_attempt_logging() {
    let pool = setup_test_db().await;
    let test_email = "login_attempt@vaultless.test";
    let ip: IpAddr = "192.168.1.100".parse().unwrap();

    cleanup_ip(&pool, &ip.to_string()).await;

    println!("📊 Testing login attempt logging...");

    // Log successful attempt
    LoginAttempt::log(&pool, test_email.to_string(), ip, true, None)
        .await
        .expect("Failed to log success");

    println!("✅ Logged successful login");

    // Log failed attempt
    LoginAttempt::log(
        &pool,
        test_email.to_string(),
        ip,
        false,
        Some("Invalid password".to_string()),
    )
    .await
    .expect("Failed to log failure");

    println!("✅ Logged failed login");

    // Verify attempts were logged
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_attempts WHERE email = $1")
        .bind(test_email)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(count, 2, "Should have 2 login attempts");
    println!("✅ Found {} login attempts in database", count);

    cleanup_ip(&pool, &ip.to_string()).await;
    println!("✅ Login attempt logging test complete\n");
}

#[tokio::test]
async fn test_rate_limiting() {
    let pool = setup_test_db().await;
    let test_email = "rate_limit@vaultless.test";
    let ip: IpAddr = "10.0.0.50".parse().unwrap();

    cleanup_ip(&pool, &ip.to_string()).await;

    println!("⏱️ Testing rate limiting (brute force protection)...");

    // Should not be rate limited initially
    let is_limited = LoginAttempt::is_rate_limited(&pool, ip).await.unwrap();
    assert!(!is_limited, "Should not be rate limited initially");
    println!("✅ Not rate limited initially");

    // Log 5 failed attempts
    for i in 1..=5 {
        LoginAttempt::log(
            &pool,
            test_email.to_string(),
            ip,
            false,
            Some(format!("Attempt {}", i)),
        )
        .await
        .unwrap();
    }

    println!("✅ Logged 5 failed attempts");

    // Should now be rate limited
    let is_limited_now = LoginAttempt::is_rate_limited(&pool, ip).await.unwrap();
    assert!(is_limited_now, "Should be rate limited after 5 failures");
    println!("✅ Rate limited after 5 failures");

    // Log successful attempt
    LoginAttempt::log(&pool, test_email.to_string(), ip, true, None)
        .await
        .unwrap();

    // Should still be rate limited (failures within 15 min window)
    let still_limited = LoginAttempt::is_rate_limited(&pool, ip).await.unwrap();
    assert!(still_limited, "Should still be rate limited");
    println!("✅ Still rate limited (15 min window)");

    cleanup_ip(&pool, &ip.to_string()).await;
    println!("✅ Rate limiting test complete\n");
}

#[tokio::test]
async fn test_different_ips_independent_rate_limiting() {
    let pool = setup_test_db().await;
    let test_email = "multi_ip@vaultless.test";
    let ip1: IpAddr = "192.168.1.10".parse().unwrap();
    let ip2: IpAddr = "192.168.1.20".parse().unwrap();

    cleanup_ip(&pool, &ip1.to_string()).await;
    cleanup_ip(&pool, &ip2.to_string()).await;

    println!("🌐 Testing independent rate limiting per IP...");

    // Trigger rate limit for IP1
    for _ in 0..5 {
        LoginAttempt::log(
            &pool,
            test_email.to_string(),
            ip1,
            false,
            Some("Test".to_string()),
        )
        .await
        .unwrap();
    }

    // IP1 should be limited
    assert!(LoginAttempt::is_rate_limited(&pool, ip1).await.unwrap());
    println!("✅ IP1 rate limited");

    // IP2 should NOT be limited
    assert!(!LoginAttempt::is_rate_limited(&pool, ip2).await.unwrap());
    println!("✅ IP2 not rate limited");

    cleanup_ip(&pool, &ip1.to_string()).await;
    cleanup_ip(&pool, &ip2.to_string()).await;
    println!("✅ Independent rate limiting test complete\n");
}

// ============================================================================
// SUMMARY TEST
// ============================================================================

#[tokio::test]
async fn test_complete_auth_workflow() {
    let pool = setup_test_db().await;
    let test_email = "complete_flow@vaultless.test";
    let password = "SecurePassword123!";
    let ip: IpAddr = "203.0.113.50".parse().unwrap();

    cleanup_user(&pool, test_email).await;
    cleanup_ip(&pool, &ip.to_string()).await;

    println!("\n🎯 ==============================================");
    println!("🎯 COMPLETE AUTH WORKFLOW TEST");
    println!("🎯 ==============================================\n");

    // 1. Register
    println!("1️⃣ Registering user...");
    let user = User::create(
        &pool,
        test_email.to_string(),
        password.to_string(),
        Some("Complete Test".to_string()),
    )
    .await
    .expect("Registration failed");
    println!("   ✅ User registered: {}", user.id);

    // 2. Log failed login attempt
    println!("\n2️⃣ Logging failed login attempt...");
    LoginAttempt::log(
        &pool,
        test_email.to_string(),
        ip,
        false,
        Some("Wrong password".to_string()),
    )
    .await
    .unwrap();
    println!("   ✅ Failed attempt logged");

    // 3. Log successful login
    println!("\n3️⃣ Logging successful login...");
    LoginAttempt::log(&pool, test_email.to_string(), ip, true, None)
        .await
        .unwrap();
    User::update_last_login(&pool, user.id).await.unwrap();
    println!("   ✅ Successful login logged");

    // 4. Create session
    println!("\n4️⃣ Creating user session...");
    let session = UserSession::create(&pool, user.id, "access_token_hash".to_string(), None, 3600)
        .await
        .unwrap();
    println!("   ✅ Session created: {}", session.id);

    // 5. Create refresh token
    println!("\n5️⃣ Creating refresh token...");
    let token_family = Uuid::new_v4();
    let _refresh_token = RefreshToken::create(
        &pool,
        user.id,
        "refresh_token_hash".to_string(),
        token_family,
        30,
    )
    .await
    .unwrap();
    println!("   ✅ Refresh token created");

    // 6. Verify email
    println!("\n6️⃣ Verifying email...");
    let verification_token = user.email_verification_token.clone().unwrap();
    let _verified_user = User::verify_email(&pool, &verification_token)
        .await
        .unwrap();
    println!("   ✅ Email verified");

    // 7. Request password reset
    println!("\n7️⃣ Requesting password reset...");
    let reset_token = User::request_password_reset(&pool, test_email)
        .await
        .unwrap();
    println!("   ✅ Reset token generated");

    // 8. Reset password
    println!("\n8️⃣ Resetting password...");
    let new_password = "NewPassword456!";
    User::reset_password(&pool, &reset_token, new_password.to_string())
        .await
        .unwrap();
    println!("   ✅ Password reset successful");

    // 9. Revoke all sessions (logout everywhere)
    println!("\n9️⃣ Revoking all sessions...");
    UserSession::revoke_all_for_user(&pool, user.id)
        .await
        .unwrap();
    println!("   ✅ All sessions revoked");

    // 10. Verify final state
    println!("\n🔍 Verifying final state...");
    let final_user = User::find_by_id(&pool, user.id).await.unwrap();
    assert!(final_user.email_verified, "Email should be verified");
    assert!(
        final_user.verify_password(new_password).unwrap(),
        "New password should work"
    );
    assert!(
        !final_user.verify_password(password).unwrap(),
        "Old password should not work"
    );
    println!("   ✅ All state changes verified");

    cleanup_user(&pool, test_email).await;
    cleanup_ip(&pool, &ip.to_string()).await;

    println!("\n🎯 ==============================================");
    println!("🎯 ✅ COMPLETE WORKFLOW TEST PASSED!");
    println!("🎯 ==============================================\n");
}
