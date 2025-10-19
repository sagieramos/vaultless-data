use sqlx::PgPool;
use vaultless_core::getrandom;
use vaultless_core::{
    SubscriptionTier,
    crypto::{encrypt, hash_content, sign_data, verify_signature},
    models::{
        api_key::{ApiKey, CreateApiKey},
        auth::User,
        message::{CreateMessage, Message},
        proof::{CreateProof, MessageProof},
    },
};

// ============================================================================
// TEST SETUP
// ============================================================================

async fn setup_test_db() -> PgPool {
    let database_url = std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://vaultless:vaultless_dev_pass@localhost:5432/vaultless_test".to_string()
    });

    println!("🔌 Connecting to test database: {}", database_url);

    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .expect("❌ Failed to connect to test database");

    println!("🔄 Running migrations...");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("❌ Failed to run migrations");

    println!("✅ Database setup complete\n");

    pool
}

async fn cleanup_user(pool: &PgPool, email: &str) {
    let _ = sqlx::query(
        "DELETE FROM message_proofs WHERE message_id IN (SELECT id FROM messages WHERE api_key_id IN (SELECT id FROM api_keys WHERE user_id IN (SELECT id FROM users WHERE email = $1)))"
    )
    .bind(email)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "DELETE FROM messages WHERE api_key_id IN (SELECT id FROM api_keys WHERE user_id IN (SELECT id FROM users WHERE email = $1))"
    )
    .bind(email)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "DELETE FROM api_keys WHERE user_id IN (SELECT id FROM users WHERE email = $1)",
    )
    .bind(email)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "DELETE FROM user_sessions WHERE user_id IN (SELECT id FROM users WHERE email = $1)",
    )
    .bind(email)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "DELETE FROM refresh_tokens WHERE user_id IN (SELECT id FROM users WHERE email = $1)",
    )
    .bind(email)
    .execute(pool)
    .await;

    let _ = sqlx::query("DELETE FROM users WHERE email = $1")
        .bind(email)
        .execute(pool)
        .await;
}

// ============================================================================
// COMPLETE END-TO-END TEST
// ============================================================================

#[tokio::test]
async fn test_complete_message_relay_workflow() {
    let pool = setup_test_db().await;
    let test_email = "e2e_test@vaultless.test";

    cleanup_user(&pool, test_email).await;

    println!("\n🎯 ==============================================");
    println!("🎯 END-TO-END MESSAGE RELAY TEST");
    println!("🎯 ==============================================\n");

    // STEP 1: USER REGISTRATION
    println!("1️⃣ Creating user account...");

    let user = User::create(
        &pool,
        test_email.to_string(),
        "SecurePassword123!".to_string(),
        Some("E2E Test User".to_string()),
    )
    .await
    .expect("Failed to create user");

    println!("   ✅ User created: {}", user.id);
    println!("   📧 Email: {}", user.email);

    // STEP 2: CREATE API KEY
    println!("\n2️⃣ Creating API key...");

    let test_api_key_raw = "vlt_test_e2e_key_12345678901234567890";
    let key_hash = hash_content(test_api_key_raw.as_bytes());
    let key_prefix = "vlt_test";

    let api_key = ApiKey::create(
        &pool,
        CreateApiKey {
            user_id: user.id,
            key_hash: key_hash.clone(),
            key_prefix: key_prefix.to_string(),
            tier: SubscriptionTier::Pro,
            description: Some("End-to-end integration test key".to_string()),
            scopes: Some("messages:send,proofs:write".to_string()),
            expires_at: None,
        },
    )
    .await
    .expect("Failed to create API key");

    println!("   ✅ API key created: {}", api_key.id);
    println!("   🔑 Tier: {:?}", api_key.tier);
    println!(
        "   📊 Quota: {} messages/month",
        api_key.monthly_message_quota
    );

    // STEP 3: ENCRYPT MESSAGE
    println!("\n3️⃣ Encrypting message...");

    let plaintext = b"This is a secret message from Vaultless Data E2E test!";
    let mut encryption_key = [0u8; 32];
    getrandom::fill(&mut encryption_key).expect("Failed to generate key");

    let mut decryption_key = encryption_key;
    let encrypted = encrypt(plaintext, &mut encryption_key).expect("Encryption failed");

    println!("   ✅ Message encrypted");
    println!(
        "   📦 Ciphertext length: {} bytes",
        encrypted.ciphertext.len()
    );
    println!("   🔒 Nonce length: {} bytes", encrypted.nonce.len());

    // STEP 4: CREATE CRYPTOGRAPHIC PROOF
    println!("\n4️⃣ Creating cryptographic proof...");

    let mut signing_key = [0u8; 32];
    getrandom::fill(&mut signing_key).expect("Failed to generate signing key");

    let content_hash = hash_content(plaintext);
    println!("   🔐 Content hash: {}", content_hash);

    let signed_data = sign_data(plaintext, &signing_key).expect("Signing failed");

    println!("   ✅ Message signed with Ed25519");
    println!(
        "   📝 Signature length: {} bytes",
        signed_data.signature.len()
    );
    println!(
        "   🔑 Public key length: {} bytes",
        signed_data.public_key.len()
    );

    // STEP 5: SEND MESSAGE
    println!("\n5️⃣ Sending message...");

    let recipient_id = "alice@example.com";

    let message = Message::create(
        &pool,
        CreateMessage {
            recipient_id: recipient_id.to_string(),
            ciphertext: encrypted.ciphertext.clone(),
            nonce: encrypted.nonce.clone(),
            content_type: Some("text/plain".to_string()),
            content_size_bytes: encrypted.ciphertext.len() as i32,
            api_key_id: api_key.id,
            ttl_seconds: Some(604800),
            max_access_count: Some(5),
            require_proof_verification: true,
        },
    )
    .await
    .expect("Failed to send message");

    println!("   ✅ Message sent: {}", message.id);
    println!("   👤 Recipient: {}", message.recipient_id);
    println!("   ⏰ Expires: {}", message.expires_at);

    // STEP 6: STORE PROOF
    println!("\n6️⃣ Storing cryptographic proof...");

    let proof = MessageProof::create(
        &pool,
        CreateProof {
            message_id: message.id,
            content_hash: content_hash.clone(),
            signature: signed_data.signature.clone(),
            public_key: signed_data.public_key.clone(),
            algorithm: Some("Ed25519".to_string()),
            hash_algorithm: Some("SHA-256".to_string()),
            proof_metadata: None,
        },
    )
    .await
    .expect("Failed to create proof");

    println!("   ✅ Proof stored: {}", proof.id);
    println!("   🔗 Linked to message: {}", proof.message_id);

    // STEP 7: RECEIVE MESSAGE
    println!("\n7️⃣ Receiving messages...");

    let received_messages = Message::find_by_recipient(&pool, recipient_id, 10)
        .await
        .expect("Failed to receive messages");

    assert!(!received_messages.is_empty(), "No messages received");

    let received_msg = &received_messages[0];
    println!("   ✅ Received {} message(s)", received_messages.len());
    println!("   📬 Message ID: {}", received_msg.id);
    println!("   📊 Access count: {}", received_msg.access_count);

    // STEP 8: DECRYPT MESSAGE
    println!("\n8️⃣ Decrypting message...");

    let encrypted_data = vaultless_core::crypto::EncryptedData {
        ciphertext: received_msg.ciphertext.clone(),
        nonce: received_msg.nonce.clone(),
    };

    let decrypted_plaintext = vaultless_core::crypto::decrypt(&encrypted_data, &mut decryption_key)
        .expect("Decryption failed");

    println!(
        "   ✅ Message decrypted successfully\n   📝 Plaintext: {:?}",
        String::from_utf8_lossy(&decrypted_plaintext)
    );

    assert_eq!(
        decrypted_plaintext, plaintext,
        "Decrypted message doesn't match original"
    );

    // STEP 9: VERIFY CRYPTOGRAPHIC PROOF
    println!("\n9️⃣ Verifying cryptographic proof...");

    let stored_proof = MessageProof::find_by_message_id(&pool, received_msg.id)
        .await
        .expect("Failed to find proof");

    let computed_hash = hash_content(&decrypted_plaintext);
    assert_eq!(
        computed_hash, stored_proof.content_hash,
        "Content hash mismatch"
    );

    println!("   ✅ Content hash verified");

    verify_signature(
        &decrypted_plaintext,
        &stored_proof.signature,
        &stored_proof.public_key,
    )
    .expect("Signature verification failed");

    println!("   ✅ Ed25519 signature verified\n   🎉 Cryptographic proof is VALID!");

    let verified_proof = MessageProof::mark_verified(&pool, stored_proof.id)
        .await
        .expect("Failed to mark proof as verified");

    println!(
        "   📊 Verification count: {}",
        verified_proof.verification_count
    );

    // STEP 10: CHECK USAGE METRICS
    println!("\n🔟 Checking usage metrics...");

    let message_count = Message::count_monthly(&pool, api_key.id)
        .await
        .expect("Failed to count messages");

    println!("   ✅ Messages sent this month: {}", message_count);

    let has_quota = ApiKey::check_quota(&pool, api_key.id)
        .await
        .expect("Failed to check quota");

    println!("   ✅ Quota available: {}", has_quota);

    // CLEANUP
    println!("\n🧹 Cleaning up test data...");
    cleanup_user(&pool, test_email).await;
    println!("   ✅ Test data cleaned up");

    println!("\n🎯 ==============================================");
    println!("🎯 ✅ ALL TESTS PASSED!");
    println!("🎯 ==============================================");
}
