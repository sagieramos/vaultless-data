use std::collections::HashMap;
use uuid::Uuid;
use vaultless_core::{
    models::pricing::{
        dto::CreatePricingPlan,
        enums::{PricingMode, SubscriptionStatus},
        pricing_plan::{Paginated, PricingPlan, PricingPlanWithAttachmentCount},
        snapshot::PricingSnapshot,
    },
    error::VaultlessError,
};
use sqlx::{PgPool, Row};

// Integration tests for PricingPlan model
// These tests require a running PostgreSQL database

struct TestSetup {
    pool: PgPool,
    developer_id: Uuid,
}

async fn setup_test() -> TestSetup {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://vaultless@localhost:5432/vaultless_db".to_string());

    let pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // Create a test developer
    let developer_id = sqlx::query_scalar!(
        "INSERT INTO users (email, password_hash, is_active, created_at)
         VALUES ($1, $2, $3, NOW())
         RETURNING id",
        format!("test_{}.example.com", Uuid::new_v4()),
        "hashed_password_placeholder",
        true
    )
    .fetch_one(&pool)
    .await
    .expect("Failed to create test developer");

    TestSetup { pool, developer_id }
}

async fn cleanup_test(setup: &TestSetup) {
    // Clean up test data
    sqlx::query!("DELETE FROM pricing_plans WHERE developer_id = $1", setup.developer_id)
        .execute(&setup.pool)
        .await
        .expect("Failed to clean up pricing plans");

    sqlx::query!("DELETE FROM users WHERE id = $1", setup.developer_id)
        .execute(&setup.pool)
        .await
        .expect("Failed to clean up test developer");
}

#[tokio::test]
async fn test_create_pricing_plan() {
    let setup = setup_test().await;

    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Test Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: Some(5000),
        price_per_proof_cents: Some(250),
        prepaid_amount_cents: None,
    };

    let plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    assert_eq!(plan.name, "Test Plan");
    assert_eq!(plan.pricing_mode, PricingMode::Postpaid);
    assert_eq!(plan.price_per_message_cents, Some(100));
    assert_eq!(plan.price_per_gb_cents, Some(5000));
    assert_eq!(plan.price_per_proof_cents, Some(250));
    assert_eq!(plan.prepaid_amount_cents, None);
    assert_eq!(plan.developer_id, setup.developer_id);

    // Verify the plan exists in the database
    let db_plan: PricingPlan = sqlx::query_as("SELECT * FROM pricing_plans WHERE id = $1")
        .bind(plan.id)
        .fetch_one(&setup.pool)
        .await
        .expect("Failed to fetch created plan");

    assert_eq!(db_plan.id, plan.id);
    assert_eq!(db_plan.name, plan.name);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_create_free_pricing_plan() {
    let setup = setup_test().await;

    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Free Plan".to_string(),
        pricing_mode: PricingMode::Free,
        price_per_message_cents: None,
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create free pricing plan");

    assert_eq!(plan.name, "Free Plan");
    assert_eq!(plan.pricing_mode, PricingMode::Free);
    assert!(plan.price_per_message_cents.is_none());
    assert!(plan.price_per_gb_cents.is_none());
    assert!(plan.price_per_proof_cents.is_none());
    assert!(plan.prepaid_amount_cents.is_none());

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_create_prepaid_pricing_plan() {
    let setup = setup_test().await;

    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Prepaid Plan".to_string(),
        pricing_mode: PricingMode::Prepaid,
        price_per_message_cents: None,
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: Some(10000), // $100 prepaid
    };

    let plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create prepaid pricing plan");

    assert_eq!(plan.name, "Prepaid Plan");
    assert_eq!(plan.pricing_mode, PricingMode::Prepaid);
    assert!(plan.price_per_message_cents.is_none());
    assert!(plan.price_per_gb_cents.is_none());
    assert!(plan.price_per_proof_cents.is_none());
    assert_eq!(plan.prepaid_amount_cents, Some(10000));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_by_id() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Find Test Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Find the plan by ID
    let found_plan = PricingPlan::find_by_id(&setup.pool, setup.developer_id, created_plan.id)
        .await
        .expect("Failed to find pricing plan by ID");

    assert_eq!(found_plan.id, created_plan.id);
    assert_eq!(found_plan.name, "Find Test Plan");
    assert_eq!(found_plan.pricing_mode, PricingMode::Postpaid);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_by_id_not_found() {
    let setup = setup_test().await;

    let non_existent_id = Uuid::new_v4();
    let result = PricingPlan::find_by_id(&setup.pool, setup.developer_id, non_existent_id)
        .await;

    assert!(matches!(result, Err(VaultlessError::NotFound(_))));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_by_wrong_developer() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Wrong Dev Test Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Try to find the plan with a wrong developer ID
    let fake_developer_id = Uuid::new_v4();
    let result = PricingPlan::find_by_id(&setup.pool, fake_developer_id, created_plan.id)
        .await;

    assert!(matches!(result, Err(VaultlessError::NotFound(_))));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_by_developer_paginated() {
    let setup = setup_test().await;

    // Create multiple plans
    for i in 1..=5 {
        let input = CreatePricingPlan {
            developer_id: setup.developer_id,
            name: format!("Paginated Test Plan {}", i),
            pricing_mode: PricingMode::Postpaid,
            price_per_message_cents: Some(100),
            price_per_gb_cents: None,
            price_per_proof_cents: None,
            prepaid_amount_cents: None,
        };

        PricingPlan::create(&setup.pool, input)
            .await
            .expect("Failed to create pricing plan");
    }

    // Find plans with pagination
    let paginated_result = PricingPlan::find_by_developer_paginated(&setup.pool, setup.developer_id, 1, 3)
        .await
        .expect("Failed to find pricing plans with pagination");

    assert_eq!(paginated_result.items.len(), 3);
    assert_eq!(paginated_result.total_count, 5);
    assert_eq!(paginated_result.page, 1);
    assert_eq!(paginated_result.page_size, 3);
    assert_eq!(paginated_result.total_pages, 2); // 5 items with page size 3 = 2 pages

    // Check that items are ordered by created_at DESC (most recent first)
    for i in 0..2 {
        assert!(paginated_result.items[i].created_at >= paginated_result.items[i + 1].created_at);
    }

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_by_developer_paginated_empty() {
    let setup = setup_test().await;

    // Find plans when none exist
    let paginated_result = PricingPlan::find_by_developer_paginated(&setup.pool, setup.developer_id, 1, 10)
        .await
        .expect("Failed to find pricing plans with pagination");

    assert_eq!(paginated_result.items.len(), 0);
    assert_eq!(paginated_result.total_count, 0);
    assert_eq!(paginated_result.total_pages, 0);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_delete_pricing_plan() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Delete Test Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Verify the plan exists
    let found_plan = PricingPlan::find_by_id(&setup.pool, setup.developer_id, created_plan.id)
        .await
        .expect("Failed to find pricing plan");
    assert_eq!(found_plan.name, "Delete Test Plan");

    // Delete the plan
    let deleted = PricingPlan::delete(&setup.pool, created_plan.id, setup.developer_id)
        .await
        .expect("Failed to delete pricing plan");

    assert!(deleted);

    // Verify the plan no longer exists
    let result = PricingPlan::find_by_id(&setup.pool, setup.developer_id, created_plan.id)
        .await;
    assert!(matches!(result, Err(VaultlessError::NotFound(_))));

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_delete_nonexistent_plan() {
    let setup = setup_test().await;

    let non_existent_id = Uuid::new_v4();
    let deleted = PricingPlan::delete(&setup.pool, non_existent_id, setup.developer_id)
        .await
        .expect("Failed to attempt deletion");

    assert!(!deleted);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_delete_plan_attached_to_application() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Attached Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Create an application
    let application_id = sqlx::query_scalar!(
        "INSERT INTO applications (developer_id, name, description, is_active, created_at) 
         VALUES ($1, $2, $3, $4, NOW()) 
         RETURNING id",
        setup.developer_id,
        "Test Application",
        "Test Description",
        true
    )
    .fetch_one(&setup.pool)
    .await
    .expect("Failed to create test application");

    // Attach the plan to the application
    sqlx::query!(
        "INSERT INTO application_pricing_plans (application_id, pricing_plan_id, is_default) 
         VALUES ($1, $2, $3)",
        application_id,
        created_plan.id,
        true
    )
    .execute(&setup.pool)
    .await
    .expect("Failed to attach plan to application");

    // Attempt to delete the plan (should fail)
    let result = PricingPlan::delete(&setup.pool, created_plan.id, setup.developer_id).await;

    assert!(matches!(result, Err(VaultlessError::BadRequest(_))));

    // Verify the plan still exists
    let found_plan = PricingPlan::find_by_id(&setup.pool, setup.developer_id, created_plan.id)
        .await
        .expect("Failed to find pricing plan");
    assert_eq!(found_plan.name, "Attached Plan");

    // Clean up
    sqlx::query!("DELETE FROM applications WHERE id = $1", application_id)
        .execute(&setup.pool)
        .await
        .expect("Failed to clean up test application");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_to_snapshot() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Snapshot Test Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: Some(5000),
        price_per_proof_cents: Some(250),
        prepaid_amount_cents: None,
    };

    let plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Create a snapshot from the plan
    let snapshot = plan.to_snapshot();

    assert_eq!(snapshot.plan_id, plan.id);
    assert_eq!(snapshot.plan_name, plan.name);
    assert_eq!(snapshot.pricing_mode, plan.pricing_mode);
    assert_eq!(snapshot.price_per_message_cents, plan.price_per_message_cents);
    assert_eq!(snapshot.price_per_gb_cents, plan.price_per_gb_cents);
    assert_eq!(snapshot.price_per_proof_cents, plan.price_per_proof_cents);
    assert_eq!(snapshot.prepaid_amount_cents, plan.prepaid_amount_cents);
    assert_eq!(snapshot.currency, Some("USD".to_string()));
    assert!(snapshot.platform_fee_percent.is_none()); // Default to None

    // Verify that the snapshot has a different ID than the plan
    assert_ne!(snapshot.id, plan.id);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_with_attachment_count() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Attachment Count Test Plan".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Create another plan
    let input2 = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Another Plan".to_string(),
        pricing_mode: PricingMode::Free,
        price_per_message_cents: None,
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan2 = PricingPlan::create(&setup.pool, input2)
        .await
        .expect("Failed to create second pricing plan");

    // Create an application
    let application_id = sqlx::query_scalar!(
        "INSERT INTO applications (developer_id, name, description, is_active, created_at) 
         VALUES ($1, $2, $3, $4, NOW()) 
         RETURNING id",
        setup.developer_id,
        "Test Application",
        "Test Description",
        true
    )
    .fetch_one(&setup.pool)
    .await
    .expect("Failed to create test application");

    // Attach the first plan to the application
    sqlx::query!(
        "INSERT INTO application_pricing_plans (application_id, pricing_plan_id, is_default) 
         VALUES ($1, $2, $3)",
        application_id,
        created_plan.id,
        true
    )
    .execute(&setup.pool)
    .await
    .expect("Failed to attach plan to application");

    // Find plans with attachment count
    let paginated_result = PricingPlan::find_with_attachment_count(
        &setup.pool,
        setup.developer_id,
        None,
        Some(1),
        Some(10)
    )
    .await
    .expect("Failed to find pricing plans with attachment count");

    assert_eq!(paginated_result.items.len(), 2);
    assert_eq!(paginated_result.total_count, 2);

    // Find the plan with attachment count
    let plan_with_count = paginated_result.items.iter()
        .find(|item| item.id == created_plan.id)
        .expect("Failed to find the first plan in results");

    assert_eq!(plan_with_count.attached_app_count, 1);

    // Find the plan without attachment count
    let plan_without_count = paginated_result.items.iter()
        .find(|item| item.id == created_plan2.id)
        .expect("Failed to find the second plan in results");

    assert_eq!(plan_without_count.attached_app_count, 0);

    // Clean up
    sqlx::query!("DELETE FROM applications WHERE id = $1", application_id)
        .execute(&setup.pool)
        .await
        .expect("Failed to clean up test application");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_with_attachment_count_single_plan() {
    let setup = setup_test().await;

    // Create a plan first
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Single Plan Attachment Count Test".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: None,
        price_per_proof_cents: None,
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Find the specific plan with attachment count
    let paginated_result = PricingPlan::find_with_attachment_count(
        &setup.pool,
        setup.developer_id,
        Some(created_plan.id),
        None,
        None
    )
    .await
    .expect("Failed to find specific pricing plan with attachment count");

    assert_eq!(paginated_result.items.len(), 1);
    assert_eq!(paginated_result.total_count, 1);
    assert_eq!(paginated_result.items[0].id, created_plan.id);
    assert_eq!(paginated_result.items[0].attached_app_count, 0);

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_owned_by_user_with_pricing_plan() {
    use vaultless_core::models::applications::{Application, dto::CreateApplication};
    use std::sync::Arc;

    let setup = setup_test().await;

    // Create an application using the proper method (this creates API keys too)
    let create_input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App With Pricing".to_string(),
        description: Some("Test Description".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created_app = Application::create(Arc::new(setup.pool.clone()), None, create_input)
        .await
        .expect("Failed to create test application");

    let application_id = created_app.application.id;

    // Create a pricing plan
    let input = CreatePricingPlan {
        developer_id: setup.developer_id,
        name: "Test Plan For App".to_string(),
        pricing_mode: PricingMode::Postpaid,
        price_per_message_cents: Some(100),
        price_per_gb_cents: Some(5000),
        price_per_proof_cents: Some(250),
        prepaid_amount_cents: None,
    };

    let created_plan = PricingPlan::create(&setup.pool, input)
        .await
        .expect("Failed to create pricing plan");

    // Attach the plan to the application
    sqlx::query!(
        "INSERT INTO application_pricing_plans (application_id, pricing_plan_id, is_default, attached_at)
         VALUES ($1, $2, $3, NOW())",
        application_id,
        created_plan.id,
        true
    )
    .execute(&setup.pool)
    .await
    .expect("Failed to attach plan to application");

    // Refresh the materialized view so our application shows up
    sqlx::query("REFRESH MATERIALIZED VIEW mv_applications_with_usage")
        .execute(&setup.pool)
        .await
        .expect("Failed to refresh materialized view");

    // Test the method with include_pricing_plan = true
    let result = Application::find_owned_by_user(
        &setup.pool,
        application_id,
        setup.developer_id,
        true, // include_pricing_plan
    )
    .await
    .expect("Failed to find application with pricing plan");

    // Verify application data
    assert_eq!(result.application.application_id, application_id);
    assert_eq!(result.application.name, "Test App With Pricing");
    assert_eq!(result.application.user_id, setup.developer_id);

    // Verify pricing plan data
    assert!(result.pricing_plan.is_some(), "Pricing plan should be attached");
    let pricing_plan = result.pricing_plan.unwrap();
    assert_eq!(pricing_plan.id, created_plan.id);
    assert_eq!(pricing_plan.name, "Test Plan For App");
    assert_eq!(pricing_plan.pricing_mode, PricingMode::Postpaid);
    assert_eq!(pricing_plan.price_per_message_cents, Some(100));
    assert_eq!(pricing_plan.price_per_gb_cents, Some(5000));
    assert_eq!(pricing_plan.price_per_proof_cents, Some(250));
    assert!(pricing_plan.is_default);

    // Clean up
    sqlx::query!("DELETE FROM applications WHERE id = $1", application_id)
        .execute(&setup.pool)
        .await
        .expect("Failed to clean up test application");

    cleanup_test(&setup).await;
}

#[tokio::test]
async fn test_find_owned_by_user_with_pricing_plan_no_plan_attached() {
    use vaultless_core::models::applications::{Application, dto::CreateApplication};
    use std::sync::Arc;

    let setup = setup_test().await;

    // Create an application without attaching a pricing plan
    let create_input = CreateApplication {
        user_id: setup.developer_id,
        name: "Test App Without Pricing".to_string(),
        description: Some("Test Description".to_string()),
        max_ttl_seconds: None,
        is_key_rotation_forced: None,
        environment: None,
    };

    let created_app = Application::create(Arc::new(setup.pool.clone()), None, create_input)
        .await
        .expect("Failed to create test application");

    let application_id = created_app.application.id;

    // Refresh the materialized view so our application shows up
    sqlx::query("REFRESH MATERIALIZED VIEW mv_applications_with_usage")
        .execute(&setup.pool)
        .await
        .expect("Failed to refresh materialized view");

    // Test the method with include_pricing_plan = true (but no plan attached)
    let result = Application::find_owned_by_user(
        &setup.pool,
        application_id,
        setup.developer_id,
        true, // include_pricing_plan
    )
    .await
    .expect("Failed to find application without pricing plan");

    // Verify application data
    assert_eq!(result.application.application_id, application_id);
    assert_eq!(result.application.name, "Test App Without Pricing");

    // Verify no pricing plan is attached
    assert!(result.pricing_plan.is_none(), "Pricing plan should not be attached");

    // Clean up
    sqlx::query!("DELETE FROM applications WHERE id = $1", application_id)
        .execute(&setup.pool)
        .await
        .expect("Failed to clean up test application");

    cleanup_test(&setup).await;
}