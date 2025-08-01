//! rtfs_compiler/src/capabilities/http_api.rs

use crate::runtime::capability::{Capability, CapabilitySpec, CapabilityProvider};
use crate::ast::{Value, RuntimeError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

// A generic capability for making HTTP GET requests.
pub struct HttpGetCapability {
    spec: CapabilitySpec,
    base_url: String,
}

impl HttpGetCapability {
    pub fn new(name: &str, version: &str, base_url: &str) -> Self {
        let mut metadata = HashMap::new();
        metadata.insert("protocol".to_string(), "http".to_string());
        metadata.insert("method".to_string(), "get".to_string());

        Self {
            spec: CapabilitySpec {
                name: name.to_string(),
                version: version.to_string(),
                metadata,
            },
            base_url: base_url.to_string(),
        }
    }
}

#[async_trait]
impl Capability for HttpGetCapability {
    fn spec(&self) -> &CapabilitySpec {
        &self.spec
    }

    async fn execute(&self, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        // In a real implementation, this would use an HTTP client like reqwest.
        // We'll simulate it for now.
        let topic = params.get("topic").and_then(|v| v.as_str()).unwrap_or_default();
        println!("Simulating HTTP GET to {} with topic: {}", self.base_url, topic);

        // Simulate a successful response with dummy data.
        let mut result_map = HashMap::new();
        result_map.insert("data".to_string(), Value::String(format!("Data for {}", topic)));
        
        Ok(Value::Map(result_map))
    }
}

// A provider for our example HTTP capabilities.
pub struct HttpApiProvider;

impl CapabilityProvider for HttpApiProvider {
    fn get_capabilities(&self) -> Vec<Arc<dyn Capability>> {
        vec![
            Arc::new(HttpGetCapability::new(
                "com.bizdata.eu:v1.financial-report",
                "1.0.0",
                "https://api.bizdata.eu/v1/reports"
            )),
            Arc::new(HttpGetCapability::new(
                "com.tech-analysis.eu:v1.spec-breakdown",
                "1.0.0",
                "https://api.tech-analysis.eu/v1/specs"
            )),
        ]
    }
}
