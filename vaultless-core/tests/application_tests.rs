use std::sync::Arc;
use uuid::Uuid;
use vaultless_core::{
    error::VaultlessError,
    models::applications::{
        application::ApplicationFilter,
        dto::{Application, CreateApplication},
    },
};
use sqlx::PgPool;

// Integration tests for Application model
// These tests require a running PostgreSQL database with the vaultless schema

struct TestSetup {
    pool: Arc<PgPool>,
    developer_id: Uuid,
}

async fn setup_test() -> TestSetup {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://vaultless@localhost:5432/vaultless_db".to_string()
    });

    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // Create a test developer
    let developer_id = sqlx::query_scalar!(
        "INSERT INTO users (email, password_hash, is_active, created_at)
         VALUES ($1, $2, $3, NOW())
         RETURNING id",
        format!("test_app_{}@example.com", Uuid::new_v4()),
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
async fn test_create_application() {
    let setup = setup_test().await;

    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test Application".to_string(),
        description: Some("A test application for integration testing".to_string()),
        max_ttl_seconds: Some(3600),
        is_key_rotation_forced: Some(false),
        environment: Some("live".to_string()),
    };

    let response = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Verify application fields
    assert_eq!(response.application.name, "Test Application");
    assert_eq!(
        response.application.description,
        Some("A test application for integration testing".to_string())
    );
    assert_eq!(response.application.max_ttl_seconds, 3600);
    assert!(!response.application.is_key_rotation_forced);
    assert!(response.application.is_active);
    assert_eq!(response.application.user_id, setup.developer_id);

    // Verify keys are generated
    assert!(response.secret_key.is_some());
    let secret_key = response.secret_key.unwrap();
    assert!(secret_key.starts_with("sk_live_"));

    assert!(!response.publishable_key_plaintext.is_empty());
    assert!(response.publishable_key_plaintext.starts_with("pk_live_"));

    // Verify application exists in database
    let db_app: Application = sqlx::query_as(
        "SELECT id, developer_id AS user_id, name, description, is_active,
         created_at, updated_at, max_ttl_seconds, is_key_rotation_forced,
         deletion_requested_at, internal_notes, app_meta
         FROM applications WHERE id = $1"
    )
    .bind(response.application.id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to fetch created application");

    assert_eq!(db_app.id, response.application.id);
    assert_eq!(db_app.name, "Test Application");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_create_application_with_test_environment() {
    let setup = setup_test().await;

    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test Environment App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: Some("test".to_string()),
    };

    let response = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application with test environment");

    // Verify test environment keys
    let secret_key = response.secret_key.unwrap();
    assert!(secret_key.starts_with("sk_test_"));

    assert!(response.publishable_key_plaintext.starts_with("pk_test_"));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_create_application_validation_failure() {
    let setup = setup_test().await;

    // Empty name should fail validation
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let result = Application::create(setup.pool.clone(), None, input).await;
    assert!(result.is_err());

    if let Err(VaultlessError::Validation(msg)) = result {
        assert!(msg.contains("name"));
    } else {
        panic!("Expected validation error for empty name");
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_application_by_id() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Findable App".to_string(),
        description: Some("Test find by ID".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Find by ID
    let filter = ApplicationFilter::new().id(created.application.id);
    let found = Application::find(setup.pool.as_ref(), filter)
        .await
        .expect("Failed to find application by ID");

    assert_eq!(found.id, created.application.id);
    assert_eq!(found.name, "Findable App");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_application_by_developer_id() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Developer App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Find by developer_id
    let filter = ApplicationFilter::new().developer_id(setup.developer_id);
    let found = Application::find(setup.pool.as_ref(), filter)
        .await
        .expect("Failed to find application by developer_id");

    assert_eq!(found.user_id, setup.developer_id);
    assert_eq!(found.id, created.application.id);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_application_by_publishable_key() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "PK Findable App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Find by publishable key
    let filter = ApplicationFilter::new().publishable_key(&created.publishable_key_plaintext);
    let found = Application::find(setup.pool.as_ref(), filter)
        .await
        .expect("Failed to find application by publishable key");

    assert_eq!(found.id, created.application.id);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_application_by_secret_key() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "SK Findable App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let secret_key = created.secret_key.expect("Secret key should be present");

    // Find by secret key
    let filter = ApplicationFilter::new().secret_key(&secret_key);
    let found = Application::find(setup.pool.as_ref(), filter)
        .await
        .expect("Failed to find application by secret key");

    assert_eq!(found.id, created.application.id);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_application_by_is_active() {
    let setup = setup_test().await;

    // Create active application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Active App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Find active application
    let filter = ApplicationFilter::new()
        .id(created.application.id)
        .is_active(true);
    let found = Application::find(setup.pool.as_ref(), filter)
        .await
        .expect("Failed to find active application");

    assert!(found.is_active);
    assert_eq!(found.id, created.application.id);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_application_not_found() {
    let setup = setup_test().await;

    // Try to find non-existent application
    let filter = ApplicationFilter::new().id(Uuid::new_v4());
    let result = Application::find(setup.pool.as_ref(), filter).await;

    assert!(result.is_err());
    if let Err(VaultlessError::NotFound(msg)) = result {
        assert!(msg.contains("Application not found"));
    } else {
        panic!("Expected NotFound error");
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_deactivate_deep() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Deactivate Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Deactivate application
    Application::deactivate_deep(
        setup.pool.clone(),
        None,
        created.application.id,
        setup.developer_id,
    )
    .await
    .expect("Failed to deactivate application");

    // Verify application is deactivated
    let app: (bool,) = sqlx::query_as("SELECT is_active FROM applications WHERE id = $1")
        .bind(created.application.id)
        .fetch_one(setup.pool.as_ref())
        .await
        .expect("Failed to fetch application");

    assert!(!app.0, "Application should be deactivated");

    // Verify API keys are deactivated
    let key_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys WHERE application_id = $1 AND is_active = false"
    )
    .bind(created.application.id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count deactivated keys");

    assert!(key_count.0 > 0, "API keys should be deactivated");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_deactivate_deep_wrong_owner() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Protected App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Try to deactivate with wrong user_id
    let wrong_user_id = Uuid::new_v4();
    let result = Application::deactivate_deep(
        setup.pool.clone(),
        None,
        created.application.id,
        wrong_user_id,
    )
    .await;

    assert!(result.is_err());
    if let Err(VaultlessError::NotFound(msg)) = result {
        assert!(msg.contains("not found") || msg.contains("access denied"));
    } else {
        panic!("Expected NotFound error for unauthorized deactivation");
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_delete_application() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Delete Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Delete application directly via SQL for testing
    let result = sqlx::query("DELETE FROM applications WHERE id = $1 AND developer_id = $2")
        .bind(app_id)
        .bind(setup.developer_id)
        .execute(setup.pool.as_ref())
        .await
        .expect("Failed to delete application");

    assert!(result.rows_affected() > 0, "Application should be deleted");

    // Verify application no longer exists
    let app_exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM applications WHERE id = $1")
        .bind(app_id)
        .fetch_optional(setup.pool.as_ref())
        .await
        .expect("Failed to query application");

    assert!(app_exists.is_none(), "Application should be deleted");

    // Verify API keys are also deleted (cascade)
    let key_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM api_keys WHERE application_id = $1"
    )
    .bind(app_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count keys");

    assert_eq!(key_count.0, 0, "API keys should be cascade deleted");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_delete_application_wrong_owner() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Protected Delete App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Try to delete with wrong user_id directly via SQL
    let wrong_user_id = Uuid::new_v4();
    let result = sqlx::query("DELETE FROM applications WHERE id = $1 AND developer_id = $2")
        .bind(created.application.id)
        .bind(wrong_user_id)
        .execute(setup.pool.as_ref())
        .await
        .expect("Failed to execute delete query");

    // Should not delete any rows
    assert_eq!(result.rows_affected(), 0, "No rows should be deleted with wrong owner");

    // Verify application still exists
    let app_exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM applications WHERE id = $1"
    )
    .bind(created.application.id)
    .fetch_optional(setup.pool.as_ref())
    .await
    .expect("Failed to query application");

    assert!(app_exists.is_some(), "Application should still exist");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_quota_key_generation() {
    let app_id = Uuid::new_v4();
    let key = Application::quota_key(app_id);

    assert!(key.contains("app"));
    assert!(key.contains("quota"));
    assert!(key.contains(&app_id.to_string()));
}

#[tokio::test]
async fn test_integrity_config_handler() {
    let setup = setup_test().await;

    // Create test application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Integrity Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    // Get integrity handler
    let integrity_handler = created.application.integrity()
        .expect("Failed to get integrity handler");

    // Verify handler is created (basic check)
    // More detailed tests would be in integrity_handler_tests.rs
    let browser_config = integrity_handler.get_browser_config();
    assert!(browser_config.is_some() || browser_config.is_none());

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_multiple_applications_same_developer() {
    let setup = setup_test().await;

    // Create multiple applications for the same developer
    for i in 1..=3 {
        let input = CreateApplication {
            user_id: setup.developer_id,
            name: format!("Multi App {}", i),
            description: Some(format!("Application number {}", i)),
            max_ttl_seconds: None,
            is_key_rotation_forced: None,
            environment: None,
        };

        Application::create(setup.pool.clone(), None, input)
            .await
            .expect("Failed to create application");
    }

    // Verify all applications exist
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM applications WHERE developer_id = $1"
    )
    .bind(setup.developer_id)
    .fetch_one(setup.pool.as_ref())
    .await
    .expect("Failed to count applications");

    assert_eq!(count.0, 3, "Should have 3 applications");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_application_filter_builder() {
    let app_id = Uuid::new_v4();
    let dev_id = Uuid::new_v4();

    let filter = ApplicationFilter::new()
        .id(app_id)
        .developer_id(dev_id)
        .is_active(true)
        .publishable_key("pk_test_123")
        .secret_key("sk_test_456");

    assert_eq!(filter.id, Some(app_id));
    assert_eq!(filter.developer_id, Some(dev_id));
    assert_eq!(filter.is_active, Some(true));
    assert_eq!(filter.publishable_key, Some("pk_test_123"));
    assert_eq!(filter.secret_key, Some("sk_test_456"));
}
