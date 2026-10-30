//! Full Google CloudCode Payload Builder & Message Converter

use serde_json::{json, Value};
use crate::types::ChatMessage;

pub fn convert_messages_to_google(messages: &[ChatMessage]) -> (Option<Value>, Vec<Value>) {
    let mut system_instruction: Option<Value> = None;
    let mut contents: Vec<Value> = Vec::new();

    for msg in messages {
        let role = msg.role.as_str();

        if role == "system" {
            let text = match &msg.content {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            system_instruction = Some(json!({
                "parts": [{ "text": text }]
            }));
            continue;
        }

        let google_role = match role {
            "assistant" => "model",
            "model" => "model",
            _ => "user",
        };

        let mut parts = Vec::new();

        // Handle text content
        match &msg.content {
            Value::String(s) => {
                if !s.trim().is_empty() {
                    parts.push(json!({ "text": s }));
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        if !t.trim().is_empty() {
                            parts.push(json!({ "text": t }));
                        }
                    }
                }
            }
            _ => {}
        }

        // Handle tool calls from assistant
        if let Some(tool_calls) = &msg.tool_calls {
            if let Some(calls) = tool_calls.as_array() {
                for call in calls {
                    if let (Some(name), Some(args_val)) = (
                        call.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()),
                        call.get("function").and_then(|f| f.get("arguments")),
                    ) {
                        let parsed_args = match args_val {
                            Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(json!({})),
                            v => v.clone(),
                        };
                        parts.push(json!({
                            "functionCall": {
                                "name": name,
                                "args": parsed_args
                            }
                        }));
                    }
                }
            }
        }

        // Handle tool responses from client
        if role == "tool" {
            let name = msg.name.clone().unwrap_or_else(|| "tool_result".into());
            let content_val = match &msg.content {
                Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or_else(|_| json!({ "output": s })),
                v => v.clone(),
            };
            parts.push(json!({
                "functionResponse": {
                    "name": name,
                    "response": content_val
                }
            }));
        }

        if !parts.is_empty() {
            contents.push(json!({
                "role": google_role,
                "parts": parts
            }));
        }
    }

    (system_instruction, contents)
}

pub fn build_cloudcode_payload(
    project_id: &str,
    model: &str,
    messages: &[ChatMessage],
    tools: Option<&Value>,
    thinking_budget: Option<i64>,
) -> Value {
    let (system_instruction, contents) = convert_messages_to_google(messages);

    let mut request_obj = json!({
        "contents": contents
    });

    if let Some(sys) = system_instruction {
        request_obj["systemInstruction"] = sys;
    }

    if let Some(budget) = thinking_budget {
        if !model.starts_with("claude-") {
            request_obj["generationConfig"] = json!({
                "thinkingConfig": {
                    "thinkingBudget": budget
                }
            });
        }
    }

    if let Some(tool_defs) = tools {
        if let Some(tools_arr) = tool_defs.as_array() {
            let mut declarations = Vec::new();
            for t in tools_arr {
                if let Some(func) = t.get("function") {
                    declarations.push(func.clone());
                }
            }
            if !declarations.is_empty() {
                request_obj["tools"] = json!([{
                    "functionDeclarations": declarations
                }]);
            }
        }
    }

    json!({
        "project": project_id,
        "model": model,
        "request": request_obj,
        "userAgent": "antigravity",
        "requestId": format!("agent-{}", uuid::Uuid::new_v4())
    })
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExtractedNonStreaming {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<Value>>,
    pub finish_reason: String,
}

pub fn extract_non_streaming_from_sse(raw_sse: &str) -> ExtractedNonStreaming {
    let mut full_text = String::new();
    let mut reasoning_text = String::new();
    let mut tool_calls = Vec::new();
    let mut finish_reason = None;
    let mut tool_counter = 0usize;

    for line in raw_sse.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" || trimmed.starts_with(':') {
            continue;
        }

        // Handle lines with or without "data:" prefix
        let json_part = if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("data:") {
            trimmed[5..].trim()
        } else {
            trimmed
        };

        if json_part.is_empty() || json_part == "[DONE]" {
            continue;
        }

        if let Ok(val) = serde_json::from_str::<Value>(json_part) {
            let candidates = val
                .get("response")
                .and_then(|r| r.get("candidates"))
                .or_else(|| val.get("candidates"))
                .and_then(|c| c.as_array());

            if let Some(cands) = candidates {
                for (cand_idx, cand) in cands.iter().enumerate() {
                    if let Some(fr) = cand.get("finishReason").and_then(|r| r.as_str()) {
                        let mapped = match fr {
                            "STOP" => "stop",
                            "MAX_TOKENS" => "length",
                            "SAFETY" | "RECITATION" | "BLOCKLIST" => "content_filter",
                            _ => "stop",
                        };
                        finish_reason = Some(mapped.to_string());
                    }

                    if let Some(parts) = cand.pointer("/content/parts").and_then(|p| p.as_array()) {
                        for part in parts {
                            let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);

                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    if is_thought {
                                        reasoning_text.push_str(text);
                                    } else {
                                        full_text.push_str(text);
                                    }
                                }
                            }

                            if let Some(fc) = part.get("functionCall") {
                                let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                let args = fc.get("args").cloned().unwrap_or(json!({}));
                                let call_id = format!("call_{}_{}", cand_idx, tool_counter);
                                tool_counter += 1;

                                tool_calls.push(json!({
                                    "id": call_id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": args.to_string()
                                    }
                                }));
                            }
                        }
                    }
                }
            }
        }
    }

    let final_finish = if !tool_calls.is_empty() {
        "tool_calls".to_string()
    } else {
        finish_reason.unwrap_or_else(|| "stop".to_string())
    };

    ExtractedNonStreaming {
        content: if full_text.is_empty() { None } else { Some(full_text) },
        reasoning_content: if reasoning_text.is_empty() { None } else { Some(reasoning_text) },
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        finish_reason: final_finish,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_non_streaming_tool_calls_reproduced_leak() {
        let raw_leak = r#"{"response": {"candidates": [{"content": {"role": "model","parts": [{"thoughtSignature": "dummy-sig","functionCall": {"name": "consultant_history","args": {"query": "bootstrapper kiosk shell"},"id": "call_133571"}}]}}],"usageMetadata": {"promptTokenCount": 15618,"candidatesTokenCount": 21,"totalTokenCount": 15731,"thoughtsTokenCount": 92},"modelVersion": "gemini-3.8-flash-tiered","responseId": "SKflaqjhEebCnsEP-pef4QM"},"traceId": "7c19ed507ba88b00","metadata": {}}
data: {"response": {"candidates": [{"content": {"role": "model","parts": [{"functionCall": {"name": "search_files","args": {"path": "D:/Side-Projects","pattern": "*kiosk*","target": "files"},"id": "call_133577"}}]}}],"usageMetadata": {"promptTokenCount": 15618,"candidatesTokenCount": 56,"totalTokenCount": 15766,"thoughtsTokenCount": 92},"modelVersion": "gemini-3.8-flash-tiered","responseId": "SKflaqjhEebCnsEP-pef4QM"},"traceId": "7c19ed507ba88b00","metadata": {}}
data: {"response": {"candidates": [{"content": {"role": "model","parts": [{"text": ""}]},"finishReason": "STOP"}],"usageMetadata": {"promptTokenCount": 15618,"candidatesTokenCount": 56,"totalTokenCount": 15766,"thoughtsTokenCount": 92},"modelVersion": "gemini-3.8-flash-tiered","responseId": "SKflaqjhEebCnsEP-pef4QM"},"traceId": "7c19ed507ba88b00","metadata": {}}"#;

        let extracted = extract_non_streaming_from_sse(raw_leak);
        assert_eq!(extracted.finish_reason, "tool_calls");
        assert!(extracted.content.is_none());
        assert!(extracted.tool_calls.is_some());

        let tools = extracted.tool_calls.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["function"]["name"], "consultant_history");
        assert_eq!(tools[1]["function"]["name"], "search_files");
    }

    #[test]
    fn test_extract_non_streaming_mixed_text_and_thoughts() {
        let raw = r#"
data: {"response": {"candidates": [{"content": {"role": "model","parts": [{"thought": true, "text": "Thinking deeply..."}]}}]}}
data: {"response": {"candidates": [{"content": {"role": "model","parts": [{"thought": false, "text": "Hello world!"}]},"finishReason": "STOP"}]}}
"#;
        let extracted = extract_non_streaming_from_sse(raw);
        assert_eq!(extracted.finish_reason, "stop");
        assert_eq!(extracted.content.as_deref(), Some("Hello world!"));
        assert_eq!(extracted.reasoning_content.as_deref(), Some("Thinking deeply..."));
        assert!(extracted.tool_calls.is_none());
    }
}
