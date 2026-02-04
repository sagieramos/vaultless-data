use std::sync::Arc;
use uuid::Uuid;
use vaultless_core::{
    error::VaultlessError,
    models::applications::Application,
    models::applications::dto::CreateApplication,
};
use sqlx::PgPool;

// Integration tests for Application key rotation
// These tests require a running PostgreSQL database with the vaultless schema

struct TestSetup {
    pool: Arc<PgPool>,
    developer_id: Uuid,
}

async fn setup_test() -> TestSetup {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://vaultless:vaultless_dev_pass@localhost:5432/vaultless_db".to_string()
    });

    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // Create a test developer
    let developer_id = sqlx::query_scalar!(
        "INSERT INTO users (email, password_hash, is_active, created_at)
         VALUES ($1, $2, $3, NOW())
         RETURNING id",
        format!("test_key_rotation_{}@example.com", Uuid::new_v4()),
        "hashed_password_placeholder",
        true
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create test developer");

    TestSetup {
        pool: Arc::new(pool),
        developer_id,
    }
}

async fn cleanup_test(setup: &TestSetup) {
    // Clean up applications (cascade will delete api_keys)
    sqlx::query!("DELETE FROM applications WHERE developer_id = $1", setup.developer_id)
        .execute(setup.pool.as_ref())
        .await
        .expect("Failed to clean up applications");

    // Clean up test developer
    sqlx::query!("DELETE FROM users WHERE id = $1", setup.developer_id)
        .execute(setup.pool.as_ref())
        .await
        .expect("Failed to clean up test developer");
}

#[tokio::test]
async fn test_rotate_secret_key() {
    let setup = setup_test().await;

    // Create an application with keys
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Secret Key Rotation Test".to_string(),
        description: Some("Test secret key rotation".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    let original_secret_key = created.secret_key.expect("Secret key should be present");

    // Rotate the secret key
    let rotation_result = Application::rotate_secret_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to rotate secret key");

    // Verify rotation result
    assert_eq!(rotation_result.application_id, app_id);
    assert!(rotation_result.new_secret_key.starts_with("sk_live_"));
    assert_ne!(rotation_result.new_secret_key, original_secret_key);
    assert_eq!(rotation_result.key_prefix.len(), 8);

    // Verify old key is deactivated
    let old_key_active: Option<(bool,)> = sqlx::query_as(
        "SELECT is_active FROM api_keys WHERE id = $1"
    )
    .bind(rotation_result.old_key_id)
    .fetch_optional(setup.pool.as_ref())
    .await
    .expect("Failed to query old key");

    assert!(old_key_active.is_some());
    assert!(!old_key_active.unwrap().0, "Old key should be deactivated");

    // Verify new key is active
    let new_key_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND key_type = 'secret'::key_type
         AND is_active = true"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count active secret keys");

    assert_eq!(new_key_count.0, 1, "Should have exactly one active secret key");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_rotate_publishable_key() {
    let setup = setup_test().await;

    // Create an application with keys
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Publishable Key Rotation Test".to_string(),
        description: Some("Test publishable key rotation".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    let original_publishable_key = created.publishable_key_plaintext.clone();

    // Rotate the publishable key
    let rotation_result = Application::rotate_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        None, // Rotate the oldest (only) publishable key
    )
    .await
    .expect("Failed to rotate publishable key");

    // Verify rotation result
    assert_eq!(rotation_result.application_id, app_id);
    assert!(rotation_result.new_publishable_key.starts_with("pk_live_"));
    assert_ne!(rotation_result.new_publishable_key, original_publishable_key);
    assert_eq!(rotation_result.key_prefix.len(), 16);

    // Verify old key is deactivated
    let old_key_active: Option<(bool,)> = sqlx::query_as(
        "SELECT is_active FROM api_keys WHERE id = $1"
    )
    .bind(rotation_result.old_key_id)
    .fetch_optional(setup.pool.as_ref())
    .await
    .expect("Failed to query old key");

    assert!(old_key_active.is_some());
    assert!(!old_key_active.unwrap().0, "Old key should be deactivated");

    // Verify new key is active
    let new_key_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND key_type = 'publishable'::key_type
         AND is_active = true"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count active publishable keys");

    assert_eq!(new_key_count.0, 1, "Should have exactly one active publishable key");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_rotate_specific_publishable_key() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Specific PK Rotation Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    let first_publishable_key = created.publishable_key_plaintext.clone();

    // Add a second publishable key
    let second_key_result = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        None,
        None,
    )
    .await
    .expect("Failed to add second publishable key");

    // Rotate the first (specific) publishable key
    let rotation_result = Application::rotate_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        Some(&first_publishable_key),
    )
    .await
    .expect("Failed to rotate specific publishable key");

    // Verify the second key is still active
    let second_key_active: (bool,) = sqlx::query_as(
        "SELECT is_active FROM api_keys WHERE publishable_key_plaintext = $1"
    )
    .bind(&second_key_result.new_publishable_key)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to check second key status");

    assert!(second_key_active.0, "Second key should still be active");

    // Verify we now have 2 active publishable keys (second + new from rotation)
    let active_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND key_type = 'publishable'::key_type
         AND is_active = true"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count active publishable keys");

    assert_eq!(active_count.0, 2, "Should have 2 active publishable keys");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_add_publishable_key() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Add PK Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Add a second publishable key
    let add_result = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        Some("live"),
        Some(5),
    )
    .await
    .expect("Failed to add publishable key");

    // Verify result
    assert_eq!(add_result.application_id, app_id);
    assert!(add_result.new_publishable_key.starts_with("pk_live_"));
    assert_eq!(add_result.key_prefix.len(), 16);
    assert_eq!(add_result.total_active_publishable_keys, 2);

    // Verify key exists in database
    let key_exists: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND publishable_key_plaintext = $2
         AND is_active = true"
    )
    .bind(app_id)
    .bind(&add_result.new_publishable_key)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to verify key existence");

    assert_eq!(key_exists.0, 1, "New publishable key should exist");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_add_publishable_key_with_test_environment() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test Env PK Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: Some("test".to_string()),
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Add a publishable key with test environment
    let add_result = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        Some("test"),
        None,
    )
    .await
    .expect("Failed to add test publishable key");

    // Verify the key has the test prefix
    assert!(add_result.new_publishable_key.starts_with("pk_test_"));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_add_publishable_key_max_limit() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Max PK Limit Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Add keys up to the limit (already has 1, add 1 more for limit of 2)
    let _second_key = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        None,
        Some(2),
    )
    .await
    .expect("Failed to add second key");

    // Try to add one more key (should fail)
    let result = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        None,
        Some(2),
    )
    .await;

    assert!(result.is_err(), "Should fail when max limit is reached");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_add_publishable_key_invalid_environment() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Invalid Env Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Try to add key with invalid environment (too long)
    let result = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        Some("toolong"),
        None,
    )
    .await;

    assert!(result.is_err(), "Should fail with invalid environment length");
    if let Err(VaultlessError::InvalidInput(msg)) = result {
        assert!(msg.contains("Environment must be 4 characters"));
    } else {
        panic!("Expected InvalidInput error");
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_deactivate_publishable_key() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Deactivate PK Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    let first_pk = created.publishable_key_plaintext.clone();

    // Add a second publishable key
    let second_key = Application::add_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        None,
        None,
    )
    .await
    .expect("Failed to add second publishable key");

    // Deactivate the first key
    Application::deactivate_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        &first_pk,
    )
    .await
    .expect("Failed to deactivate publishable key");

    // Verify first key is deactivated
    let first_key_active: (bool,) = sqlx::query_as(
        "SELECT is_active FROM api_keys WHERE publishable_key_plaintext = $1"
    )
    .bind(&first_pk)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to check first key status");

    assert!(!first_key_active.0, "First key should be deactivated");

    // Verify second key is still active
    let second_key_active: (bool,) = sqlx::query_as(
        "SELECT is_active FROM api_keys WHERE publishable_key_plaintext = $1"
    )
    .bind(&second_key.new_publishable_key)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to check second key status");

    assert!(second_key_active.0, "Second key should still be active");

    // Verify only 1 active publishable key remains
    let active_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND key_type = 'publishable'::key_type
         AND is_active = true"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count active publishable keys");

    assert_eq!(active_count.0, 1, "Should have 1 active publishable key");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_rotate_key_unauthorized() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Unauthorized Rotation Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Try to rotate with wrong user_id
    let wrong_user_id = Uuid::new_v4();
    let result = Application::rotate_secret_key(
        setup.pool.clone(),
        None,
        app_id,
        wrong_user_id,
    )
    .await;

    assert!(result.is_err(), "Should fail with wrong user_id");
    if let Err(VaultlessError::NotFound(msg)) = result {
        assert!(msg.contains("not found") || msg.contains("access denied"));
    } else {
        panic!("Expected NotFound error for unauthorized rotation");
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_rotate_nonexistent_application() {
    let setup = setup_test().await;

    // Try to rotate key for non-existent application
    let fake_app_id = Uuid::new_v4();
    let result = Application::rotate_secret_key(
        setup.pool.clone(),
        None,
        fake_app_id,
        setup.developer_id,
    )
    .await;

    assert!(result.is_err(), "Should fail for non-existent application");
    if let Err(VaultlessError::NotFound(msg)) = result {
        assert!(msg.contains("not found"));
    } else {
        panic!("Expected NotFound error");
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_deactivate_nonexistent_publishable_key() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Nonexistent PK Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Try to deactivate a non-existent key
    let result = Application::deactivate_publishable_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
        "pk_live_nonexistent123456789012345678901234567890123456789012345",
    )
    .await;

    assert!(result.is_err(), "Should fail for non-existent key");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_multiple_key_rotations() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Multiple Rotation Test".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Rotate secret key multiple times
    let first_rotation = Application::rotate_secret_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed first secret key rotation");

    let second_rotation = Application::rotate_secret_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed second secret key rotation");

    let third_rotation = Application::rotate_secret_key(
        setup.pool.clone(),
        None,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed third secret key rotation");

    // Verify all keys are different
    assert_ne!(first_rotation.new_secret_key, second_rotation.new_secret_key);
    assert_ne!(second_rotation.new_secret_key, third_rotation.new_secret_key);
    assert_ne!(first_rotation.new_secret_key, third_rotation.new_secret_key);

    // Verify only one active secret key
    let active_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND key_type = 'secret'::key_type
         AND is_active = true"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count active secret keys");

    assert_eq!(active_count.0, 1, "Should have exactly one active secret key");

    // Verify total secret keys (original + 3 rotations = 4)
    let total_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys
         WHERE application_id = $1
         AND key_type = 'secret'::key_type"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count total secret keys");

    assert_eq!(total_count.0, 4, "Should have 4 total secret keys (1 original + 3 rotations)");

    cleanup_test(&setup).await;
}
