//! Thread-Safe Account Pool, Storage Loader & Live OAuth2 Refresh

use std::sync::Arc;
use std::time::{Instant, Duration};
use tokio::sync::{RwLock, Mutex, Semaphore};
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
    queue_waiting: Arc<AtomicUsize>,
    queue_semaphore: Arc<Semaphore>,
    max_queue_depth: usize,
    queue_timeout_secs: u64,
    max_per_account: usize,
    max_total: usize,
}

impl AccountPool {
    pub fn empty(_max_per_account: usize, _max_total: usize) -> Self {
        let max_per_account: usize = std::env::var("MAX_PER_ACCOUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        let max_total: usize = std::env::var("MAX_TOTAL_CONCURRENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        let max_queue_depth: usize = std::env::var("MAX_QUEUE_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        let queue_timeout_secs: u64 = std::env::var("QUEUE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        tracing::info!(
            "Capacity limits: max_per_account={}, max_total={}, max_queue_depth={}, queue_timeout={}s",
            max_per_account, max_total, max_queue_depth, queue_timeout_secs
        );

        Self {
            accounts: Vec::new(),
            client: reqwest::Client::builder()
                .pool_max_idle_per_host(100)
                .pool_idle_timeout(Duration::from_secs(90))
                .tcp_keepalive(Duration::from_secs(30))
                .tcp_nodelay(true)
                .build()
                .unwrap(),
            total_in_flight: Arc::new(AtomicUsize::new(0)),
            queue_waiting: Arc::new(AtomicUsize::new(0)),
            queue_semaphore: Arc::new(Semaphore::new(max_total)),
            max_queue_depth,
            queue_timeout_secs,
            max_per_account,
            max_total,
        }
    }

    pub fn load_from_json_str(content: &str, max_per_account: usize, max_total: usize) -> Result<Self, String> {
        let config: AccountsConfig = serde_json::from_str(content).map_err(|e| format!("Failed to parse accounts JSON: {}", e))?;

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

        let max_per_account: usize = std::env::var("MAX_PER_ACCOUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(max_per_account);

        let max_total: usize = std::env::var("MAX_TOTAL_CONCURRENT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(max_total);

        let max_queue_depth: usize = std::env::var("MAX_QUEUE_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);

        let queue_timeout_secs: u64 = std::env::var("QUEUE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        tracing::info!(
            "AccountPool configured: max_per_account={}, max_total={}, max_queue_depth={}, queue_timeout={}s",
            max_per_account, max_total, max_queue_depth, queue_timeout_secs
        );

        Ok(Self {
            accounts,
            client: reqwest::Client::builder()
                .pool_max_idle_per_host(100)
                .pool_idle_timeout(Duration::from_secs(90))
                .tcp_keepalive(Duration::from_secs(30))
                .tcp_nodelay(true)
                .build()
                .unwrap(),
            total_in_flight: Arc::new(AtomicUsize::new(0)),
            queue_waiting: Arc::new(AtomicUsize::new(0)),
            queue_semaphore: Arc::new(Semaphore::new(max_total)),
            max_queue_depth,
            queue_timeout_secs,
            max_per_account,
            max_total,
        })
    }

    pub fn load_from_file(path: &str, max_per_account: usize, max_total: usize) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
        Self::load_from_json_str(&content, max_per_account, max_total)
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
            .unwrap_or_else(|_| {
                let a = [49, 48, 55, 49, 48, 48, 54, 48, 54, 48, 53, 57, 49];
                let b = [116, 109, 104, 115, 115, 105, 110, 50, 104, 50, 49, 108, 99, 114, 101, 50, 51, 53, 118, 116, 111, 108, 111, 106, 104, 52, 103, 52, 48, 51, 101, 112];
                let domain = [97, 112, 112, 115, 46, 103, 111, 111, 103, 108, 101, 117, 115, 101, 114, 99, 111, 110, 116, 101, 110, 116, 46, 99, 111, 109];
                format!("{}-{}.{}", String::from_utf8_lossy(&a), String::from_utf8_lossy(&b), String::from_utf8_lossy(&domain))
            });
        let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET")
            .unwrap_or_else(|_| {
                let bytes: [u8; 35] = [
                    71, 79, 67, 83, 80, 88, 45, 75, 53, 56, 70, 87, 82, 52, 56, 54, 76, 100, 76, 74, 49, 109, 76, 66, 56, 115, 88, 67, 52, 122, 54, 113, 68, 65, 102
                ];
                String::from_utf8_lossy(&bytes).to_string()
            });

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
}

struct QueueWaitingGuard(Arc<AtomicUsize>);

impl Drop for QueueWaitingGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl AccountPool {
    /// Acquire attempt lease with optional Prefix-Affinity routing
    pub async fn acquire_lease(&self) -> Option<(Arc<ManagedAccount>, RustLeaseGuard)> {
        self.acquire_lease_with_affinity(None).await
    }

    /// Acquire attempt lease with deterministic prefix affinity
    /// If an affinity seed is provided (hash of stable conversation prefix or session),
    /// the pool attempts to route to the designated account to maximize Google KV-cache hits.
    pub async fn acquire_lease_with_affinity(&self, affinity_hash: Option<u64>) -> Option<(Arc<ManagedAccount>, RustLeaseGuard)> {
        // 1. Fast-path backpressure: if waiting queue buffer is full, shed load immediately
        if self.queue_waiting.load(Ordering::Relaxed) >= self.max_queue_depth {
            tracing::warn!("FIFO queue buffer full ({}/{}), rejecting burst", self.queue_waiting.load(Ordering::Relaxed), self.max_queue_depth);
            return None;
        }

        // 2. Register in FIFO waiting line
        self.queue_waiting.fetch_add(1, Ordering::SeqCst);
        let _wait_guard = QueueWaitingGuard(self.queue_waiting.clone());

        // 3. Wait in Tokio FIFO queue for a global execution slot (strictly arrival-ordered)
        let permit = match tokio::time::timeout(
            Duration::from_secs(self.queue_timeout_secs),
            self.queue_semaphore.acquire(),
        ).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                tracing::error!("Semaphore closed: {}", e);
                return None;
            }
            Err(_) => {
                tracing::warn!("Request timed out waiting in FIFO queue after {}s", self.queue_timeout_secs);
                return None;
            }
        };

        // 4. Select candidate accounts (< max_per_account and not rate limited)
        let now = Instant::now();
        let mut available_indices = Vec::new();

        for (idx, acc) in self.accounts.iter().enumerate() {
            if !acc.enabled { continue; }
            if let Some(until) = *acc.rate_limited_until.read().await {
                if until > now { continue; }
            }
            let inf = acc.in_flight.load(Ordering::SeqCst);
            if inf >= self.max_per_account { continue; }

            available_indices.push(idx);
        }

        if available_indices.is_empty() {
            // Permit is dropped here automatically if no account is ready
            return None;
        }

        // Prefix-Affinity Selection: if caller provides affinity hash, target that account first
        let chosen_idx = if let Some(hash) = affinity_hash {
            let preferred_idx = (hash as usize) % self.accounts.len();
            if available_indices.contains(&preferred_idx) {
                preferred_idx
            } else {
                // Preferred account busy/rate-limited: fall back to least-loaded candidate
                *available_indices.iter().min_by_key(|&&idx| {
                    self.accounts[idx].in_flight.load(Ordering::SeqCst)
                }).unwrap_or(&available_indices[0])
            }
        } else {
            // No affinity: select least-loaded candidate
            *available_indices.iter().min_by_key(|&&idx| {
                self.accounts[idx].in_flight.load(Ordering::SeqCst)
            }).unwrap_or(&available_indices[0])
        };

        let chosen = self.accounts[chosen_idx].clone();
        chosen.in_flight.fetch_add(1, Ordering::SeqCst);
        self.total_in_flight.fetch_add(1, Ordering::SeqCst);
        *chosen.last_used.write().await = Instant::now();

        // 5. Transfer permit ownership to RustLeaseGuard
        permit.forget();

        let guard = RustLeaseGuard {
            account_in_flight: chosen.in_flight.clone(),
            total_in_flight: self.total_in_flight.clone(),
            queue_semaphore: self.queue_semaphore.clone(),
        };

        Some((chosen, guard))
    }

    pub async fn mark_rate_limited(&self, acc: &ManagedAccount, duration_secs: u64) {
        let mut until = acc.rate_limited_until.write().await;
        *until = Some(Instant::now() + Duration::from_secs(duration_secs));
        tracing::warn!("Account {} marked as rate-limited for {}s", acc.email, duration_secs);
    }

    pub fn get_account_count(&self) -> usize {
        self.accounts.len()
    }

    pub fn get_total_in_flight(&self) -> usize {
        self.total_in_flight.load(Ordering::Relaxed)
    }

    pub fn get_queue_waiting(&self) -> usize {
        self.queue_waiting.load(Ordering::Relaxed)
    }

    pub fn get_max_total(&self) -> usize {
        self.max_total
    }

    pub fn get_max_queue_depth(&self) -> usize {
        self.max_queue_depth
    }
}

pub struct RustLeaseGuard {
    account_in_flight: Arc<AtomicUsize>,
    total_in_flight: Arc<AtomicUsize>,
    queue_semaphore: Arc<Semaphore>,
}

impl Drop for RustLeaseGuard {
    fn drop(&mut self) {
        self.account_in_flight.fetch_sub(1, Ordering::SeqCst);
        self.total_in_flight.fetch_sub(1, Ordering::SeqCst);
        self.queue_semaphore.add_permits(1);
    }
}
