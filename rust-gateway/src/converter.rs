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
