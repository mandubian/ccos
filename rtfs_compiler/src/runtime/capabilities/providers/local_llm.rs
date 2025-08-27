//! Local LLM example provider using the runtime::capability_provider API.

use std::collections::HashMap;

use crate::runtime::capabilities::provider::{
    CapabilityProvider,
    CapabilityDescriptor,
    SecurityRequirements,
    ProviderMetadata,
    ExecutionContext,
    HealthStatus,
    NetworkAccess,
    ResourceLimits,
};
use crate::runtime::{RuntimeResult, RuntimeError, Value};
use crate::ast::{TypeExpr, PrimitiveType, MapKey};

#[derive(Debug, Default)]
pub struct LocalLlmProvider;

impl LocalLlmProvider {
    fn synthesize_descriptor() -> CapabilityDescriptor {
        CapabilityDescriptor {
            id: "com.local-llm:v1.synthesize".to_string(),
            description: "Synthesizes an analysis from provided docs".to_string(),
            // Use a simple string -> map signature; non-string types are permissive in validator
            capability_type: CapabilityDescriptor::constrained_function_type(
                vec![CapabilityDescriptor::non_empty_string_type()],
                TypeExpr::Primitive(PrimitiveType::String),
                None
            ),
            security_requirements: SecurityRequirements {
                permissions: vec![],
                requires_microvm: false,
                resource_limits: ResourceLimits { max_memory: None, max_cpu_time: None, max_disk_space: None },
                network_access: NetworkAccess::None,
            },
            metadata: HashMap::new(),
        }
    }

    fn draft_descriptor() -> CapabilityDescriptor {
        CapabilityDescriptor {
            id: "com.local-llm:v1.draft-document".to_string(),
            description: "Drafts a document from context".to_string(),
            capability_type: CapabilityDescriptor::constrained_function_type(
                vec![CapabilityDescriptor::non_empty_string_type()],
                TypeExpr::Primitive(PrimitiveType::String),
                None
            ),
            security_requirements: SecurityRequirements {
                permissions: vec![],
                requires_microvm: false,
                resource_limits: ResourceLimits { max_memory: None, max_cpu_time: None, max_disk_space: None },
                network_access: NetworkAccess::None,
            },
            metadata: HashMap::new(),
        }
    }
}

impl CapabilityProvider for LocalLlmProvider {
    fn provider_id(&self) -> &str { "com.local-llm" }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![Self::synthesize_descriptor(), Self::draft_descriptor()]
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        _inputs: &Value,
        _context: &ExecutionContext,
    ) -> RuntimeResult<Value> {
        match capability_id {
            "com.local-llm:v1.synthesize" => {
                let mut result_map: HashMap<MapKey, Value> = HashMap::new();
                result_map.insert(MapKey::String("analysis-document".to_string()), Value::String("This is a synthesized analysis.".to_string()));
                result_map.insert(MapKey::String("key-takeaways".to_string()), Value::String("Key takeaway: Project Phoenix is a competitor.".to_string()));
                Ok(Value::Map(result_map))
            }
            "com.local-llm:v1.draft-document" => {
                Ok(Value::String("This is a draft press release.".to_string()))
            }
            other => Err(RuntimeError::Generic(format!("Unknown capability: {}", other)))
        }
    }

    fn initialize(&mut self, _config: &crate::runtime::capabilities::provider::ProviderConfig) -> Result<(), String> {
        Ok(())
    }

    fn health_check(&self) -> HealthStatus { HealthStatus::Healthy }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "Local LLM Provider".into(),
            version: "1.0.0".into(),
            description: "Sample local LLM capabilities for demos".into(),
            author: "CCOS".into(),
            license: Some("Apache-2.0".into()),
            dependencies: vec![],
        }
    }
}
