#[cfg(test)]
mod tests {
    use crate::llm::{CompletionRequest, Message, Role};
    use serde_json::json;

    #[test]
    fn test_openai_payload() {
        let req = CompletionRequest {
            model: "gpt-4o".to_string(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: "You are a bot".to_string(),
                },
                Message {
                    role: Role::User,
                    content: "Hello".to_string(),
                },
            ],
            max_tokens: None,
            temperature: Some(0.7),
        };

        let messages = req
            .messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role.as_str(),
                    "content": m.content
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": "gpt-4o",
            "messages": messages,
        });

        if let Some(t) = req.temperature {
            body.as_object_mut()
                .unwrap()
                .insert("temperature".to_string(), json!(t));
        }

        assert_eq!(body["model"], "gpt-4o");
        let temp_actual = body["temperature"].as_f64().unwrap();
        assert!((temp_actual - 0.7).abs() < 0.001);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Hello");
    }

    #[test]
    fn test_anthropic_payload() {
        let req = CompletionRequest {
            model: "claude-3-5-sonnet".to_string(),
            messages: vec![
                Message {
                    role: Role::System,
                    content: "You are Claude".to_string(),
                },
                Message {
                    role: Role::User,
                    content: "Hi".to_string(),
                },
            ],
            max_tokens: Some(1024),
            temperature: Some(0.0), // test omitted
        };

        let mut system_text = String::new();
        let mut messages = Vec::new();

        for m in &req.messages {
            if m.role == Role::System {
                system_text.push_str(&m.content);
                system_text.push('\n');
            } else {
                messages.push(json!({
                    "role": m.role.as_str(),
                    "content": m.content
                }));
            }
        }

        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens.unwrap_or(4096),
            "messages": messages,
        });

        if !system_text.is_empty() {
            body.as_object_mut()
                .unwrap()
                .insert("system".to_string(), json!(system_text.trim()));
        }

        assert_eq!(body["system"], "You are Claude");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1); // system was extracted
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["max_tokens"], 1024);
        assert!(body.get("temperature").is_none());
    }
}
