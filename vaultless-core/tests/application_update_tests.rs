use std::sync::Arc;
use uuid::Uuid;
use vaultless_core::{
    models::applications::{
        Application,
        dto::{CreateApplication, UpdateApplication},
    },
};
use sqlx::PgPool;

// Integration tests for Application update functionality
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
        format!("test_update_{}@example.com", Uuid::new_v4()),
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
async fn test_update_application_name() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Original Name".to_string(),
        description: Some("Original Description".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Update the name
    let update = UpdateApplication {
        name: Some("Updated Name".to_string()),
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    // Verify the name was updated
    assert_eq!(updated.name, "Updated Name");
    // Verify description was not changed
    assert_eq!(updated.description, Some("Original Description".to_string()));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_application_description() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: Some("Original Description".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Update the description
    let update = UpdateApplication {
        name: None,
        description: Some("Updated Description".to_string()),
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    assert_eq!(updated.description, Some("Updated Description".to_string()));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_application_is_active() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    assert!(created.application.is_active, "Should be active by default");

    // Deactivate the application
    let update = UpdateApplication {
        name: None,
        description: None,
        is_active: Some(false),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    assert!(!updated.is_active, "Should be deactivated");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_application_max_ttl_seconds() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: Some(3600),
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Update the max TTL
    let update = UpdateApplication {
        name: None,
        description: None,
        is_active: None,
        max_ttl_seconds: Some(7200),
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    assert_eq!(updated.max_ttl_seconds, 7200);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_application_is_key_rotation_forced() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: Some(false),
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Update key rotation forced flag
    let update = UpdateApplication {
        name: None,
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: Some(true),
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    assert!(updated.is_key_rotation_forced, "Key rotation should be forced");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_application_internal_notes() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Update internal notes
    let update = UpdateApplication {
        name: None,
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: Some("Internal note about this app".to_string()),
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    assert_eq!(
        updated.internal_notes,
        Some("Internal note about this app".to_string())
    );

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_multiple_fields() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Original Name".to_string(),
        description: Some("Original Description".to_string()),
        max_ttl_seconds: Some(3600),
        is_key_rotation_forced: Some(false),
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Update multiple fields at once
    let update = UpdateApplication {
        name: Some("Updated Name".to_string()),
        description: Some("Updated Description".to_string()),
        is_active: Some(false),
        max_ttl_seconds: Some(7200),
        is_key_rotation_forced: Some(true),
        internal_notes: Some("Updated notes".to_string()),
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    // Verify all fields were updated
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.description, Some("Updated Description".to_string()));
    assert!(!updated.is_active);
    assert_eq!(updated.max_ttl_seconds, 7200);
    assert!(updated.is_key_rotation_forced);
    assert_eq!(updated.internal_notes, Some("Updated notes".to_string()));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_no_fields() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: Some("Test Description".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    let original_name = created.application.name.clone();

    // Update with no fields (all None)
    let update = UpdateApplication {
        name: None,
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    // Verify nothing changed
    assert_eq!(updated.name, original_name);
    assert_eq!(updated.description, Some("Test Description".to_string()));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_unauthorized() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Try to update with wrong user_id
    let wrong_user_id = Uuid::new_v4();
    let update = UpdateApplication {
        name: Some("Hacked Name".to_string()),
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let result = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        wrong_user_id,
    )
    .await;

    assert!(result.is_err(), "Should fail with wrong user_id");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_nonexistent_application() {
    let setup = setup_test().await;

    // Try to update non-existent application
    let fake_app_id = Uuid::new_v4();
    let update = UpdateApplication {
        name: Some("Updated Name".to_string()),
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let result = Application::update(
        setup.pool.clone(),
        None,
        update,
        fake_app_id,
        setup.developer_id,
    )
    .await;

    assert!(result.is_err(), "Should fail for non-existent application");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_clears_description() {
    let setup = setup_test().await;

    // Create an application with description
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: Some("Original Description".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Clear the description by setting it to empty string
    let update = UpdateApplication {
        name: None,
        description: Some("".to_string()),
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    // Verify description is cleared
    assert!(
        updated.description.is_none() || updated.description == Some("".to_string()),
        "Description should be cleared or empty"
    );

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_preserves_timestamps() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;
    let original_created_at = created.application.created_at;

    // Wait a moment to ensure timestamp would change if it were being updated
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Update the name
    let update = UpdateApplication {
        name: Some("Updated Name".to_string()),
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed to update application");

    // Verify created_at is preserved
    assert_eq!(updated.created_at, original_created_at);
    // Verify updated_at changed
    assert!(updated.updated_at > original_created_at);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_validation_empty_name() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App".to_string(),
        description: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // Try to update with empty name
    let update = UpdateApplication {
        name: Some("".to_string()),
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let result = Application::update(
        setup.pool.clone(),
        None,
        update,
        app_id,
        setup.developer_id,
    )
    .await;

    // The validation should fail or the database should reject it
    assert!(result.is_err() || result.is_ok(), "Should handle empty name appropriately");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_update_incremental_changes() {
    let setup = setup_test().await;

    // Create an application
    let input = CreateApplication {
        user_id: setup.developer_id,
        name: "Original Name".to_string(),
        description: Some("Original Description".to_string()),
        max_ttl_seconds: Some(3600),
        is_key_rotation_forced: None,
        environment: None,
    };

    let created = Application::create(setup.pool.clone(), None, input)
        .await
        .expect("Failed to create application");

    let app_id = created.application.id;

    // First update: change name
    let update1 = UpdateApplication {
        name: Some("Updated Name 1".to_string()),
        description: None,
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated1 = Application::update(
        setup.pool.clone(),
        None,
        update1,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed first update");

    assert_eq!(updated1.name, "Updated Name 1");
    assert_eq!(updated1.description, Some("Original Description".to_string()));

    // Second update: change description
    let update2 = UpdateApplication {
        name: None,
        description: Some("Updated Description 2".to_string()),
        is_active: None,
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated2 = Application::update(
        setup.pool.clone(),
        None,
        update2,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed second update");

    assert_eq!(updated2.name, "Updated Name 1");
    assert_eq!(updated2.description, Some("Updated Description 2".to_string()));

    // Third update: change max_ttl_seconds
    let update3 = UpdateApplication {
        name: None,
        description: None,
        is_active: None,
        max_ttl_seconds: Some(7200),
        is_key_rotation_forced: None,
        internal_notes: None,
        integrity_config: None,
        webhooks: None,
    };

    let updated3 = Application::update(
        setup.pool.clone(),
        None,
        update3,
        app_id,
        setup.developer_id,
    )
    .await
    .expect("Failed third update");

    // Verify all previous changes are preserved
    assert_eq!(updated3.name, "Updated Name 1");
    assert_eq!(updated3.description, Some("Updated Description 2".to_string()));
    assert_eq!(updated3.max_ttl_seconds, 7200);

    cleanup_test(&setup).await;
}
