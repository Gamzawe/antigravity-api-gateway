//! Antigravity Constants & Canonical Model Definitions

pub const DEFAULT_PORT: u16 = 8089;
pub const ANTIGRAVITY_ENDPOINT_DAILY: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
pub const ANTIGRAVITY_ENDPOINT_PROD: &str = "https://cloudcode-pa.googleapis.com";

pub const ANTIGRAVITY_ENDPOINTS: &[&str] = &[
    ANTIGRAVITY_ENDPOINT_DAILY,
    ANTIGRAVITY_ENDPOINT_PROD,
];

pub const DEFAULT_PROJECT_ID: &str = "rising-fact-p41fc";
pub const DEFAULT_COOLDOWN_SECS: u64 = 2;

/// Resolves client-requested model name to canonical Google CloudCode model ID
pub fn resolve_model(model: &str) -> &'static str {
    let lower = model.to_lowercase();
    if lower.contains("claude-opus") || lower.contains("opus-4-6") {
        "claude-opus-4-6-thinking"
    } else if lower.contains("claude") || lower.contains("sonnet") {
        "claude-sonnet-4-6"
    } else if lower.contains("gemini-2.5-flash-thinking") {
        "gemini-2.5-flash-thinking"
    } else if lower.contains("gemini-2.5-flash-lite") {
        "gemini-2.5-flash-lite"
    } else if lower.contains("gemini-2.5-flash") {
        "gemini-2.5-flash"
    } else if lower.contains("gemini-3.1-flash-lite") {
        "gemini-3.1-flash-lite"
    } else if lower.contains("gemini-3.1-flash-image") {
        "gemini-3.1-flash-image"
    } else if lower.contains("gemini-3.5-flash-lite") {
        "gemini-3.5-flash-lite"
    } else if lower.contains("gemini-3-flash") {
        "gemini-3-flash"
    } else if lower.contains("gemini-3.8-flash") || lower.contains("gemini-flash") || lower.contains("gemini") {
        "gemini-3.8-flash-tiered"
    } else {
        "gemini-3.8-flash-tiered"
    }
}

pub fn is_thinking_model(canonical: &str) -> bool {
    canonical.contains("thinking") || canonical.contains("claude") || canonical.contains("gemini-3")
}
