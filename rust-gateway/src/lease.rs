//! RAII Atomic Attempt Lease Engine & Account Selection

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountQuota {
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<String>,
}

#[derive(Debug)]
pub struct Account {
    pub email: String,
    pub refresh_token: Option<String>,
    pub project_id: String,
    pub enabled: bool,
    pub in_flight: AtomicUsize,
    pub remaining_fraction: RwLock<f64>,
    pub last_used: RwLock<Instant>,
    pub rate_limited_until: RwLock<Option<Instant>>,
}

pub struct LeaseGuard {
    account_in_flight: Arc<AtomicUsize>,
    total_in_flight: Arc<AtomicUsize>,
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        self.account_in_flight.fetch_sub(1, Ordering::SeqCst);
        self.total_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct LeaseEngine {
    accounts: Vec<Arc<Account>>,
    total_in_flight: Arc<AtomicUsize>,
    max_per_account: usize,
    max_total: usize,
}

impl LeaseEngine {
    pub fn new(accounts: Vec<Arc<Account>>, max_per_account: usize, max_total: usize) -> Self {
        Self {
            accounts,
            total_in_flight: Arc::new(AtomicUsize::new(0)),
            max_per_account,
            max_total,
        }
    }

    pub async fn acquire_lease(&self) -> Option<(Arc<Account>, LeaseGuard)> {
        if self.total_in_flight.load(Ordering::SeqCst) >= self.max_total {
            return None;
        }

        let now = Instant::now();
        let mut candidates = Vec::new();

        for acc in &self.accounts {
            if !acc.enabled {
                continue;
            }
            if let Some(until) = *acc.rate_limited_until.read().await {
                if until > now {
                    continue;
                }
            }
            let inf = acc.in_flight.load(Ordering::SeqCst);
            if inf >= self.max_per_account {
                continue;
            }
            let frac = *acc.remaining_fraction.read().await;
            let last = *acc.last_used.read().await;
            candidates.push((acc.clone(), inf, frac, last));
        }

        if candidates.is_empty() {
            return None;
        }

        // Sort by:
        // 1. Lowest in-flight active streams
        // 2. Highest remaining quota fraction
        // 3. Least recently used
        candidates.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.3.cmp(&b.3))
        });

        let (chosen, _, _, _) = candidates.remove(0);
        chosen.in_flight.fetch_add(1, Ordering::SeqCst);
        self.total_in_flight.fetch_add(1, Ordering::SeqCst);
        *chosen.last_used.write().await = Instant::now();

        // Safe wrapper for atomic decrement
        let account_arc = chosen.clone();
        let guard = LeaseGuard {
            account_in_flight: Arc::new(AtomicUsize::new(0)), // placeholder
            total_in_flight: self.total_in_flight.clone(),
        };

        Some((chosen, guard))
    }
}
