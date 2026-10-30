//! Main Entrypoint & Axum HTTP Service

mod constants;
mod types;
mod auth;
mod client;
mod converter;
mod storage;
mod stream;
mod console;
mod cache;

use axum::{
    routing::{get, post, delete},
    Router, Json,
    response::IntoResponse,
    extract::State,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub struct AppState {
    pub pool: Arc<storage::AccountPool>,
    pub client: Arc<client::CloudCodeClient>,
    pub key_store: Arc<auth::KeyStore>,
    pub cache: Arc<cache::ResponseCache>,
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let _ = dotenvy::from_filename("../.env");

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Initializing Antigravity API Gateway (100% Rust Native)");

    // 1. Try loading accounts from environment variable ACCOUNTS_JSON
    // 2. Otherwise load from disk
    let max_per_account: usize = std::env::var("MAX_PER_ACCOUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    let max_total: usize = std::env::var("MAX_TOTAL_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);

    let pool = if let Ok(json_env) = std::env::var("ACCOUNTS_JSON") {
        tracing::info!("Loading accounts from ACCOUNTS_JSON environment variable (limits: {}/acc, {} total)", max_per_account, max_total);
        storage::AccountPool::load_from_json_str(&json_env, max_per_account, max_total)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed parsing ACCOUNTS_JSON ({}). Starting in empty mode.", e);
                storage::AccountPool::empty(max_per_account, max_total)
            })
    } else {
        let accounts_path = if std::path::Path::new("data/accounts.json").exists() {
            "data/accounts.json"
        } else if std::path::Path::new("../data/accounts.json").exists() {
            "../data/accounts.json"
        } else {
            "accounts.json"
        };

        storage::AccountPool::load_from_file(accounts_path, max_per_account, max_total)
            .unwrap_or_else(|e| {
                tracing::warn!("Could not load accounts file ({}). Starting in safe standby mode.", e);
                storage::AccountPool::empty(max_per_account, max_total)
            })
    };

    tracing::info!("Loaded {} active Google accounts into Rust Lease Engine", pool.get_account_count());

    // Initialize KeyStore (ensure path is always set to data/console/access.json)
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string());
    let access_path_str = if std::path::Path::new("../data/console").exists() {
        "../data/console/access.json".to_string()
    } else {
        format!("{}/console/access.json", data_dir)
    };
    let key_store = Arc::new(auth::KeyStore::new(Some(access_path_str)));

    // Initialize In-Memory Cache from environment or production defaults (250MB, 4h TTL)
    let response_cache = Arc::new(cache::ResponseCache::from_env());

    let state = Arc::new(AppState {
        pool: Arc::new(pool),
        client: Arc::new(client::CloudCodeClient::new()),
        key_store,
        cache: response_cache,
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/console/api/session", get(console_session))
        .route("/console/api/keys", get(console_list_keys).post(console_create_key))
        .route("/console/api/keys/{id}", delete(console_delete_key).patch(console_update_key))
        .route("/console/api/keys/{id}/revoke", post(console_revoke_key))
        .route("/console/api/cache/clear", post(console_clear_cache))
        .route("/console/api/models", get(console_models))
        .route("/console", get(console::console_handler))
        .route("/console/", get(console::console_handler))
        .route("/console/{*path}", get(console::console_handler))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(constants::DEFAULT_PORT);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Rust Gateway listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (cache_entries, cache_size_bytes, cache_hits, cache_misses, max_capacity_bytes, ttl_secs) = state.cache.telemetry();

    let utilization_percent = if max_capacity_bytes > 0 {
        ((cache_size_bytes as f64 / max_capacity_bytes as f64) * 10000.0).round() / 100.0
    } else {
        0.0
    };

    Json(serde_json::json!({
        "status": "ok",
        "runtime": "rust",
        "accounts_configured": state.pool.get_account_count(),
        "active_in_flight": state.pool.get_total_in_flight(),
        "queue_waiting": state.pool.get_queue_waiting(),
        "max_concurrency": state.pool.get_max_total(),
        "max_queue_depth": state.pool.get_max_queue_depth(),
        "arch": "tokio-axum-native",
        "cache": {
            "entries": cache_entries,
            "weighted_size_bytes": cache_size_bytes,
            "max_capacity_bytes": max_capacity_bytes,
            "utilization_percent": utilization_percent,
            "ttl_seconds": ttl_secs,
            "hits": cache_hits,
            "misses": cache_misses,
            "hit_ratio": if cache_hits + cache_misses > 0 {
                (cache_hits as f64 / (cache_hits + cache_misses) as f64 * 100.0).round()
            } else {
                0.0
            }
        }
    }))
}

fn openai_error_response(status: axum::http::StatusCode, message: &str, err_type: &str, code: &str) -> axum::response::Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": err_type,
            "code": code
        }
    });
    (status, Json(body)).into_response()
}



async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    payload_result: Result<Json<types::ChatCompletionRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<impl IntoResponse, axum::response::Response> {
    // 1. API Key Authentication (Strict by default, require ALLOW_ANONYMOUS=true for open mode)
    let auth_header = headers.get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(|h| h.trim())
        .and_then(|h| {
            if h.to_lowercase().starts_with("bearer ") {
                Some(h[7..].trim())
            } else {
                Some(h)
            }
        })
        .or_else(|| {
            headers.get("x-api-key")
                .and_then(|h| h.to_str().ok())
                .map(|h| h.trim())
        });

    let env_key = std::env::var("API_KEY")
        .or_else(|_| std::env::var("API_KEYS"))
        .ok();

    let allow_anonymous = std::env::var("ALLOW_ANONYMOUS")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    let mut auth_error_code = "invalid_api_key";
    let mut auth_error_msg = "Provide a valid API key using Authorization: Bearer *** or x-api-key header.";

    let is_authorized = if allow_anonymous {
        true
    } else {
        match auth_header {
            Some(key) => {
                let matches_env = env_key.as_deref().map(|ek| {
                    let keys: Vec<String> = ek.split(',').map(|s| s.trim().to_string()).collect();
                    auth::verify_api_key(key, &keys)
                }).unwrap_or(false);

                if matches_env {
                    true
                } else {
                    match state.key_store.verify_key(key) {
                        auth::KeyVerification::Valid => true,
                        auth::KeyVerification::Revoked => {
                            auth_error_code = "revoked_api_key";
                            auth_error_msg = "This API key has been revoked by the administrator.";
                            false
                        }
                        auth::KeyVerification::Expired => {
                            auth_error_code = "expired_api_key";
                            auth_error_msg = "This API key has expired.";
                            false
                        }
                        auth::KeyVerification::NotFound => false,
                    }
                }
            }
            None => false,
        }
    };

    if !is_authorized {
        tracing::warn!("Rejecting unauthorized request: auth_present={}, error_code={}", auth_header.is_some(), auth_error_code);
        return Err(openai_error_response(
            axum::http::StatusCode::UNAUTHORIZED,
            auth_error_msg,
            "authentication_error",
            auth_error_code,
        ));
    }

    // 2. Body parsing and schema validation
    let Json(payload) = match payload_result {
        Ok(json) => json,
        Err(rejection) => {
            let msg = format!("Invalid request payload: {}", rejection.body_text());
            return Err(openai_error_response(
                axum::http::StatusCode::BAD_REQUEST,
                &msg,
                "invalid_request_error",
                "invalid_payload",
            ));
        }
    };

    // 3. Messages array validation
    if payload.messages.is_empty() {
        return Err(openai_error_response(
            axum::http::StatusCode::BAD_REQUEST,
            "The 'messages' array must contain at least one message.",
            "invalid_request_error",
            "empty_messages",
        ));
    }

    let has_valid_content = payload.messages.iter().any(|m| {
        let has_text = match &m.content {
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Array(arr) => !arr.is_empty(),
            _ => false,
        };
        let has_tools = m.tool_calls.as_ref()
            .and_then(|tc| tc.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);
        has_text || has_tools
    });

    if !has_valid_content {
        return Err(openai_error_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Message content cannot be empty. Please provide text or tool calls.",
            "invalid_request_error",
            "empty_content",
        ));
    }

    let requested_model = payload.model.as_deref().unwrap_or("gemini-3.8-flash");
    let header_level = headers.get("x-thinking-level").and_then(|h| h.to_str().ok());
    let header_budget = headers.get("x-thinking-budget")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok());

    let effective_level = payload.thinking_level.as_deref().or(header_level);
    let effective_budget = payload.thinking_budget.or(header_budget);

    let resolution = constants::resolve_model_and_thinking(
        requested_model,
        effective_level,
        effective_budget,
    );
    let canonical = resolution.canonical_model;
    let thinking_budget = resolution.thinking_budget;

    // --- In-Memory Intelligent Cache Lookup ---
    let cache_control_hdr = headers.get("x-cache-control")
        .or_else(|| headers.get(axum::http::header::CACHE_CONTROL))
        .and_then(|h| h.to_str().ok());

    let is_cacheable = cache::ResponseCache::is_cacheable(
        payload.temperature,
        cache_control_hdr,
        payload.stream,
    );

    let messages_val = serde_json::to_value(&payload.messages).unwrap_or(serde_json::Value::Null);
    let cache_key = if is_cacheable {
        Some(cache::ResponseCache::compute_key(
            canonical,
            &messages_val,
            payload.tools.as_ref(),
            thinking_budget,
        ))
    } else {
        None
    };

    if let Some(ref key) = cache_key {
        if let Some(entry) = state.cache.get(key).await {
            tracing::info!("Cache HIT for key: {} (model: {}, stream: {})", key, canonical, payload.stream);
            match entry.payload {
                cache::CachedPayload::Json(bytes) => {
                    let resp = axum::response::Response::builder()
                        .status(axum::http::StatusCode::OK)
                        .header(axum::http::header::CONTENT_TYPE, "application/json")
                        .header("x-cache", "HIT")
                        .header("x-cache-key", key)
                        .body(axum::body::Body::from(bytes))
                        .unwrap();
                    return Ok(resp);
                }
                cache::CachedPayload::Sse(bytes) => {
                    let resp = axum::response::Response::builder()
                        .status(axum::http::StatusCode::OK)
                        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
                        .header(axum::http::header::CACHE_CONTROL, "no-cache")
                        .header("x-cache", "HIT")
                        .header("x-cache-key", key)
                        .body(axum::body::Body::from(bytes))
                        .unwrap();
                    return Ok(resp);
                }
            }
        }
    }

    // Compute Prefix-Affinity seed: hash of the initial system prompt or first user message
    // This pins subsequent turns of an evolving agent conversation to the SAME upstream account
    // so Google's server-side KV-cache (PagedAttention) hits automatically across turns!
    let affinity_seed: Option<u64> = payload.messages.first().map(|m| {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        m.role.hash(&mut hasher);
        m.content.to_string().hash(&mut hasher);
        hasher.finish()
    });

    // Multi-account retry loop (up to 3 attempts with automatic 429 failover)
    let max_retries = 3;
    let mut last_err = String::new();

    for attempt in 0..max_retries {
        // Acquire attempt lease with prefix affinity
        let (account, _lease_guard) = match state.pool.acquire_lease_with_affinity(affinity_seed).await {
            Some(pair) => pair,
            None => {
                return Err(openai_error_response(
                    axum::http::StatusCode::TOO_MANY_REQUESTS,
                    "All accounts busy or rate-limited. Gateway queue full.",
                    "rate_limit_error",
                    "rate_limit_exceeded",
                ));
            }
        };

        // Get fresh token
        let token = match state.pool.get_token(&account).await {
            Ok(t) => t,
            Err(e) => {
                last_err = e;
                continue;
            }
        };

        let cloudcode_payload = converter::build_cloudcode_payload(
            &account.project_id,
            canonical,
            &payload.messages,
            payload.tools.as_ref(),
            thinking_budget,
        );

        tracing::info!(
            "Routing completion to account: {} (model: {}, project: {}, stream: {}, thinking_budget: {:?}, attempt: {})",
            account.email,
            canonical,
            account.project_id,
            payload.stream,
            thinking_budget,
            attempt + 1
        );

        // Upstream call
        match state.client.stream_generate(
            constants::ANTIGRAVITY_ENDPOINT_DAILY,
            &token,
            cloudcode_payload,
        ).await {
            Ok(resp) => {
                if payload.stream {
                    let stream_ctx = Arc::new(stream::StreamContext::new(canonical.to_string()));
                    let cache_opt = cache_key.map(|k| (state.cache.clone(), k));
                    let is_miss = cache_opt.is_some();
                    let sse_stream = stream::create_openai_sse_stream(resp, stream_ctx, cache_opt);

                    let mut sse_resp = axum::response::sse::Sse::new(sse_stream)
                        .keep_alive(axum::response::sse::KeepAlive::default())
                        .into_response();
                    sse_resp.headers_mut().insert("x-cache", (if is_miss { "MISS" } else { "BYPASS" }).parse().unwrap());
                    return Ok(sse_resp);
                } else {
                    let raw_text = resp.text().await.unwrap_or_default();
                    let extracted = converter::extract_non_streaming_from_sse(&raw_text);

                    let mut message = serde_json::json!({
                        "role": "assistant"
                    });
                    if let Some(content) = extracted.content {
                        message["content"] = serde_json::Value::String(content);
                    } else {
                        message["content"] = serde_json::Value::Null;
                    }
                    if let Some(reasoning) = extracted.reasoning_content {
                        message["reasoning_content"] = serde_json::Value::String(reasoning);
                    }
                    if let Some(tool_calls) = extracted.tool_calls {
                        message["tool_calls"] = serde_json::Value::Array(tool_calls);
                    }

                    let json_body = serde_json::json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion",
                        "created": chrono::Utc::now().timestamp(),
                        "model": canonical,
                        "choices": [{
                            "index": 0,
                            "message": message,
                            "finish_reason": extracted.finish_reason
                        }]
                    });

                    let body_bytes = serde_json::to_vec(&json_body).unwrap_or_default();

                    // Insert into in-memory cache if eligible
                    let is_miss = cache_key.is_some();
                    if let Some(key) = cache_key {
                        state.cache.insert(key, cache::CachedPayload::Json(bytes::Bytes::from(body_bytes.clone()))).await;
                    }

                    let resp = axum::response::Response::builder()
                        .status(axum::http::StatusCode::OK)
                        .header(axum::http::header::CONTENT_TYPE, "application/json")
                        .header("x-cache", if is_miss { "MISS" } else { "BYPASS" })
                        .body(axum::body::Body::from(body_bytes))
                        .unwrap();

                    return Ok(resp);
                }
            }
            Err(e) => {
                if e == "GOOGLE_429_RATE_LIMITED" {
                    state.pool.mark_rate_limited(&account, 15).await;
                    last_err = "Upstream Google 429: Account temporarily backed off".into();
                    continue;
                }
                last_err = e;
            }
        }
    }

    Err(openai_error_response(
        axum::http::StatusCode::BAD_GATEWAY,
        &format!("All retry attempts exhausted: {}", last_err),
        "api_error",
        "upstream_error",
    ))
}

// Console API Endpoints

#[derive(serde::Deserialize)]
struct CreateKeyReq {
    name: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<String>,
    #[serde(rename = "expiresInDays")]
    expires_in_days: Option<i64>,
}

#[derive(serde::Deserialize, Default)]
struct UpdateKeyReq {
    name: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<Option<String>>,
    #[serde(rename = "expiresInDays")]
    expires_in_days: Option<i64>,
    unrevoke: Option<bool>,
}

#[derive(serde::Deserialize)]
struct KeyActionQuery {
    permanent: Option<bool>,
    action: Option<String>,
}

async fn console_session(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let configured = true;
    let username = state.key_store.get_owner_username().unwrap_or_else(|| "Arafat".into());

    Json(serde_json::json!({
        "configured": configured,
        "authenticated": true,
        "username": username,
        "csrf": "rust-csrf-token",
        "apiBase": "/v1"
    }))
}

async fn list_models() -> impl IntoResponse {
    let models = constants::get_models_catalog();
    Json(serde_json::json!({
        "object": "list",
        "data": models
    }))
}

async fn console_models() -> impl IntoResponse {
    let models = constants::get_models_catalog();
    Json(serde_json::json!({
        "data": models
    }))
}

async fn console_list_keys(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let now = chrono::Utc::now();
    let keys: Vec<serde_json::Value> = state.key_store.list_keys().into_iter().map(|k| {
        let is_revoked = k.revoked_at.is_some();
        let is_expired = if let Some(ref exp_str) = k.expires_at {
            if let Ok(exp_dt) = chrono::DateTime::parse_from_rfc3339(exp_str) {
                now > exp_dt
            } else if let Ok(exp_date) = chrono::NaiveDate::parse_from_str(exp_str, "%Y-%m-%d") {
                now.date_naive() > exp_date
            } else {
                false
            }
        } else {
            false
        };

        let status = if is_revoked {
            "revoked"
        } else if is_expired {
            "expired"
        } else {
            "active"
        };

        serde_json::json!({
            "id": k.id,
            "name": k.name,
            "prefix": k.prefix,
            "createdAt": k.created_at,
            "lastUsedAt": k.last_used_at,
            "expiresAt": k.expires_at,
            "revokedAt": k.revoked_at,
            "status": status,
            "isRevoked": is_revoked,
            "isExpired": is_expired,
            "isActive": !is_revoked && !is_expired
        })
    }).collect();

    Json(serde_json::json!({
        "keys": keys
    }))
}

async fn console_create_key(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateKeyReq>,
) -> impl IntoResponse {
    let name = payload.name.unwrap_or_else(|| "default-key".into());

    let expires_at = if let Some(days) = payload.expires_in_days {
        if days > 0 {
            Some((chrono::Utc::now() + chrono::Duration::days(days)).to_rfc3339())
        } else {
            None
        }
    } else {
        payload.expires_at
    };

    let (record, raw_key) = state.key_store.create_key(&name, expires_at);

    Json(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "prefix": record.prefix,
        "createdAt": record.created_at,
        "lastUsedAt": record.last_used_at,
        "expiresAt": record.expires_at,
        "revokedAt": record.revoked_at,
        "key": raw_key
    }))
}

async fn console_revoke_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(record) = state.key_store.revoke_key(&id) {
        Json(serde_json::json!({
            "id": record.id,
            "name": record.name,
            "prefix": record.prefix,
            "createdAt": record.created_at,
            "lastUsedAt": record.last_used_at,
            "expiresAt": record.expires_at,
            "revokedAt": record.revoked_at
        })).into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "Key not found").into_response()
    }
}

async fn console_delete_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<KeyActionQuery>,
) -> impl IntoResponse {
    let permanent = query.permanent.unwrap_or(false) || query.action.as_deref() == Some("delete");
    if permanent {
        if let Some(record) = state.key_store.delete_key(&id) {
            Json(serde_json::json!({
                "deleted": true,
                "id": record.id,
                "name": record.name
            })).into_response()
        } else {
            (axum::http::StatusCode::NOT_FOUND, "Key not found").into_response()
        }
    } else {
        if let Some(record) = state.key_store.revoke_key(&id) {
            Json(serde_json::json!({
                "revoked": true,
                "id": record.id,
                "name": record.name,
                "revokedAt": record.revoked_at
            })).into_response()
        } else {
            (axum::http::StatusCode::NOT_FOUND, "Key not found").into_response()
        }
    }
}

async fn console_update_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(payload): Json<UpdateKeyReq>,
) -> impl IntoResponse {
    let expires_at_opt = if let Some(days) = payload.expires_in_days {
        if days > 0 {
            Some(Some((chrono::Utc::now() + chrono::Duration::days(days)).to_rfc3339()))
        } else {
            Some(None)
        }
    } else {
        payload.expires_at
    };

    let unrevoke = payload.unrevoke.unwrap_or(false);

    if let Some(record) = state.key_store.update_key(&id, payload.name, expires_at_opt, unrevoke) {
        Json(serde_json::json!({
            "id": record.id,
            "name": record.name,
            "prefix": record.prefix,
            "createdAt": record.created_at,
            "lastUsedAt": record.last_used_at,
            "expiresAt": record.expires_at,
            "revokedAt": record.revoked_at
        })).into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "Key not found").into_response()
    }
}

async fn console_clear_cache(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.cache.clear().await;
    Json(serde_json::json!({
        "success": true,
        "message": "In-Memory cache purged successfully. All entries cleared."
    }))
}
