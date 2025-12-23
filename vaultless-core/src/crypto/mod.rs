pub mod encryption;
pub mod handshake;
pub mod hashing;
pub mod key_exchange;
pub mod keys;
pub mod signing;

pub use encryption::{
    EncryptedData, EncryptionAlgorithm, decrypt, decrypt_xchacha, encrypt, encrypt_xchacha,
    XCHACHA_NONCE_SIZE,
};
pub use hashing::{hash_content, verify_hash};
pub use key_exchange::{
    SESSION_KEY_SIZE, derive_session_key, exchange_and_derive, perform_key_exchange,
};
pub use keys::{
    DualKeypair, ExchangeKeypair, SigningKeypair, generate_api_key, generate_dual_keypair,
    generate_encryption_key, generate_exchange_keypair, generate_secure_token,
    generate_signing_keypair,
};
pub use signing::{PRIVATE_KEY_SIZE, PUBLIC_KEY_SIZE, SignedData, sign_data, verify_signature};
