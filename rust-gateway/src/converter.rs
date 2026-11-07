//! Full Google CloudCode Payload Builder & Message Converter

use serde_json::{json, Map, Value};
use crate::types::ChatMessage;

fn is_unsupported_schema_key(key: &str) -> bool {
    matches!(
        key,
        "additionalProperties"
            | "default"
            | "$schema"
            | "$defs"
            | "definitions"
            | "$ref"
            | "$id"
            | "$comment"
            | "title"
            | "minLength"
            | "maxLength"
            | "pattern"
            | "format"
            | "minItems"
            | "maxItems"
            | "examples"
            | "allOf"
            | "anyOf"
            | "oneOf"
            | "strict"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
    )
}

fn score_schema_option(value: &Value) -> i32 {
    if let Some(obj) = value.as_object() {
        if obj.get("type").and_then(|t| t.as_str()) == Some("object") || obj.contains_key("properties") {
            return 3;
        }
        if obj.get("type").and_then(|t| t.as_str()) == Some("array") || obj.contains_key("items") {
            return 2;
        }
        if let Some(t) = obj.get("type").and_then(|t| t.as_str()) {
            if t != "null" {
                return 1;
            }
        }
    }
    0
}

fn append_description_hint(obj: &mut Map<String, Value>, hint: &str) {
    let merged = match obj.get("description").and_then(|d| d.as_str()) {
        Some(existing) if !existing.is_empty() => format!("{} ({})", existing, hint),
        _ => hint.to_string(),
    };
    obj.insert("description".into(), Value::String(merged));
}

/// Strip JSON Schema constructs that Google's function declaration API rejects,
/// while preserving as much type information as possible.
pub fn sanitize_schema_for_gemini(schema: &Value) -> Value {
    match schema {
        Value::Array(items) => {
            Value::Array(items.iter().map(sanitize_schema_for_gemini).collect())
        }
        Value::Object(obj) => {
            if let Some(reference) = obj.get("$ref").and_then(|v| v.as_str()) {
                let name = reference.rsplit('/').next().unwrap_or("unknown");
                return json!({ "type": "object", "description": format!("See: {}", name) });
            }

            let mut out: Map<String, Value> = obj.clone();

            if let Some(all_of) = out.remove("allOf") {
                if let Some(subschemas) = all_of.as_array() {
                    let mut merged_properties: Map<String, Value> = Map::new();
                    let mut merged_required: Vec<Value> = Vec::new();
                    let mut other: Map<String, Value> = Map::new();
                    for sub in subschemas {
                        if let Some(sub_obj) = sub.as_object() {
                            if let Some(props) = sub_obj.get("properties").and_then(|v| v.as_object()) {
                                for (k, v) in props {
                                    merged_properties.insert(k.clone(), v.clone());
                                }
                            }
                            if let Some(required) = sub_obj.get("required").and_then(|v| v.as_array()) {
                                for r in required {
                                    if !merged_required.contains(r) {
                                        merged_required.push(r.clone());
                                    }
                                }
                            }
                            for (k, v) in sub_obj {
                                if k != "properties" && k != "required" && !other.contains_key(k) {
                                    other.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                    for (k, v) in other {
                        if !out.contains_key(&k) {
                            out.insert(k, v);
                        }
                    }
                    if !merged_properties.is_empty() {
                        if let Some(props) = out.get("properties").and_then(|v| v.as_object()) {
                            for (k, v) in props {
                                merged_properties.insert(k.clone(), v.clone());
                            }
                        }
                        out.insert("properties".into(), Value::Object(merged_properties));
                    }
                    if !merged_required.is_empty() {
                        if let Some(required) = out.get("required").and_then(|v| v.as_array()) {
                            for r in required {
                                if !merged_required.contains(r) {
                                    merged_required.push(r.clone());
                                }
                            }
                        }
                        out.insert("required".into(), Value::Array(merged_required));
                    }
                }
            }

            for union_key in ["anyOf", "oneOf"] {
                if let Some(union) = out.remove(union_key) {
                    if let Some(options) = union.as_array() {
                        let mut best: Option<&Value> = None;
                        let mut best_score = -1;
                        let mut type_names: Vec<String> = Vec::new();
                        for option in options {
                            if let Some(option_obj) = option.as_object() {
                                if let Some(t) = option_obj.get("type").and_then(|v| v.as_str()) {
                                    if t != "null" {
                                        type_names.push(t.to_string());
                                    }
                                } else if option_obj.contains_key("properties") {
                                    type_names.push("object".into());
                                }
                                let score = score_schema_option(option);
                                if score > best_score {
                                    best_score = score;
                                    best = Some(option);
                                }
                            }
                        }
                        if let Some(selected) = best {
                            let flattened = sanitize_schema_for_gemini(selected);
                            if let Some(flattened_obj) = flattened.as_object() {
                                let parent_description = out.get("description").cloned();
                                for (k, v) in flattened_obj {
                                    if k == "description" {
                                        if parent_description.as_ref() != Some(v) {
                                            match parent_description.as_ref().and_then(|d| d.as_str()) {
                                                Some(parent) => {
                                                    let merged = format!("{} ({})", parent, v.as_str().unwrap_or(""));
                                                    out.insert("description".into(), Value::String(merged));
                                                }
                                                None => {
                                                    out.insert("description".into(), v.clone());
                                                }
                                            }
                                        }
                                    } else if !out.contains_key(k) || k == "type" || k == "properties" || k == "items" {
                                        out.insert(k.clone(), v.clone());
                                    }
                                }
                                if type_names.len() > 1 {
                                    let mut unique: Vec<String> = Vec::new();
                                    for t in type_names {
                                        if !unique.contains(&t) {
                                            unique.push(t);
                                        }
                                    }
                                    append_description_hint(&mut out, &format!("Accepts: {}", unique.join(" | ")));
                                }
                            }
                        }
                    }
                }
            }

            if let Some(type_value) = out.get("type").cloned() {
                if let Some(type_array) = type_value.as_array() {
                    let mut chosen: Option<String> = None;
                    let mut nullable = false;
                    for t in type_array {
                        if let Some(name) = t.as_str() {
                            if name == "null" {
                                nullable = true;
                            } else if chosen.is_none() {
                                chosen = Some(name.to_string());
                            }
                        }
                    }
                    match chosen {
                        Some(name) => {
                            out.insert("type".into(), Value::String(name));
                        }
                        None => {
                            out.remove("type");
                        }
                    }
                    if nullable {
                        out.insert("nullable".into(), Value::Bool(true));
                    }
                }
            }

            let keys: Vec<String> = out.keys().cloned().collect();
            for key in keys {
                if is_unsupported_schema_key(&key) {
                    out.remove(&key);
                }
            }

            if let Some(props) = out.get("properties").and_then(|v| v.as_object()).cloned() {
                let mut cleaned = Map::new();
                for (k, v) in props {
                    cleaned.insert(k, sanitize_schema_for_gemini(&v));
                }
                out.insert("properties".into(), Value::Object(cleaned));
            }

            if let Some(items) = out.get("items").cloned() {
                out.insert("items".into(), sanitize_schema_for_gemini(&items));
            }

            if let Some(required) = out.get("required").and_then(|v| v.as_array()).cloned() {
                if let Some(props) = out.get("properties").and_then(|v| v.as_object()) {
                    let filtered: Vec<Value> = required
                        .into_iter()
                        .filter(|r| r.as_str().map(|s| props.contains_key(s)).unwrap_or(false))
                        .collect();
                    if filtered.is_empty() {
                        out.remove("required");
                    } else {
                        out.insert("required".into(), Value::Array(filtered));
                    }
                }
            }

            Value::Object(out)
        }
        _ => schema.clone(),
    }
}

pub fn convert_messages_to_google(messages: &[ChatMessage]) -> (Option<Value>, Vec<Value>) {
    let mut system_instruction: Option<Value> = None;
    let mut contents: Vec<Value> = Vec::new();

    // Map tool_call_id to function name for OpenAI -> Gemini tool response correlation
    let mut tool_id_to_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for msg in messages {
        if let Some(tool_calls) = &msg.tool_calls {
            if let Some(calls) = tool_calls.as_array() {
                for call in calls {
                    if let (Some(id), Some(name)) = (
                        call.get("id").and_then(|i| i.as_str()),
                        call.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()),
                    ) {
                        tool_id_to_name.insert(id.to_string(), name.to_string());
                    }
                }
            }
        }
    }

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

        // Handle text content (only for user/assistant messages, not tool responses)
        if role != "tool" {
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
                            Value::String(s) => match serde_json::from_str::<Value>(s) {
                                Ok(Value::Object(m)) => Value::Object(m),
                                _ => json!({}),
                            },
                            Value::Object(m) => Value::Object(m.clone()),
                            _ => json!({}),
                        };
                        parts.push(json!({
                            "functionCall": {
                                "name": name,
                                "args": parsed_args
                            },
                            "thoughtSignature": "skip_thought_signature_validator"
                        }));
                    }
                }
            }
        }

        // Handle tool responses from client
        if role == "tool" {
            let name = msg.name.clone()
                .or_else(|| msg.tool_call_id.as_ref().and_then(|id| tool_id_to_name.get(id).cloned()))
                .unwrap_or_else(|| "tool_result".into());
            let content_val = match &msg.content {
                Value::String(s) => match serde_json::from_str::<Value>(s) {
                    Ok(Value::Object(map)) => Value::Object(map),
                    _ => json!({ "output": s }),
                },
                Value::Object(map) => Value::Object(map.clone()),
                other => json!({ "output": other }),
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

    if let Some(last) = contents.last() {
        let is_model = last.get("role").and_then(|r| r.as_str()) == Some("model");
        let has_function_call = last
            .get("parts")
            .and_then(|p| p.as_array())
            .map(|parts| parts.iter().any(|part| part.get("functionCall").is_some()))
            .unwrap_or(false);
        if is_model && !has_function_call {
            contents.push(json!({
                "role": "user",
                "parts": [{ "text": "Continue." }]
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
                if let Some(func) = t.get("function").and_then(|f| f.as_object()) {
                    let mut declaration = Map::new();
                    if let Some(name) = func.get("name") {
                        declaration.insert("name".into(), name.clone());
                    }
                    if let Some(description) = func.get("description") {
                        declaration.insert("description".into(), description.clone());
                    }
                    if let Some(parameters) = func.get("parameters") {
                        declaration.insert("parameters".into(), sanitize_schema_for_gemini(parameters));
                    }
                    declarations.push(Value::Object(declaration));
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

    #[test]
    fn test_convert_messages_tool_response_struct_and_thought_signature() {
        let messages = vec![
            ChatMessage {
                role: "assistant".into(),
                content: serde_json::Value::Null,
                name: None,
                tool_calls: Some(json!([{
                    "id": "call_123",
                    "type": "function",
                    "function": {
                        "name": "run_shell",
                        "arguments": "{\"cmd\":\"echo 10\"}"
                    }
                }])),
                tool_call_id: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: json!("10"),
                name: None,
                tool_calls: None,
                tool_call_id: Some("call_123".into()),
            },
        ];

        let (_, contents) = convert_messages_to_google(&messages);
        assert_eq!(contents.len(), 2);

        // Model call part must contain thoughtSignature
        let model_parts = contents[0]["parts"].as_array().unwrap();
        assert_eq!(model_parts[0]["thoughtSignature"], "skip_thought_signature_validator");
        assert_eq!(model_parts[0]["functionCall"]["name"], "run_shell");

        // Tool response part must be correlated to "run_shell" and response must be a JSON Object Struct
        let tool_parts = contents[1]["parts"].as_array().unwrap();
        let fn_resp = &tool_parts[0]["functionResponse"];
        assert_eq!(fn_resp["name"], "run_shell");
        assert!(fn_resp["response"].is_object(), "functionResponse.response MUST be a JSON Object (Google Struct)");
        assert_eq!(fn_resp["response"]["output"], "10");
    }

    #[test]
    fn test_convert_messages_appends_user_turn_after_trailing_model_text() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: json!("hi"),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: "assistant".into(),
                content: json!("hello"),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let (_, contents) = convert_messages_to_google(&messages);
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(contents[2]["parts"][0]["text"], "Continue.");
    }

    #[test]
    fn test_convert_messages_does_not_append_after_trailing_tool_call() {
        let messages = vec![ChatMessage {
            role: "assistant".into(),
            content: serde_json::Value::Null,
            name: None,
            tool_calls: Some(json!([{
                "id": "call_1",
                "type": "function",
                "function": { "name": "run_shell", "arguments": "{}" }
            }])),
            tool_call_id: None,
        }];

        let (_, contents) = convert_messages_to_google(&messages);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "model");
    }

    #[test]
    fn test_sanitize_schema_removes_unsupported_and_flattens() {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "cmd": { "type": "string", "format": "uri", "default": "ls" },
                "count": { "anyOf": [{ "type": "null" }, { "type": "integer" }] }
            },
            "required": ["cmd", "missing"]
        });

        let cleaned = sanitize_schema_for_gemini(&schema);
        assert!(cleaned.get("$schema").is_none());
        assert!(cleaned.get("additionalProperties").is_none());

        let props = cleaned["properties"].as_object().unwrap();
        assert!(props["cmd"].get("format").is_none());
        assert!(props["cmd"].get("default").is_none());
        assert_eq!(props["count"]["type"], "integer");

        let required = cleaned["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "cmd");
    }

    #[test]
    fn test_sanitize_schema_removes_exclusive_bounds() {
        let schema = json!({
            "type": "object",
            "properties": {
                "days": {
                    "type": "integer",
                    "exclusiveMinimum": 0,
                    "exclusiveMaximum": 15
                }
            },
            "required": ["days"]
        });

        let cleaned = sanitize_schema_for_gemini(&schema);
        let days = &cleaned["properties"]["days"];
        assert!(days.get("exclusiveMinimum").is_none());
        assert!(days.get("exclusiveMaximum").is_none());
        assert_eq!(days["type"], "integer");
    }

    #[test]
    fn test_build_payload_sanitizes_tool_parameters() {
        let messages: Vec<ChatMessage> = Vec::new();
        let tools = json!([{
            "type": "function",
            "function": {
                "name": "run_shell",
                "description": "run a command",
                "strict": true,
                "parameters": {
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "type": "object",
                    "additionalProperties": false,
                    "properties": { "cmd": { "type": "string" } },
                    "required": ["cmd"]
                }
            }
        }]);

        let payload = build_cloudcode_payload("project", "gemini-3.8-flash", &messages, Some(&tools), None);
        let declaration = &payload["request"]["tools"][0]["functionDeclarations"][0];
        assert_eq!(declaration["name"], "run_shell");
        assert!(declaration.get("strict").is_none());
        assert!(declaration["parameters"].get("$schema").is_none());
        assert!(declaration["parameters"].get("additionalProperties").is_none());
        assert_eq!(declaration["parameters"]["properties"]["cmd"]["type"], "string");
    }
}
