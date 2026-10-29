//! Main Entrypoint & Axum HTTP Service

mod constants;
mod types;
mod lease;
mod auth;
mod client;
mod converter;
mod storage;
mod stream;
mod console;

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

    // Initialize KeyStore (load from data/console/access.json if exists)
    let access_path = if std::path::Path::new("data/console/access.json").exists() {
        Some("data/console/access.json".to_string())
    } else if std::path::Path::new("../data/console/access.json").exists() {
        Some("../data/console/access.json".to_string())
    } else {
        None
    };
    let key_store = Arc::new(auth::KeyStore::new(access_path));

    let state = Arc::new(AppState {
        pool: Arc::new(pool),
        client: Arc::new(client::CloudCodeClient::new()),
        key_store,
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/console/api/session", get(console_session))
        .route("/console/api/keys", get(console_list_keys).post(console_create_key))
        .route("/console/api/keys/{id}", delete(console_revoke_key))
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
    Json(serde_json::json!({
        "status": "ok",
        "runtime": "rust",
        "accounts_configured": state.pool.get_account_count(),
        "arch": "tokio-axum-native"
    }))
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<types::ChatCompletionRequest>,
) -> Result<impl IntoResponse, (axum::http::StatusCode, String)> {
    // API Key Authentication (Optional/Enforced if keys exist or API_KEY env set)
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
    let has_env_key = env_key.is_some();
    let has_store_keys = !state.key_store.list_keys().is_empty();

    let is_authorized = if has_env_key || has_store_keys {
        match auth_header {
            Some(key) => {
                let matches_env = env_key.as_deref().map(|ek| {
                    let keys: Vec<String> = ek.split(',').map(|s| s.trim().to_string()).collect();
                    auth::verify_api_key(key, &keys)
                }).unwrap_or(false);
                let matches_store = state.key_store.verify_key(key);
                matches_env || matches_store
            }
            None => false,
        }
    } else {
        true
    };

    if !is_authorized {
        tracing::warn!("Rejecting unauthorized request: auth_present={}", auth_header.is_some());
        return Err((axum::http::StatusCode::UNAUTHORIZED, "Unauthorized: Invalid or missing API key".into()));
    }

    let model = payload.model.unwrap_or_else(|| "gemini-3.8-flash".into());
    let canonical = constants::resolve_model(&model);

    // Multi-account retry loop (up to 3 attempts with automatic 429 failover)
    let max_retries = 3;
    let mut last_err = String::new();

    for attempt in 0..max_retries {
        // Acquire attempt lease
        let (account, _lease_guard) = match state.pool.acquire_lease().await {
            Some(pair) => pair,
            None => {
                return Err((axum::http::StatusCode::TOO_MANY_REQUESTS, "All accounts busy or rate-limited".into()));
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
        );

        tracing::info!(
            "Routing completion to account: {} (model: {}, project: {}, stream: {}, attempt: {})",
            account.email,
            canonical,
            account.project_id,
            payload.stream,
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
                    let sse_stream = stream::create_openai_sse_stream(resp, stream_ctx);
                    return Ok(axum::response::sse::Sse::new(sse_stream).keep_alive(axum::response::sse::KeepAlive::default()).into_response());
                } else {
                    let text = resp.text().await.unwrap_or_default();
                    return Ok(Json(serde_json::json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion",
                        "created": chrono::Utc::now().timestamp(),
                        "model": canonical,
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": text
                            },
                            "finish_reason": "stop"
                        }]
                    })).into_response());
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

    Err((axum::http::StatusCode::BAD_GATEWAY, format!("All retry attempts exhausted: {}", last_err)))
}

// Console API Endpoints

#[derive(serde::Deserialize)]
struct CreateKeyReq {
    name: Option<String>,
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

async fn console_models() -> impl IntoResponse {
    Json(serde_json::json!({
        "data": [
            {"id": "gemini-3.8-flash", "name": "Gemini 3.8 Flash"},
            {"id": "claude-sonnet-4-6", "name": "Claude 3.7 Sonnet"},
            {"id": "claude-opus-4-6-thinking", "name": "Claude 3.7 Sonnet (Thinking)"}
        ]
    }))
}

async fn console_list_keys(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let keys = state.key_store.list_keys();
    Json(serde_json::json!({
        "keys": keys
    }))
}

async fn console_create_key(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateKeyReq>,
) -> impl IntoResponse {
    let name = payload.name.unwrap_or_else(|| "default-key".into());
    let (record, raw_key) = state.key_store.create_key(&name);

    Json(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "prefix": record.prefix,
        "createdAt": record.created_at,
        "lastUsedAt": record.last_used_at,
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
            "revokedAt": record.revoked_at
        })).into_response()
    } else {
        (axum::http::StatusCode::NOT_FOUND, "Key not found").into_response()
    }
}
