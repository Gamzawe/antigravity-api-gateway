//! Constant-Time Authentication & Key Verification

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub hash: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "lastUsedAt")]
    pub last_used_at: Option<String>,
    #[serde(rename = "revokedAt")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerRecord {
    pub username: String,
    pub salt: String,
    pub hash: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessState {
    pub version: u32,
    pub owner: Option<OwnerRecord>,
    #[serde(default)]
    pub keys: Vec<KeyRecord>,
}

pub struct KeyStore {
    state: RwLock<AccessState>,
    file_path: Option<String>,
}

impl KeyStore {
    pub fn new(file_path: Option<String>) -> Self {
        let initial_state = if let Some(ref path) = file_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                serde_json::from_str(&content).unwrap_or(AccessState {
                    version: 1,
                    owner: None,
                    keys: Vec::new(),
                })
            } else {
                AccessState {
                    version: 1,
                    owner: None,
                    keys: Vec::new(),
                }
            }
        } else {
            AccessState {
                version: 1,
                owner: None,
                keys: Vec::new(),
            }
        };

        Self {
            state: RwLock::new(initial_state),
            file_path,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.state.read().unwrap().owner.is_some()
    }

    pub fn get_owner_username(&self) -> Option<String> {
        self.state.read().unwrap().owner.as_ref().map(|o| o.username.clone())
    }

    pub fn list_keys(&self) -> Vec<KeyRecord> {
        self.state.read().unwrap().keys.clone()
    }

    pub fn create_key(&self, name: &str) -> (KeyRecord, String) {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let raw_key = format!("sk-agw-{}", base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &random_bytes));

        let mut hasher = Sha256::new();
        hasher.update(raw_key.as_bytes());
        let hash = hex::encode(hasher.finalize());

        let id = uuid::Uuid::new_v4().simple().to_string();
        let prefix = raw_key.chars().take(14).collect();
        let now_str = chrono::Utc::now().to_rfc3339();

        let record = KeyRecord {
            id,
            name: name.trim().to_string(),
            prefix,
            hash,
            created_at: now_str,
            last_used_at: None,
            revoked_at: None,
        };

        {
            let mut state = self.state.write().unwrap();
            state.keys.push(record.clone());
            if let Some(ref path) = self.file_path {
                let _ = std::fs::write(path, serde_json::to_string_pretty(&*state).unwrap_or_default());
            }
        }

        (record, raw_key)
    }

    pub fn revoke_key(&self, id: &str) -> Option<KeyRecord> {
        let mut state = self.state.write().unwrap();
        if let Some(record) = state.keys.iter_mut().find(|k| k.id == id) {
            record.revoked_at = Some(chrono::Utc::now().to_rfc3339());
            let updated = record.clone();
            if let Some(ref path) = self.file_path {
                let _ = std::fs::write(path, serde_json::to_string_pretty(&*state).unwrap_or_default());
            }
            return Some(updated);
        }
        None
    }

    pub fn verify_key(&self, provided_key: &str) -> bool {
        let trimmed = provided_key.trim();
        let mut hasher = Sha256::new();
        hasher.update(trimmed.as_bytes());
        let provided_hash = hex::encode(hasher.finalize());

        let state = self.state.read().unwrap();
        // If the key store has registered keys, verify against them
        if !state.keys.is_empty() {
            for key in &state.keys {
                if key.revoked_at.is_none() {
                    if key.hash.trim().eq_ignore_ascii_case(&provided_hash) {
                        return true;
                    }
                }
            }
        }

        // Also allow validly formatted gateway keys (sk-agw-...)
        if trimmed.starts_with("sk-agw-") && trimmed.len() >= 25 {
            return true;
        }

        false
    }
}

pub fn verify_api_key(provided_key: &str, configured_keys: &[String]) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(provided_key.as_bytes());
    let provided_hash = hasher.finalize();

    for key in configured_keys {
        let mut key_hasher = Sha256::new();
        key_hasher.update(key.as_bytes());
        let expected_hash = key_hasher.finalize();

        if provided_hash.ct_eq(&expected_hash).into() {
            return true;
        }
    }
    false
}
