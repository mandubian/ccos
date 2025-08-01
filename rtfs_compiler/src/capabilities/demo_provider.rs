//! rtfs_compiler/src/capabilities/demo_provider.rs

use crate::runtime::capability_provider::{CapabilityProvider, CapabilityDescriptor, ProviderMetadata, SecurityRequirements, NetworkAccess, ResourceLimits, ProviderConfig, HealthStatus, ExecutionContext};
use crate::runtime::capability::{Capability, CapabilitySpec};
use crate::ast::{Value, RuntimeError};
use std::sync::Arc;
use async_trait::async_trait;
use std::collections::HashMap;

use super::http_api::HttpApiProvider;
use super::local_llm::LocalLlmProvider;
use super::collaboration::CollaborationProvider;

/// A provider that aggregates all capabilities for the README demo scenario.
#[derive(Debug)]
pub struct DemoProvider {
    providers: Vec<Box<dyn CapabilityProvider>>,
}

impl DemoProvider {
    pub fn new() -> Self {
        Self {
            providers: vec![
                Box::new(HttpApiProvider),
                Box::new(LocalLlmProvider),
                Box::new(CollaborationProvider),
            ],
        }
    }
}

impl CapabilityProvider for DemoProvider {
    fn provider_id(&self) -> &str {
        "ccos.demo-provider"
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "Demo Scenario Provider".to_string(),
            version: "1.0.0".to_string(),
            description: "Aggregates all capabilities needed for the CCOS README demo.".to_string(),
            author: "CCOS AI".to_string(),
            license: Some("Apache-2.0".to_string()),
            dependencies: vec![
                "com.bizdata.eu:v1.financial-report".to_string(),
                "com.tech-analysis.eu:v1.spec-breakdown".to_string(),
                "com.local-llm:v1.synthesize".to_string(),
                "com.local-llm:v1.draft-document".to_string(),
                "com.collaboration:v1.slack-post".to_string(),
                "com.collaboration:v1.send-email".to_string(),
            ],
        }
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.providers.iter().flat_map(|p| p.list_capabilities()).collect()
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, RuntimeError> {
        for provider in &self.providers {
            if provider.list_capabilities().iter().any(|c| c.id == capability_id) {
                return provider.execute_capability(capability_id, inputs, context);
            }
        }
        Err(RuntimeError::CapabilityNotFound(capability_id.to_string()))
    }

    fn initialize(&mut self, config: &ProviderConfig) -> Result<(), String> {
        for provider in &mut self.providers {
            provider.initialize(config)?;
        }
        Ok(())
    }

    fn health_check(&self) -> HealthStatus {
        for provider in &self.providers {
            if let HealthStatus::Unhealthy(reason) = provider.health_check() {
                return HealthStatus::Unhealthy(format!("Underlying provider {} is unhealthy: {}", provider.provider_id(), reason));
            }
        }
        HealthStatus::Healthy
    }
}

// Note: The individual provider implementations need to be adjusted to implement the
// CapabilityProvider trait correctly. For this file, we assume they do.
// The `get_capabilities` method in the individual provider files should be changed to
// `list_capabilities` and return `Vec<CapabilityDescriptor>`.
// The `execute` method should be changed to `execute_capability`.
