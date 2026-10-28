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
    extract::State,
    routing::{get, post},
    response::{IntoResponse, Json},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub struct AppState {
    pub pool: storage::AccountPool,
    pub client: client::CloudCodeClient,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Initializing Antigravity API Gateway (100% Rust Native)");

    // 1. Try loading accounts from environment variable ACCOUNTS_JSON
    // 2. Otherwise load from disk
    let pool = if let Ok(json_env) = std::env::var("ACCOUNTS_JSON") {
        tracing::info!("Loading accounts from ACCOUNTS_JSON environment variable");
        storage::AccountPool::load_from_json_str(&json_env, 8, 48)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed parsing ACCOUNTS_JSON ({}). Starting in empty mode.", e);
                storage::AccountPool::empty(8, 48)
            })
    } else {
        let accounts_path = if std::path::Path::new("data/accounts.json").exists() {
            "data/accounts.json"
        } else if std::path::Path::new("../data/accounts.json").exists() {
            "../data/accounts.json"
        } else {
            "accounts.json"
        };

        storage::AccountPool::load_from_file(accounts_path, 8, 48)
            .unwrap_or_else(|e| {
                tracing::warn!("Could not load accounts file ({}). Starting in safe standby mode.", e);
                storage::AccountPool::empty(8, 48)
            })
    };

    tracing::info!("Loaded {} active Google accounts into Rust Lease Engine", pool.get_account_count());

    let state = Arc::new(AppState {
        pool,
        client: client::CloudCodeClient::new(),
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/console", get(console::console_handler))
        .route("/console/", get(console::console_handler))
        .route("/console/{*path}", get(console::console_handler))
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
    Json(payload): Json<types::ChatCompletionRequest>,
) -> Result<impl IntoResponse, (axum::http::StatusCode, String)> {
    let model = payload.model.unwrap_or_else(|| "gemini-3.8-flash".into());
    let canonical = constants::resolve_model(&model);

    // Acquire attempt lease
    let (account, _lease_guard) = state.pool.acquire_lease().await
        .ok_or((axum::http::StatusCode::TOO_MANY_REQUESTS, "All accounts busy or rate-limited".into()))?;

    // Get fresh token
    let token = state.pool.get_token(&account).await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let cloudcode_payload = converter::build_cloudcode_payload(
        &account.project_id,
        canonical,
        &payload.messages,
        payload.tools.as_ref(),
    );

    tracing::info!(
        "Routing completion to account: {} (model: {}, project: {}, stream: {})",
        account.email,
        canonical,
        account.project_id,
        payload.stream
    );

    // Upstream call
    let resp = state.client.stream_generate(
        constants::ANTIGRAVITY_ENDPOINT_DAILY,
        &token,
        cloudcode_payload,
    ).await.map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e))?;

    if payload.stream {
        let stream_ctx = Arc::new(stream::StreamContext::new(canonical.to_string()));
        let sse_stream = stream::create_openai_sse_stream(resp, stream_ctx);
        Ok(axum::response::sse::Sse::new(sse_stream).keep_alive(axum::response::sse::KeepAlive::default()).into_response())
    } else {
        let text = resp.text().await.unwrap_or_default();
        Ok(Json(serde_json::json!({
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
        })).into_response())
    }
}
