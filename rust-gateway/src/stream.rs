//! Server-Sent Events (SSE) Stream Transformer with Framing & Commit Guards

use axum::response::sse::Event;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use eventsource_stream::Eventsource;

pub struct StreamContext {
    pub stream_id: String,
    pub model: String,
    pub downstream_committed: Arc<AtomicBool>,
    pub tool_counter: Arc<std::sync::atomic::AtomicUsize>,
}

impl StreamContext {
    pub fn new(model: String) -> Self {
        Self {
            stream_id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            model,
            downstream_committed: Arc::new(AtomicBool::new(false)),
            tool_counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}

/// Parses Google CloudCode SSE chunk and converts to OpenAI-compatible SSE Event and wire representation
pub fn process_google_sse_chunk(
    ctx: &StreamContext,
    raw_json: &str,
) -> Vec<(Event, String)> {
    let mut events = Vec::new();

    let Ok(val) = serde_json::from_str::<Value>(raw_json) else {
        return events;
    };

    // Google CloudCode payload schema: response.candidates OR candidates
    let candidates = val
        .get("response")
        .and_then(|r| r.get("candidates"))
        .or_else(|| val.get("candidates"))
        .and_then(|c| c.as_array());

    if let Some(cands) = candidates {
        for (cand_idx, cand) in cands.iter().enumerate() {
            if let Some(parts) = cand.pointer("/content/parts").and_then(|p| p.as_array()) {
                for part in parts {
                    let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);

                    if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                        if !text.is_empty() {
                            ctx.downstream_committed.store(true, Ordering::SeqCst);
                            let delta = if is_thought {
                                json!({ "reasoning_content": text })
                            } else {
                                json!({ "content": text })
                            };
                            let payload = json!({
                                "id": ctx.stream_id,
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": ctx.model,
                                "choices": [{
                                    "index": 0,
                                    "delta": delta,
                                    "finish_reason": null
                                }]
                            });
                            let data_str = payload.to_string();
                            events.push((Event::default().data(data_str.clone()), format!("data: {}\n\n", data_str)));
                        }
                    }

                    // Function call handling
                    if let Some(fc) = part.get("functionCall") {
                        ctx.downstream_committed.store(true, Ordering::SeqCst);
                        let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                        let args = fc.get("args").cloned().unwrap_or(json!({}));
                        let tool_idx = ctx.tool_counter.fetch_add(1, Ordering::SeqCst);
                        let payload = json!({
                            "id": ctx.stream_id,
                            "object": "chat.completion.chunk",
                            "created": chrono::Utc::now().timestamp(),
                            "model": ctx.model,
                            "choices": [{
                                "index": 0,
                                "delta": {
                                    "tool_calls": [{
                                        "index": tool_idx,
                                        "id": format!("call_{}_{}", cand_idx, tool_idx),
                                        "type": "function",
                                        "function": {
                                            "name": name,
                                            "arguments": args.to_string()
                                        }
                                    }]
                                },
                                "finish_reason": "tool_calls"
                            }]
                        });
                        let data_str = payload.to_string();
                        events.push((Event::default().data(data_str.clone()), format!("data: {}\n\n", data_str)));
                    }
                }
            }
        }
    }

    events
}

/// Creates a streaming response from an upstream reqwest response, optionally buffering wire bytes for caching
pub fn create_openai_sse_stream(
    response: reqwest::Response,
    ctx: Arc<StreamContext>,
    cache_opt: Option<(Arc<crate::cache::ResponseCache>, String)>,
) -> impl Stream<Item = Result<Event, axum::Error>> {
    async_stream::stream! {
        let mut event_stream = response.bytes_stream().eventsource();
        let mut tap_buffer = Vec::new();

        while let Some(item) = event_stream.next().await {
            match item {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        break;
                    }
                    let evs = process_google_sse_chunk(&ctx, &event.data);
                    for (ev, wire) in evs {
                        if cache_opt.is_some() {
                            tap_buffer.extend_from_slice(wire.as_bytes());
                        }
                        yield Ok(ev);
                    }
                }
                Err(err) => {
                    tracing::error!("Error reading upstream SSE: {:?}", err);
                    break;
                }
            }
        }

        if let Some((cache, key)) = cache_opt {
            tap_buffer.extend_from_slice(b"data: [DONE]\n\n");
            cache.insert(key, crate::cache::CachedPayload::Sse(bytes::Bytes::from(tap_buffer))).await;
        }

        yield Ok(Event::default().data("[DONE]"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_google_sse_chunk_streaming_tools_and_thoughts() {
        let ctx = StreamContext::new("antigravity-gemini-3.8-flash".to_string());
        
        let chunk_json = r#"{"response": {"candidates": [{"content": {"role": "model","parts": [{"thought": true, "text": "Analyzing the codebase..."},{"thoughtSignature": "sig123","functionCall": {"name": "search_files","args": {"pattern": "*kiosk*"}}}]}}]}}"#;

        let events = process_google_sse_chunk(&ctx, chunk_json);
        assert_eq!(events.len(), 2);

        // Event 0: Reasoning delta
        let ev0_val: Value = serde_json::from_str(&events[0].1.trim_start_matches("data:").trim()).unwrap();
        assert_eq!(ev0_val["choices"][0]["delta"]["reasoning_content"], "Analyzing the codebase...");
        assert!(ev0_val["choices"][0]["delta"]["content"].is_null());

        // Event 1: Tool call delta
        let ev1_val: Value = serde_json::from_str(&events[1].1.trim_start_matches("data:").trim()).unwrap();
        assert_eq!(ev1_val["choices"][0]["delta"]["tool_calls"][0]["function"]["name"], "search_files");
        assert_eq!(ev1_val["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
        assert_eq!(ev1_val["choices"][0]["finish_reason"], "tool_calls");
    }
}
