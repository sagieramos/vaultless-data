use super::ios_version::*;
use crate::error::{Result, VaultlessError};
use crate::models::app_model::attestation::dto::IosIntegrityConfig;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use webpki::{EndEntityCert, Time};
use x509_parser::asn1_rs::Tag;
use x509_parser::der_parser::parse_der;
use x509_parser::prelude::*;

use super::types::*;

// =============================================================================
// APPLE ROOT CA CERTIFICATE
// =============================================================================

const APPLE_ROOT_CA_G3: &[u8] = include_bytes!("../../../../certs/AppleRootCA-G3.cer");
const APPLE_APPATTEST_EXTENSION_OID: &str = "1.2.840.113635.100.8.2";
const IOS_CHALLENGE_KEY: &str = "ios_challenge";

// =============================================================================
// APP ATTEST RESPONSE TYPES
// =============================================================================

#[derive(Debug, Deserialize)]
struct AppAttestObject {
    #[serde(rename = "fmt")]
    format: Option<String>,
    #[serde(rename = "attStmt")]
    att_stmt: AttestationStatement,
    #[serde(rename = "authData")]
    auth_data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AttestationStatement {
    #[serde(rename = "x5c")]
    x5c: Option<Vec<String>>,
    receipt: Option<String>,
}

// =============================================================================
// CERTIFICATE CHAIN VERIFICATION
// =============================================================================

pub fn verify_apple_certificate_chain(cert_chain: &[String]) -> Result<()> {
    if cert_chain.is_empty() {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Certificate chain is empty".into(),
        ));
    }

    let trust_anchor = webpki::TrustAnchor::try_from_cert_der(APPLE_ROOT_CA_G3).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid Apple root CA: {:?}", e))
    })?;

    let leaf_cert_der = BASE64.decode(&cert_chain[0]).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid leaf certificate: {}", e))
    })?;

    let leaf_cert = EndEntityCert::try_from(&leaf_cert_der[..]).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse leaf certificate: {:?}", e))
    })?;

    let mut intermediates_der = Vec::new();
    for cert_b64 in &cert_chain[1..] {
        let cert_der = BASE64.decode(cert_b64).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid intermediate certificate: {}", e))
        })?;
        intermediates_der.push(cert_der);
    }

    let intermediates: Vec<&[u8]> = intermediates_der.iter().map(|c| c.as_slice()).collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| VaultlessError::Internal(format!("System time error: {}", e)))?;

    let time = Time::from_seconds_since_unix_epoch(now.as_secs());

    leaf_cert
        .verify_is_valid_tls_server_cert(
            &[&webpki::ECDSA_P256_SHA256],
            &webpki::TlsServerTrustAnchors(&[trust_anchor]),
            &intermediates,
            time,
        )
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!(
                "Certificate chain verification failed: {:?}",
                e
            ))
        })?;

    Ok(())
}

// =============================================================================
// APP ID VERIFICATION
// =============================================================================

pub fn verify_app_id_from_certificate(
    cert_der: &[u8],
    expected_team_id: &str,
    expected_bundle_id: &str,
) -> Result<String> {
    let (_, cert) = X509Certificate::from_der(cert_der).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Failed to parse certificate: {}", e))
    })?;

    for ext in cert.extensions() {
        let oid_str = ext.oid.to_id_string();

        if oid_str == APPLE_APPATTEST_EXTENSION_OID {
            let (_, inner_data) = parse_der(ext.value).map_err(|_| {
                VaultlessError::IntegrityCheckFailed("Failed to parse App ID extension".into())
            })?;

            if inner_data.tag() == Tag::Utf8String {
                let app_id = inner_data
                    .as_str()
                    .map_err(|_| {
                        VaultlessError::IntegrityCheckFailed(
                            "Failed to read App ID as string".into(),
                        )
                    })?
                    .to_string();

                let expected_app_id = format!("{}.{}", expected_team_id, expected_bundle_id);

                if app_id.trim() != expected_app_id {
                    return Err(VaultlessError::IntegrityCheckFailed(format!(
                        "App ID mismatch. Expected: {}, Found: {}",
                        expected_app_id, app_id
                    )));
                }

                return Ok(app_id);
            } else {
                return Err(VaultlessError::IntegrityCheckFailed(
                    "App ID extension value not UTF8 string".into(),
                ));
            }
        }
    }

    Err(VaultlessError::IntegrityCheckFailed(
        "App ID extension not found in certificate".into(),
    ))
}

// =============================================================================
// iOS APP ATTEST VERIFICATION
// =============================================================================

pub async fn verify_ios_attestation(
    token: &str,
    ios_version: &str,
    config: &IosIntegrityConfig,
) -> Result<AttestationResult> {
    let mut warnings = Vec::new();
    let mut confidence: u8 = 0;

    // ---------------------------
    // 1. Validate iOS version
    // ---------------------------
    let _version_info = if let Some(min_version) = config.min_version_code {
        match validate_ios_version_from_client(ios_version, min_version) {
            Ok(info) => {
                confidence += 20;
                info
            }
            Err(e) => {
                return Ok(AttestationResult {
                    is_valid: false,
                    device_trusted: false,
                    trust_score_percent: 0,
                    verdict: Some("IOS_VERSION_TOO_OLD".to_string()),
                    error: Some(e.to_string()),
                    warnings: Some(warnings),
                    verified_at: Utc::now(),
                    platform: Platform::IOS,
                });
            }
        }
    } else {
        confidence += 10;
        IOSVersionInfo::from_version_string(ios_version).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid iOS version format: {}", e))
        })?
    };

    // ---------------------------
    // 2. Decode token
    // ---------------------------
    let attestation_bytes = BASE64.decode(token).map_err(|e| {
        VaultlessError::IntegrityCheckFailed(format!("Invalid base64 token: {}", e))
    })?;
    confidence += 10;

    // ---------------------------
    // 3. Parse CBOR
    // ---------------------------
    #[cfg(feature = "cbor")]
    let attestation_obj: AppAttestObject = ciborium::de::from_reader(&attestation_bytes[..])
        .map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid attestation format: {}", e))
        })?;

    // ---------------------------
    // 4. Cert chain
    // ---------------------------
    let cert_chain = attestation_obj
        .att_stmt
        .x5c
        .ok_or_else(|| VaultlessError::IntegrityCheckFailed("Missing certificate chain".into()))?;

    let leaf_cert_der = BASE64
        .decode(&cert_chain[0])
        .map_err(|e| VaultlessError::IntegrityCheckFailed(format!("Invalid certificate: {}", e)))?;
    confidence += 20;

    // ---------------------------
    // 5. Verify chain
    // ---------------------------
    verify_apple_certificate_chain(&cert_chain)?;
    confidence += 20;

    // ---------------------------
    // 6. Cert hash check
    // ---------------------------
    let cert_hash = {
        let mut hasher = Sha256::new();
        hasher.update(&leaf_cert_der);
        format!("{:x}", hasher.finalize())
    };

    if !config.allowed_certificate_hashes.is_empty() {
        if !config
            .allowed_certificate_hashes
            .iter()
            .any(|h| h.eq_ignore_ascii_case(&cert_hash))
        {
            return Ok(AttestationResult {
                is_valid: false,
                device_trusted: false,
                trust_score_percent: confidence,
                verdict: Some("CERTIFICATE_HASH_MISMATCH".to_string()),
                error: Some("Unapproved certificate".into()),
                warnings: Some(warnings),
                verified_at: Utc::now(),
                platform: Platform::IOS,
            });
        }
        confidence += 10;
    }

    // ---------------------------
    // 7. Bundle ID verification
    // ---------------------------
    let verified_bundle_id = if let Some(auth_data_b64) = &attestation_obj.auth_data {
        let auth_data = BASE64.decode(auth_data_b64).map_err(|e| {
            VaultlessError::IntegrityCheckFailed(format!("Invalid authData: {}", e))
        })?;

        let rp_id_hash = &auth_data[0..32];

        let mut matched = false;
        for bundle_id in &config.allowed_bundle_ids {
            let mut hasher = Sha256::new();
            hasher.update(bundle_id.as_bytes());

            if rp_id_hash == hasher.finalize().as_slice() {
                matched = true;
                break;
            }
        }

        if !matched {
            return Ok(AttestationResult {
                is_valid: false,
                device_trusted: false,
                trust_score_percent: confidence,
                verdict: Some("BUNDLE_ID_MISMATCH".to_string()),
                error: Some("Bundle mismatch".into()),
                warnings: Some(warnings),
                verified_at: Utc::now(),
                platform: Platform::IOS,
            });
        }

        confidence += 10;
        "matched".to_string()
    } else {
        return Err(VaultlessError::IntegrityCheckFailed(
            "Missing authData".into(),
        ));
    };

    // ---------------------------
    // 8. App ID verification
    // ---------------------------
    if let Some(team_id) = &config.apple_team_id {
        verify_app_id_from_certificate(&leaf_cert_der, team_id, &verified_bundle_id)?;
        confidence += 10;
    }

    let device_trusted = !config.reject_untrusted_device;

    // Cap confidence at 100
    let confidence = confidence.min(100);

    Ok(AttestationResult {
        is_valid: true,
        device_trusted,
        trust_score_percent: confidence,
        verdict: Some("APPLE_APPATTEST_VERIFIED".to_string()),
        error: None,
        warnings: if warnings.is_empty() {
            None
        } else {
            Some(warnings)
        },
        verified_at: Utc::now(),
        platform: Platform::IOS,
    })
}

// =============================================================================
// CLIENT-SIDE IMPLEMENTATION GUIDE (Swift)
// =============================================================================

/*
iOS Client Implementation:

```swift
import DeviceCheck
import UIKit

class AttestationManager {

    // 1. Get iOS version
    func getIOSVersion() -> String {
        let version = UIDevice.current.systemVersion
        return version // e.g., "17.2.1"
    }

    // 2. Get device model
    func getDeviceModel() -> String {
        var systemInfo = utsname()
        uname(&systemInfo)
        let machineMirror = Mirror(reflecting: systemInfo.machine)
        let identifier = machineMirror.children.reduce("") { identifier, element in
            guard let value = element.value as? Int8, value != 0 else { return identifier }
            return identifier + String(UnicodeScalar(UInt8(value)))
        }
        return identifier // e.g., "iPhone15,2"
    }

    // 3. Generate attestation with version info
    func generateAttestation(challenge: Data, completion: @escaping (Result<[String: Any], Error>) -> Void) {
        let service = DCAppAttestService.shared

        guard service.isSupported else {
            completion(.failure(NSError(domain: "AppAttest", code: -1,
                userInfo: [NSLocalizedDescriptionKey: "App Attest not supported"])))
            return
        }

        // Generate key
        service.generateKey { keyId, error in
            if let error = error {
                completion(.failure(error))
                return
            }

            guard let keyId = keyId else {
                completion(.failure(NSError(domain: "AppAttest", code: -2)))
                return
            }

            // Attest key
            service.attestKey(keyId, clientDataHash: challenge) { attestation, error in
                if let error = error {
                    completion(.failure(error))
                    return
                }

                guard let attestation = attestation else {
                    completion(.failure(NSError(domain: "AppAttest", code: -3)))
                    return
                }

                // Build request payload
                let payload: [String: Any] = [
                    "attestation_token": attestation.base64EncodedString(),
                    "ios_version": self.getIOSVersion(),
                    "device_model": self.getDeviceModel(),
                    "app_version": Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "unknown"
                ]

                completion(.success(payload))
            }
        }
    }
}

// Usage:
let attestationManager = AttestationManager()
let challenge = serverChallenge // Get from your server

attestationManager.generateAttestation(challenge: challenge) { result in
    switch result {
    case .success(let payload):
        // Send payload to your server
        sendToServer(payload)

    case .failure(let error):
        print("Attestation failed: \(error)")
    }
}
    ```
*/
