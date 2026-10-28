//! Thread-Safe Account Pool, Storage Loader & Live OAuth2 Refresh

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Instant, Duration};
use tokio::sync::{RwLock, Mutex};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountFileEntry {
    pub email: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: Option<String>,
    #[serde(rename = "projectId")]
    pub project_id: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountsConfig {
    pub accounts: Vec<AccountFileEntry>,
}

#[derive(Debug)]
pub struct ManagedAccount {
    pub email: String,
    pub refresh_token: String,
    pub project_id: String,
    pub enabled: bool,
    pub in_flight: Arc<AtomicUsize>,
    pub remaining_fraction: RwLock<f64>,
    pub last_used: RwLock<Instant>,
    pub rate_limited_until: RwLock<Option<Instant>>,
    // OAuth token state
    pub access_token: RwLock<Option<String>>,
    pub token_expires_at: RwLock<Instant>,
    pub refresh_mutex: Mutex<()>,
}

pub struct AccountPool {
    accounts: Vec<Arc<ManagedAccount>>,
    client: reqwest::Client,
    total_in_flight: Arc<AtomicUsize>,
    max_per_account: usize,
    max_total: usize,
}

impl AccountPool {
    pub fn load_from_file(path: &str, max_per_account: usize, max_total: usize) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
        let config: AccountsConfig = serde_json::from_str(&content).map_err(|e| format!("Failed to parse {}: {}", path, e))?;

        let mut accounts = Vec::new();
        for entry in config.accounts {
            if let Some(rt) = entry.refresh_token {
                if !rt.trim().is_empty() {
                    accounts.push(Arc::new(ManagedAccount {
                        email: entry.email,
                        refresh_token: rt,
                        project_id: entry.project_id.unwrap_or_else(|| "aicode-consumers".into()),
                        enabled: entry.enabled,
                        in_flight: Arc::new(AtomicUsize::new(0)),
                        remaining_fraction: RwLock::new(1.0),
                        last_used: RwLock::new(Instant::now()),
                        rate_limited_until: RwLock::new(None),
                        access_token: RwLock::new(None),
                        token_expires_at: RwLock::new(Instant::now()),
                        refresh_mutex: Mutex::new(()),
                    }));
                }
            }
        }

        Ok(Self {
            accounts,
            client: reqwest::Client::builder().pool_max_idle_per_host(20).build().unwrap(),
            total_in_flight: Arc::new(AtomicUsize::new(0)),
            max_per_account,
            max_total,
        })
    }

    /// Single-flight OAuth refresh to avoid thundering herd on token expiration
    pub async fn get_token(&self, acc: &ManagedAccount) -> Result<String, String> {
        // Fast-path: read lock to check valid cached token
        {
            let token_lock = acc.access_token.read().await;
            let expires = acc.token_expires_at.read().await;
            if let Some(ref t) = *token_lock {
                if Instant::now() < *expires {
                    return Ok(t.clone());
                }
            }
        }

        // Slow-path: single-flight refresh protected by per-account Mutex
        let _guard = acc.refresh_mutex.lock().await;

        // Double check after acquiring lock
        {
            let token_lock = acc.access_token.read().await;
            let expires = acc.token_expires_at.read().await;
            if let Some(ref t) = *token_lock {
                if Instant::now() < *expires {
                    return Ok(t.clone());
                }
            }
        }

        // Call Google OAuth2 token endpoint
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID")
            .unwrap_or_default();
        let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET")
            .unwrap_or_default();

        let params = [
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", acc.refresh_token.clone()),
            ("grant_type", "refresh_token".into()),
        ];

        let resp = self.client.post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("OAuth network error: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("OAuth failed ({}): {}", status, body));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let new_token = data.get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing access_token in response".to_string())?
            .to_string();

        let expires_in_secs = data.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);
        // Safety buffer: expire 60s early
        let valid_duration = Duration::from_secs(expires_in_secs.saturating_sub(60));

        *acc.access_token.write().await = Some(new_token.clone());
        *acc.token_expires_at.write().await = Instant::now() + valid_duration;

        tracing::info!("Refreshed OAuth token for: {}", acc.email);
        Ok(new_token)
    }

    /// Acquire attempt lease
    pub async fn acquire_lease(&self) -> Option<(Arc<ManagedAccount>, RustLeaseGuard)> {
        if self.total_in_flight.load(Ordering::SeqCst) >= self.max_total {
            return None;
        }

        let now = Instant::now();
        let mut candidates = Vec::new();

        for acc in &self.accounts {
            if !acc.enabled { continue; }
            if let Some(until) = *acc.rate_limited_until.read().await {
                if until > now { continue; }
            }
            let inf = acc.in_flight.load(Ordering::SeqCst);
            if inf >= self.max_per_account { continue; }

            let frac = *acc.remaining_fraction.read().await;
            let last = *acc.last_used.read().await;
            candidates.push((acc.clone(), inf, frac, last));
        }

        if candidates.is_empty() { return None; }

        candidates.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.3.cmp(&b.3))
        });

        let (chosen, _, _, _) = candidates.remove(0);
        chosen.in_flight.fetch_add(1, Ordering::SeqCst);
        self.total_in_flight.fetch_add(1, Ordering::SeqCst);
        *chosen.last_used.write().await = Instant::now();

        let guard = RustLeaseGuard {
            account_in_flight: chosen.in_flight.clone(),
            total_in_flight: self.total_in_flight.clone(),
        };

        Some((chosen, guard))
    }

    pub fn get_account_count(&self) -> usize {
        self.accounts.len()
    }
}

pub struct RustLeaseGuard {
    account_in_flight: Arc<AtomicUsize>,
    total_in_flight: Arc<AtomicUsize>,
}

impl Drop for RustLeaseGuard {
    fn drop(&mut self) {
        self.account_in_flight.fetch_sub(1, Ordering::SeqCst);
        self.total_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}
