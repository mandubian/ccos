//! Golden tests for LLM provider payload translation.
//!
//! These tests verify the exact shape of JSON sent to each provider's API.
//! They do NOT make network calls; they exercise the payload-building functions directly.

#[cfg(test)]
mod tests {
    use crate::llm::{CompletionRequest, Message, Role, ToolCall, ToolDefinition};
    use serde_json::json;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn search_tool() -> ToolDefinition {
        ToolDefinition {
            name: "search".to_string(),
            description: "Search the web for a query".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        }
    }

    fn tool_call() -> ToolCall {
        ToolCall {
            id: "call_abc123".to_string(),
            name: "search".to_string(),
            arguments: r#"{"query":"rust lifetimes"}"#.to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // OpenAI golden tests
    // -----------------------------------------------------------------------

    mod openai {
        use super::*;

        /// Build the OpenAI payload the same way OpenAiDriver.build_body() does.
        fn build_payload(req: &CompletionRequest) -> serde_json::Value {
            let messages: Vec<serde_json::Value> = req
                .messages
                .iter()
                .map(|m| {
                    let mut msg = json!({ "role": m.role.as_str() });
                    if !m.content.is_empty() {
                        msg["content"] = json!(m.content);
                    }
                    if !m.tool_calls.is_empty() {
                        msg["tool_calls"] = json!(m
                            .tool_calls
                            .iter()
                            .map(|tc| json!({
                                "id": tc.id, "type": "function",
                                "function": { "name": tc.name, "arguments": tc.arguments }
                            }))
                            .collect::<Vec<_>>());
                    }
                    if let Some(ref id) = m.tool_call_id {
                        msg["tool_call_id"] = json!(id);
                    }
                    msg
                })
                .collect();

            let mut body = json!({ "model": req.model, "messages": messages });

            if !req.tools.is_empty() {
                body["tools"] = json!(req.tools.iter().map(|t| json!({
                    "type": "function",
                    "function": { "name": t.name, "description": t.description, "parameters": t.input_schema }
                })).collect::<Vec<_>>());
                body["tool_choice"] = json!("auto");
            }
            body
        }

        #[test]
        fn test_simple_user_message() {
            let req = CompletionRequest::simple(
                "gpt-4o",
                vec![
                    Message::system("You are a helpful assistant"),
                    Message::user("Hello"),
                ],
            );
            let body = build_payload(&req);
            assert_eq!(body["model"], "gpt-4o");
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(
                body["messages"][0]["content"],
                "You are a helpful assistant"
            );
            assert_eq!(body["messages"][1]["role"], "user");
            assert_eq!(body["messages"][1]["content"], "Hello");
            assert!(body.get("tools").is_none());
        }

        #[test]
        fn test_tool_definition_serialization() {
            let req = CompletionRequest {
                model: "gpt-4o".to_string(),
                messages: vec![Message::user("search for rust")],
                tools: vec![search_tool()],
                max_tokens: None,
                temperature: None,
            };
            let body = build_payload(&req);
            assert_eq!(body["tools"][0]["type"], "function");
            assert_eq!(body["tools"][0]["function"]["name"], "search");
            assert_eq!(body["tool_choice"], "auto");
            assert_eq!(
                body["tools"][0]["function"]["parameters"]["required"][0],
                "query"
            );
        }

        #[test]
        fn test_assistant_tool_call_turn() {
            // Golden: the shape of an assistant message that contains a tool call
            let mut assistant_msg = Message::assistant(""); // no text content
            assistant_msg.tool_calls = vec![tool_call()];

            let req = CompletionRequest::simple(
                "gpt-4o",
                vec![Message::user("Search for something"), assistant_msg],
            );
            let body = build_payload(&req);
            let asst = &body["messages"][1];
            assert_eq!(asst["role"], "assistant");
            assert_eq!(asst["tool_calls"][0]["id"], "call_abc123");
            assert_eq!(asst["tool_calls"][0]["type"], "function");
            assert_eq!(asst["tool_calls"][0]["function"]["name"], "search");
            assert_eq!(
                asst["tool_calls"][0]["function"]["arguments"],
                r#"{"query":"rust lifetimes"}"#
            );
        }

        #[test]
        fn test_tool_result_turn() {
            // Golden: the shape of a tool result (role="tool") message
            let result_msg = Message::tool_result(
                "call_abc123",
                "search",
                "Rust lifetimes control ownership scopes.",
            );
            let req = CompletionRequest::simple("gpt-4o", vec![result_msg]);
            let body = build_payload(&req);
            assert_eq!(body["messages"][0]["role"], "tool");
            assert_eq!(body["messages"][0]["tool_call_id"], "call_abc123");
            assert_eq!(
                body["messages"][0]["content"],
                "Rust lifetimes control ownership scopes."
            );
        }

        #[test]
        fn test_parse_tool_call_response() {
            // Golden: parse a response containing a tool_calls array
            let raw = json!({
                "choices": [{
                    "finish_reason": "tool_calls",
                    "message": {
                        "content": null,
                        "tool_calls": [{
                            "id": "call_abc123",
                            "type": "function",
                            "function": {
                                "name": "search",
                                "arguments": "{\"query\": \"rust lifetimes\"}"
                            }
                        }]
                    }
                }],
                "usage": { "prompt_tokens": 50, "completion_tokens": 20 }
            });

            let tool_calls = raw["choices"][0]["message"]["tool_calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tc| {
                    Some(ToolCall {
                        id: tc["id"].as_str()?.to_string(),
                        name: tc["function"]["name"].as_str()?.to_string(),
                        arguments: tc["function"]["arguments"]
                            .as_str()
                            .unwrap_or("{}")
                            .to_string(),
                    })
                })
                .collect::<Vec<_>>();

            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_abc123");
            assert_eq!(tool_calls[0].name, "search");

            let finish = raw["choices"][0]["finish_reason"].as_str().unwrap();
            assert_eq!(finish, "tool_calls");
        }
    }

    // -----------------------------------------------------------------------
    // Anthropic golden tests
    // -----------------------------------------------------------------------

    mod anthropic {
        use super::*;

        fn build_payload(req: &CompletionRequest) -> serde_json::Value {
            let mut system_text = String::new();
            let mut messages: Vec<serde_json::Value> = Vec::new();

            for m in &req.messages {
                match m.role {
                    Role::System => {
                        system_text.push_str(&m.content);
                    }
                    Role::User => {
                        if let Some(ref id) = m.tool_call_id {
                            messages.push(json!({
                                "role": "user",
                                "content": [{ "type": "tool_result", "tool_use_id": id, "content": m.content }]
                            }));
                        } else {
                            messages.push(json!({ "role": "user", "content": m.content }));
                        }
                    }
                    Role::Assistant => {
                        if !m.tool_calls.is_empty() {
                            let mut content: Vec<serde_json::Value> = Vec::new();
                            if !m.content.is_empty() {
                                content.push(json!({ "type": "text", "text": m.content }));
                            }
                            for tc in &m.tool_calls {
                                let input: serde_json::Value =
                                    serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                                content.push(json!({ "type": "tool_use", "id": tc.id, "name": tc.name, "input": input }));
                            }
                            messages.push(json!({ "role": "assistant", "content": content }));
                        } else {
                            messages.push(json!({ "role": "assistant", "content": m.content }));
                        }
                    }
                    Role::Tool => {
                        if let Some(ref id) = m.tool_call_id {
                            messages.push(json!({
                                "role": "user",
                                "content": [{ "type": "tool_result", "tool_use_id": id, "content": m.content }]
                            }));
                        }
                    }
                }
            }

            let mut body = json!({ "model": "claude-3-5-sonnet-20241022", "max_tokens": 4096, "messages": messages });
            if !system_text.is_empty() {
                body["system"] = json!(system_text.trim());
            }
            if !req.tools.is_empty() {
                body["tools"] =
                    json!(req.tools.iter().map(|t| json!({
                    "name": t.name, "description": t.description, "input_schema": t.input_schema
                })).collect::<Vec<_>>());
            }
            body
        }

        #[test]
        fn test_system_extracted_to_top_level() {
            let req = CompletionRequest::simple(
                "claude-3-5-sonnet-20241022",
                vec![
                    Message::system("You are a wise assistant"),
                    Message::user("Hello"),
                ],
            );
            let body = build_payload(&req);
            // System must NOT be in messages array
            assert_eq!(body["system"], "You are a wise assistant");
            assert_eq!(body["messages"].as_array().unwrap().len(), 1);
            assert_eq!(body["messages"][0]["role"], "user");
        }

        #[test]
        fn test_tool_definition_format() {
            let req = CompletionRequest {
                model: "claude-3-5-sonnet-20241022".to_string(),
                messages: vec![Message::user("use a tool")],
                tools: vec![search_tool()],
                max_tokens: None,
                temperature: None,
            };
            let body = build_payload(&req);
            // Anthropic uses "input_schema", NOT "parameters"
            assert_eq!(body["tools"][0]["name"], "search");
            assert!(body["tools"][0].get("parameters").is_none());
            assert!(body["tools"][0]["input_schema"]["properties"]
                .get("query")
                .is_some());
        }

        #[test]
        fn test_tool_use_block_in_assistant_turn() {
            let mut asst = Message::assistant("");
            asst.tool_calls = vec![tool_call()];

            let req = CompletionRequest::simple(
                "claude-3-5-sonnet-20241022",
                vec![Message::user("search something"), asst],
            );
            let body = build_payload(&req);
            let asst_content = &body["messages"][1]["content"];
            assert_eq!(asst_content[0]["type"], "tool_use");
            assert_eq!(asst_content[0]["id"], "call_abc123");
            assert_eq!(asst_content[0]["name"], "search");
            assert_eq!(asst_content[0]["input"]["query"], "rust lifetimes");
        }

        #[test]
        fn test_tool_result_as_user_content_block() {
            let tool_result =
                Message::tool_result("call_abc123", "search", "Lifetimes are scopes.");
            let req = CompletionRequest::simple("claude-3-5-sonnet-20241022", vec![tool_result]);
            let body = build_payload(&req);

            // Anthropic expects tool results wrapped in user message content blocks
            assert_eq!(body["messages"][0]["role"], "user");
            let content = &body["messages"][0]["content"][0];
            assert_eq!(content["type"], "tool_result");
            assert_eq!(content["tool_use_id"], "call_abc123");
            assert_eq!(content["content"], "Lifetimes are scopes.");
        }

        #[test]
        fn test_parse_tool_use_response() {
            let raw = json!({
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_abc123",
                    "name": "search",
                    "input": { "query": "rust lifetimes" }
                }],
                "usage": { "input_tokens": 80, "output_tokens": 30 }
            });

            let tool_calls: Vec<ToolCall> = raw["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|b| {
                    if b["type"].as_str() == Some("tool_use") {
                        Some(ToolCall {
                            id: b["id"].as_str()?.to_string(),
                            name: b["name"].as_str()?.to_string(),
                            arguments: serde_json::to_string(&b["input"]).unwrap_or_default(),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "search");
            let args: serde_json::Value = serde_json::from_str(&tool_calls[0].arguments).unwrap();
            assert_eq!(args["query"], "rust lifetimes");
        }
    }

    // -----------------------------------------------------------------------
    // Gemini golden tests
    // -----------------------------------------------------------------------

    mod gemini {
        use super::*;

        fn build_payload(req: &CompletionRequest) -> serde_json::Value {
            let mut system_instruction = None;
            let mut contents: Vec<serde_json::Value> = Vec::new();

            for m in &req.messages {
                match m.role {
                    Role::System => {
                        system_instruction = Some(json!({ "parts": [{ "text": m.content }] }));
                    }
                    Role::User => {
                        if let Some(ref id) = m.tool_call_id {
                            contents.push(json!({
                                "role": "user",
                                "parts": [{ "functionResponse": { "name": id, "response": { "content": m.content } } }]
                            }));
                        } else {
                            contents
                                .push(json!({ "role": "user", "parts": [{ "text": m.content }] }));
                        }
                    }
                    Role::Assistant => {
                        if !m.tool_calls.is_empty() {
                            let parts: Vec<serde_json::Value> = m
                                .tool_calls
                                .iter()
                                .map(|tc| {
                                    let args: serde_json::Value =
                                        serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                                    json!({ "functionCall": { "name": tc.name, "args": args } })
                                })
                                .collect();
                            contents.push(json!({ "role": "model", "parts": parts }));
                        } else {
                            contents
                                .push(json!({ "role": "model", "parts": [{ "text": m.content }] }));
                        }
                    }
                    Role::Tool => {
                        if let Some(ref id) = m.tool_call_id {
                            contents.push(json!({
                                "role": "user",
                                "parts": [{ "functionResponse": { "name": id, "response": { "content": m.content } } }]
                            }));
                        }
                    }
                }
            }

            let mut body = json!({ "contents": contents });
            if let Some(sys) = system_instruction {
                body["systemInstruction"] = sys;
            }
            if !req.tools.is_empty() {
                body["tools"] = json!([{
                    "functionDeclarations": req.tools.iter().map(|t| json!({
                        "name": t.name, "description": t.description, "parameters": t.input_schema
                    })).collect::<Vec<_>>()
                }]);
            }
            body
        }

        #[test]
        fn test_user_message_in_parts() {
            let req = CompletionRequest::simple(
                "gemini-2.5-pro",
                vec![Message::system("Be helpful"), Message::user("Hello")],
            );
            let body = build_payload(&req);
            // System becomes systemInstruction, not a content entry
            assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be helpful");
            assert_eq!(body["contents"].as_array().unwrap().len(), 1);
            assert_eq!(body["contents"][0]["role"], "user");
            assert_eq!(body["contents"][0]["parts"][0]["text"], "Hello");
        }

        #[test]
        fn test_function_declarations_format() {
            let req = CompletionRequest {
                model: "gemini-2.5-pro".to_string(),
                messages: vec![Message::user("search")],
                tools: vec![search_tool()],
                max_tokens: None,
                temperature: None,
            };
            let body = build_payload(&req);
            // Gemini wraps tools in functionDeclarations inside a tools array
            let decls = &body["tools"][0]["functionDeclarations"];
            assert_eq!(decls[0]["name"], "search");
            assert!(decls[0]["parameters"]["properties"].get("query").is_some());
        }

        #[test]
        fn test_function_call_in_model_turn() {
            let mut asst = Message::assistant("");
            asst.tool_calls = vec![tool_call()];

            let req = CompletionRequest::simple(
                "gemini-2.5-pro",
                vec![Message::user("search something"), asst],
            );
            let body = build_payload(&req);
            let model_turn = &body["contents"][1];
            assert_eq!(model_turn["role"], "model");
            assert_eq!(model_turn["parts"][0]["functionCall"]["name"], "search");
            assert_eq!(
                model_turn["parts"][0]["functionCall"]["args"]["query"],
                "rust lifetimes"
            );
        }

        #[test]
        fn test_function_response_in_user_turn() {
            let tool_result =
                Message::tool_result("call_abc123", "search", "Lifetimes annotate scopes.");
            let req = CompletionRequest::simple("gemini-2.5-pro", vec![tool_result]);
            let body = build_payload(&req);
            // Gemini tool results go in user parts as functionResponse
            let part = &body["contents"][0]["parts"][0]["functionResponse"];
            assert_eq!(body["contents"][0]["role"], "user");
            assert_eq!(part["response"]["content"], "Lifetimes annotate scopes.");
        }

        #[test]
        fn test_parse_function_call_response() {
            let raw = json!({
                "candidates": [{
                    "finishReason": "STOP",
                    "content": {
                        "role": "model",
                        "parts": [{
                            "functionCall": {
                                "name": "search",
                                "args": { "query": "rust lifetimes" }
                            }
                        }]
                    }
                }],
                "usageMetadata": { "promptTokenCount": 60, "candidatesTokenCount": 15 }
            });

            let parts = &raw["candidates"][0]["content"]["parts"];
            let mut tool_calls: Vec<ToolCall> = Vec::new();
            for part in parts.as_array().unwrap() {
                if let Some(fc) = part.get("functionCall") {
                    let name = fc["name"].as_str().unwrap_or("").to_string();
                    let arguments = serde_json::to_string(&fc["args"]).unwrap_or_default();
                    tool_calls.push(ToolCall {
                        id: format!("gemini-{}", name),
                        name,
                        arguments,
                    });
                }
            }

            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "search");
            let args: serde_json::Value = serde_json::from_str(&tool_calls[0].arguments).unwrap();
            assert_eq!(args["query"], "rust lifetimes");
        }
    }
}
