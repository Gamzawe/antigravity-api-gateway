//! Antigravity Constants & Canonical Model Definitions
#![allow(dead_code)]

use serde_json::{json, Value};

pub const DEFAULT_PORT: u16 = 8089;
pub const ANTIGRAVITY_ENDPOINT_DAILY: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
pub const ANTIGRAVITY_ENDPOINT_PROD: &str = "https://cloudcode-pa.googleapis.com";

pub const ANTIGRAVITY_ENDPOINTS: &[&str] = &[
    ANTIGRAVITY_ENDPOINT_DAILY,
    ANTIGRAVITY_ENDPOINT_PROD,
];

pub const DEFAULT_PROJECT_ID: &str = "rising-fact-p41fc";
pub const DEFAULT_COOLDOWN_SECS: u64 = 2;

#[derive(Debug, Clone)]
pub struct ModelResolution {
    pub canonical_model: &'static str,
    pub thinking_budget: Option<i64>,
}

/// Resolves client-requested model name and optional thinking level/budget
pub fn resolve_model_and_thinking(
    model: &str,
    thinking_level: Option<&str>,
    thinking_budget: Option<i64>,
) -> ModelResolution {
    let lower = model.to_lowercase();

    // 1. Check for thinking suffix or level
    // Levels: off=0, low=1024, medium=8192, high=16384, ultra/max=32768
    let mut resolved_budget: Option<i64> = thinking_budget;

    if resolved_budget.is_none() {
        if let Some(level) = thinking_level {
            resolved_budget = match level.to_lowercase().as_str() {
                "off" | "none" | "0" | "direct" => Some(0),
                "low" | "fast" | "1" => Some(1024),
                "medium" | "balanced" | "2" => Some(8192),
                "high" | "deep" | "3" => Some(16384),
                "ultra" | "max" | "maximum" | "4" => Some(32768),
                _ => None,
            };
        }
    }

    if resolved_budget.is_none() {
        if lower.ends_with(":off") || lower.ends_with(":none") || lower.ends_with(":direct") {
            resolved_budget = Some(0);
        } else if lower.ends_with(":low") || lower.ends_with(":fast") {
            resolved_budget = Some(1024);
        } else if lower.ends_with(":medium") || lower.ends_with(":balanced") {
            resolved_budget = Some(8192);
        } else if lower.ends_with(":high") || lower.ends_with(":deep") {
            resolved_budget = Some(16384);
        } else if lower.ends_with(":ultra") || lower.ends_with(":max") {
            resolved_budget = Some(32768);
        } else if lower.ends_with(":thinking") {
            resolved_budget = Some(8192);
        }
    }

    // 2. Resolve canonical model identifier
    let canonical = if lower.contains("claude-opus") || lower.contains("opus-4-6") {
        "claude-opus-4-6-thinking"
    } else if lower.contains("claude") || lower.contains("sonnet") {
        if resolved_budget.is_some() && resolved_budget.unwrap_or(0) > 8192 {
            "claude-opus-4-6-thinking"
        } else {
            "claude-sonnet-4-6"
        }
    } else if lower.contains("gemini-2.5-flash-thinking") {
        "gemini-2.5-flash-thinking"
    } else if lower.contains("gemini-2.5-flash-lite") {
        "gemini-2.5-flash-lite"
    } else if lower.contains("gemini-2.5-flash") {
        if lower.contains("thinking") {
            "gemini-2.5-flash-thinking"
        } else {
            "gemini-2.5-flash"
        }
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
    };

    ModelResolution {
        canonical_model: canonical,
        thinking_budget: resolved_budget,
    }
}

/// Backwards compatibility resolver
pub fn resolve_model(model: &str) -> &'static str {
    resolve_model_and_thinking(model, None, None).canonical_model
}

pub fn is_thinking_model(canonical: &str) -> bool {
    canonical.contains("thinking") || canonical.contains("claude") || canonical.contains("gemini-3")
}

/// Catalog of all supported models and thinking variants for discovery via /v1/models
pub fn get_models_catalog() -> Vec<Value> {
    vec![
        // Gemini 3.8 Flagship
        json!({
            "id": "gemini-3.8-flash",
            "name": "Gemini 3.8 Flash (Tiered Thinking)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking", "vision"]
        }),
        json!({
            "id": "gemini-3.8-flash:low",
            "name": "Gemini 3.8 Flash (Low Thinking - 1k)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),
        json!({
            "id": "gemini-3.8-flash:medium",
            "name": "Gemini 3.8 Flash (Medium Thinking - 8k)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),
        json!({
            "id": "gemini-3.8-flash:ultra",
            "name": "Gemini 3.8 Flash (Ultra Thinking - 32k)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),

        // Claude 3.7 & Opus
        json!({
            "id": "claude-3-7-sonnet",
            "name": "Claude 3.7 Sonnet",
            "object": "model",
            "owned_by": "anthropic",
            "capabilities": ["chat", "tools", "vision"]
        }),
        json!({
            "id": "claude-sonnet-4-6",
            "name": "Claude 3.7 Sonnet (Hybrid)",
            "object": "model",
            "owned_by": "anthropic",
            "capabilities": ["chat", "tools", "vision"]
        }),
        json!({
            "id": "claude-opus-4-6-thinking",
            "name": "Claude Opus Extended Thinking",
            "object": "model",
            "owned_by": "anthropic",
            "capabilities": ["chat", "tools", "thinking", "vision"]
        }),

        // Gemini 2.5 Workhorses
        json!({
            "id": "gemini-2.5-flash",
            "name": "Gemini 2.5 Flash (Direct / Instant)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "vision"]
        }),
        json!({
            "id": "gemini-2.5-flash:low",
            "name": "Gemini 2.5 Flash (Thinking: Low - 1k)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),
        json!({
            "id": "gemini-2.5-flash:medium",
            "name": "Gemini 2.5 Flash (Thinking: Medium - 8k)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),
        json!({
            "id": "gemini-2.5-flash:ultra",
            "name": "Gemini 2.5 Flash (Thinking: Ultra - 32k)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),
        json!({
            "id": "gemini-2.5-flash-thinking",
            "name": "Gemini 2.5 Flash Thinking (Step-by-Step)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools", "thinking"]
        }),
        json!({
            "id": "gemini-2.5-flash-lite",
            "name": "Gemini 2.5 Flash Lite",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools"]
        }),

        // Gemini 3.x Family
        json!({
            "id": "gemini-3-flash",
            "name": "Gemini 3 Flash",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools"]
        }),
        json!({
            "id": "gemini-3.5-flash-lite",
            "name": "Gemini 3.5 Flash Lite",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools"]
        }),
        json!({
            "id": "gemini-3.1-flash-lite",
            "name": "Gemini 3.1 Flash Lite",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "tools"]
        }),
        json!({
            "id": "gemini-3.1-flash-image",
            "name": "Gemini 3.1 Flash Image (Multimodal Vision)",
            "object": "model",
            "owned_by": "google",
            "capabilities": ["chat", "vision"]
        }),
    ]
}
