//! Intelligent In-Memory Response Caching Engine
//! Built with moka::future::Cache, bounded to 40MB RAM with W-TinyLFU eviction.
#![allow(dead_code)]

use bytes::Bytes;
use moka::future::Cache;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum CachedPayload {
    /// Non-streaming JSON response body
    Json(Bytes),
    /// Raw Server-Sent Events byte stream
    Sse(Bytes),
}

impl CachedPayload {
    pub fn len(&self) -> usize {
        match self {
            CachedPayload::Json(b) => b.len(),
            CachedPayload::Sse(b) => b.len(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub payload: CachedPayload,
    pub created_at: std::time::Instant,
}

#[derive(Clone)]
pub struct ResponseCache {
    cache: Cache<String, CacheEntry>,
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
}

impl ResponseCache {
    pub fn new(max_memory_bytes: u64, ttl_secs: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_memory_bytes)
            .weigher(|_k: &String, v: &CacheEntry| {
                // Key overhead + struct + actual payload length
                let weight = 64 + v.payload.len() as u32;
                weight.max(1)
            })
            .time_to_live(Duration::from_secs(ttl_secs))
            .build();

        Self {
            cache,
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Computes a deterministic SHA-256 cache key from request parameters
    pub fn compute_key(
        canonical_model: &str,
        messages: &serde_json::Value,
        tools: Option<&serde_json::Value>,
        thinking_budget: Option<i64>,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(canonical_model.as_bytes());
        hasher.update(b"|");
        
        // Serialize messages deterministically
        if let Ok(serialized) = serde_json::to_vec(messages) {
            hasher.update(&serialized);
        }
        hasher.update(b"|");

        if let Some(t) = tools {
            if let Ok(serialized) = serde_json::to_vec(t) {
                hasher.update(&serialized);
            }
        }
        hasher.update(b"|");

        if let Some(budget) = thinking_budget {
            hasher.update(&budget.to_le_bytes());
        }

        hex::encode(hasher.finalize())
    }

    /// Evaluates if a request is eligible for caching based on policy and parameters
    pub fn is_cacheable(
        temperature: Option<f32>,
        cache_control_header: Option<&str>,
        _stream: bool,
    ) -> bool {
        // 1. Check client opt-out headers
        if let Some(header) = cache_control_header {
            let lower = header.to_lowercase();
            if lower.contains("no-cache") || lower.contains("no-store") || lower.contains("max-age=0") {
                return false;
            }
            if lower.contains("force-cache") {
                return true;
            }
        }

        // 2. Cardinal Rule: only cache deterministic queries by default
        // (temperature == 0.0 or <= 0.1, or omitted default)
        let temp = temperature.unwrap_or(0.0);
        temp <= 0.1
    }

    /// Lookup an entry in the cache
    pub async fn get(&self, key: &str) -> Option<CacheEntry> {
        if let Some(entry) = self.cache.get(key).await {
            self.hits.fetch_add(1, Ordering::Relaxed);
            Some(entry)
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Insert an entry into the cache (guarded by maximum 1MB per value to prevent bloat)
    pub async fn insert(&self, key: String, payload: CachedPayload) {
        if payload.len() > 1024 * 1024 {
            tracing::warn!("Response body exceeds 1MB cache limit ({}); skipping cache insertion", payload.len());
            return;
        }

        let entry = CacheEntry {
            payload,
            created_at: std::time::Instant::now(),
        };

        self.cache.insert(key, entry).await;
    }

    /// Expose cache telemetry
    pub fn telemetry(&self) -> (u64, u64, u64, u64) {
        let entry_count = self.cache.entry_count();
        let weighted_size = self.cache.weighted_size();
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        (entry_count, weighted_size, hits, misses)
    }
}
