//! rtfs_compiler/src/capabilities/collaboration.rs

use crate::runtime::capability::{Capability, CapabilitySpec, CapabilityProvider};
use crate::ast::{Value, RuntimeError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

// A capability to post a message to Slack.
pub struct SlackPostCapability;

#[async_trait]
impl Capability for SlackPostCapability {
    fn spec(&self) -> &CapabilitySpec {
        &CapabilitySpec {
            name: "com.collaboration:v1.slack-post".to_string(),
            version: "1.0.0".to_string(),
            metadata: HashMap::new(),
        }
    }

    async fn execute(&self, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        let channel = params.get("channel").and_then(|v| v.as_str()).unwrap_or("#general");
        let summary = params.get("summary").and_then(|v| v.as_str()).unwrap_or("");
        
        println!("Simulating posting to Slack channel '{}': {}", channel, summary);
        
        // Simulate a failure for demonstration purposes
        // In a real scenario, this would be a network call that could fail.
        // To make the demo work, we will return an error. To make it succeed, comment out the error.
        // return Err(RuntimeError::CapabilityError("network error".to_string()));

        let mut result_map = HashMap::new();
        result_map.insert("status".to_string(), Value::Keyword(":slack-success".to_string()));
        Ok(Value::Map(result_map))
    }
}

// A capability to send an email.
pub struct SendEmailCapability;

#[async_trait]
impl Capability for SendEmailCapability {
    fn spec(&self) -> &CapabilitySpec {
        &CapabilitySpec {
            name: "com.collaboration:v1.send-email".to_string(),
            version: "1.0.0".to_string(),
            metadata: HashMap::new(),
        }
    }

    async fn execute(&self, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        let to = params.get("to").and_then(|v| v.as_str()).unwrap_or("");
        let subject = params.get("subject").and_then(|v| v.as_str()).unwrap_or("");
        let body = params.get("body").and_then(|v| v.as_str()).unwrap_or("");

        println!("Simulating sending email to '{}' with subject '{}': {}", to, subject, body);
        
        let mut result_map = HashMap::new();
        result_map.insert("status".to_string(), Value::Keyword(":email-fallback-success".to_string()));
        Ok(Value::Map(result_map))
    }
}

// A provider for our example collaboration capabilities.
pub struct CollaborationProvider;

impl CapabilityProvider for CollaborationProvider {
    fn get_capabilities(&self) -> Vec<Arc<dyn Capability>> {
        vec![
            Arc::new(SlackPostCapability),
            Arc::new(SendEmailCapability),
        ]
    }
}
