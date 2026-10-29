//! Main Entrypoint & Axum HTTP Service

mod constants;
mod types;
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
        "active_in_flight": state.pool.get_total_in_flight(),
        "queue_waiting": state.pool.get_queue_waiting(),
        "max_concurrency": state.pool.get_max_total(),
        "max_queue_depth": state.pool.get_max_queue_depth(),
        "arch": "tokio-axum-native"
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

fn extract_clean_text_from_sse(raw_sse: &str) -> String {
    let mut full_text = String::new();
    for line in raw_sse.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data:") {
            continue;
        }
        let json_part = trimmed[5..].trim();
        if json_part.is_empty() || json_part == "[DONE]" {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_part) {
            if let Some(candidates) = val.pointer("/response/candidates").and_then(|c| c.as_array()) {
                for candidate in candidates {
                    if let Some(parts) = candidate.pointer("/content/parts").and_then(|p| p.as_array()) {
                        for part in parts {
                            if part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false) {
                                continue;
                            }
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                full_text.push_str(text);
                            }
                        }
                    }
                }
            }
        }
    }

    if full_text.is_empty() {
        if raw_sse.starts_with("data:") {
            raw_sse.trim_start_matches("data:").trim().to_string()
        } else {
            raw_sse.to_string()
        }
    } else {
        full_text
    }
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<types::ChatCompletionRequest>,
) -> Result<impl IntoResponse, axum::response::Response> {
    // 1. Edge input validation
    if payload.messages.is_empty() {
        return Err(openai_error_response(
            axum::http::StatusCode::BAD_REQUEST,
            "The 'messages' array must contain at least one message.",
            "invalid_request_error",
            "empty_messages",
        ));
    }

    // 2. API Key Authentication (Strict by default, require ALLOW_ANONYMOUS=true for open mode)
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

    let is_authorized = if allow_anonymous {
        true
    } else {
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
    };

    if !is_authorized {
        tracing::warn!("Rejecting unauthorized request: auth_present={}", auth_header.is_some());
        return Err(openai_error_response(
            axum::http::StatusCode::UNAUTHORIZED,
            "Provide a valid API key using Authorization: Bearer <key> or x-api-key header.",
            "authentication_error",
            "invalid_api_key",
        ));
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
                    let raw_text = resp.text().await.unwrap_or_default();
                    let clean_text = extract_clean_text_from_sse(&raw_text);
                    return Ok(Json(serde_json::json!({
                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                        "object": "chat.completion",
                        "created": chrono::Utc::now().timestamp(),
                        "model": canonical,
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": clean_text
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
