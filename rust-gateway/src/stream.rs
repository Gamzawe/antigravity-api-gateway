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
}

impl StreamContext {
    pub fn new(model: String) -> Self {
        Self {
            stream_id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            model,
            downstream_committed: Arc::new(AtomicBool::new(false)),
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

    // Google CloudCode payload schema: response.candidates[0].content.parts
    let parts_opt = val
        .get("response")
        .and_then(|r| r.get("candidates"))
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.get(0))
        .and_then(|cand| cand.get("content"))
        .and_then(|cont| cont.get("parts"))
        .and_then(|p| p.as_array());

    if let Some(parts) = parts_opt {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    ctx.downstream_committed.store(true, Ordering::SeqCst);
                    let payload = json!({
                        "id": ctx.stream_id,
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": ctx.model,
                        "choices": [{
                            "index": 0,
                            "delta": { "content": text },
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
                let payload = json!({
                    "id": ctx.stream_id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": ctx.model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "tool_calls": [{
                                "index": 0,
                                "id": format!("call_{}", uuid::Uuid::new_v4().simple()),
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
