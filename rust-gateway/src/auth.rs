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
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    #[serde(rename = "revokedAt")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyVerification {
    Valid,
    Revoked,
    Expired,
    NotFound,
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
            if let Some(parent) = std::path::Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
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

    pub fn create_key(&self, name: &str, expires_at: Option<String>) -> (KeyRecord, String) {
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
            expires_at,
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

    pub fn delete_key(&self, id: &str) -> Option<KeyRecord> {
        let mut state = self.state.write().unwrap();
        if let Some(pos) = state.keys.iter().position(|k| k.id == id) {
            let removed = state.keys.remove(pos);
            if let Some(ref path) = self.file_path {
                let _ = std::fs::write(path, serde_json::to_string_pretty(&*state).unwrap_or_default());
            }
            return Some(removed);
        }
        None
    }

    pub fn update_key(
        &self,
        id: &str,
        name: Option<String>,
        expires_at: Option<Option<String>>,
        unrevoke: bool,
    ) -> Option<KeyRecord> {
        let mut state = self.state.write().unwrap();
        if let Some(record) = state.keys.iter_mut().find(|k| k.id == id) {
            if let Some(n) = name {
                if !n.trim().is_empty() {
                    record.name = n.trim().to_string();
                }
            }
            if let Some(exp) = expires_at {
                record.expires_at = exp;
            }
            if unrevoke {
                record.revoked_at = None;
            }
            let updated = record.clone();
            if let Some(ref path) = self.file_path {
                let _ = std::fs::write(path, serde_json::to_string_pretty(&*state).unwrap_or_default());
            }
            return Some(updated);
        }
        None
    }

    pub fn verify_key(&self, provided_key: &str) -> KeyVerification {
        let trimmed = provided_key.trim();
        let mut hasher = Sha256::new();
        hasher.update(trimmed.as_bytes());
        let provided_hash = hex::encode(hasher.finalize());

        let mut state = self.state.write().unwrap();
        let now = chrono::Utc::now();

        for key in &mut state.keys {
            if key.hash.trim().eq_ignore_ascii_case(&provided_hash) {
                // 1. Check if revoked
                if key.revoked_at.is_some() {
                    return KeyVerification::Revoked;
                }

                // 2. Check if expired
                if let Some(ref exp_str) = key.expires_at {
                    if let Ok(exp_dt) = chrono::DateTime::parse_from_rfc3339(exp_str) {
                        if now > exp_dt {
                            return KeyVerification::Expired;
                        }
                    } else if let Ok(exp_date) = chrono::NaiveDate::parse_from_str(exp_str, "%Y-%m-%d") {
                        if now.date_naive() > exp_date {
                            return KeyVerification::Expired;
                        }
                    }
                }

                // 3. Valid key! Update last_used_at
                key.last_used_at = Some(now.to_rfc3339());
                return KeyVerification::Valid;
            }
        }

        KeyVerification::NotFound
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
