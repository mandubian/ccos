use std::sync::{Arc, Mutex};

use rtfs::runtime::error::RuntimeError;

use crate::capability_marketplace::CapabilityMarketplace;
use crate::cognitive_engine::config::{CognitiveEngineConfig, CognitiveEngineType};
use crate::cognitive_engine::engine::CognitiveEngine;
use crate::cognitive_engine::DelegatingCognitiveEngine;
use crate::types::IntentGraph;

use super::dummy_arbiter::DummyArbiter;
use super::hybrid_arbiter::HybridArbiter;
use super::llm_arbiter::LlmArbiter;
use super::template_arbiter::TemplateArbiter;

/// Factory for creating different types of arbiters based on configuration.
pub struct ArbiterFactory;

impl ArbiterFactory {
    /// Create an arbiter based on the provided configuration.
    pub async fn create_arbiter(
        config: CognitiveEngineConfig,
        intent_graph: Arc<Mutex<IntentGraph>>,
        _capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn CognitiveEngine>, RuntimeError> {
        // Validate the configuration first
        config.validate().map_err(|errors| {
            RuntimeError::Generic(format!("Invalid arbiter config: {}", errors.join(", ")))
        })?;

        match config.engine_type {
            CognitiveEngineType::Dummy => {
                let arbiter = DummyArbiter::new(config, intent_graph);
                Ok(Box::new(arbiter))
            }
            CognitiveEngineType::Template => {
                let template_config = config.template_config.as_ref().ok_or_else(|| {
                    RuntimeError::Generic(
                        "Template config required for template arbiter".to_string(),
                    )
                })?;
                let arbiter = TemplateArbiter::new(template_config.clone(), intent_graph)?;
                Ok(Box::new(arbiter))
            }
            CognitiveEngineType::Llm => {
                let arbiter = LlmArbiter::new(config, intent_graph).await?;
                Ok(Box::new(arbiter))
            }
            CognitiveEngineType::Delegating => {
                let llm_config = config.llm_config.as_ref().ok_or_else(|| {
                    RuntimeError::Generic("Delegating engine requires llm_config".to_string())
                })?;
                let delegation_config = config.delegation_config.as_ref().ok_or_else(|| {
                    RuntimeError::Generic(
                        "Delegating engine requires delegation_config".to_string(),
                    )
                })?;
                let capability_marketplace = _capability_marketplace.ok_or_else(|| {
                    RuntimeError::Generic(
                        "Delegating engine requires capability_marketplace".to_string(),
                    )
                })?;
                let arbiter = DelegatingCognitiveEngine::new(
                    llm_config.clone(),
                    delegation_config.clone(),
                    capability_marketplace,
                    intent_graph,
                )
                .await?;
                Ok(Box::new(arbiter))
            }
            CognitiveEngineType::Hybrid => {
                let template_config = config.template_config.as_ref().ok_or_else(|| {
                    RuntimeError::Generic("Hybrid engine requires template_config".to_string())
                })?;
                let llm_config = config.llm_config.as_ref().ok_or_else(|| {
                    RuntimeError::Generic("Hybrid engine requires llm_config".to_string())
                })?;
                let arbiter =
                    HybridArbiter::new(template_config.clone(), llm_config.clone(), intent_graph)
                        .await?;
                Ok(Box::new(arbiter))
            }
        }
    }

    /// Create a dummy arbiter for testing purposes.
    pub fn create_dummy_arbiter(intent_graph: Arc<Mutex<IntentGraph>>) -> Box<dyn CognitiveEngine> {
        let config = CognitiveEngineConfig::default();
        let arbiter = DummyArbiter::new(config, intent_graph);
        Box::new(arbiter)
    }

    /// Create an arbiter from a configuration file.
    pub async fn create_arbiter_from_file(
        config_path: &str,
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn CognitiveEngine>, RuntimeError> {
        let config = CognitiveEngineConfig::from_file(config_path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to load config from {}: {}", config_path, e))
        })?;

        Self::create_arbiter(config, intent_graph, capability_marketplace).await
    }

    /// Create an arbiter from environment variables.
    pub async fn create_arbiter_from_env(
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn CognitiveEngine>, RuntimeError> {
        let config = CognitiveEngineConfig::from_env()
            .map_err(|e| RuntimeError::Generic(format!("Failed to load config from env: {}", e)))?;

        Self::create_arbiter(config, intent_graph, capability_marketplace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_graph::IntentGraphConfig;

    #[tokio::test]
    async fn test_create_dummy_arbiter() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
        let arbiter = ArbiterFactory::create_dummy_arbiter(intent_graph);

        // Test that it can process a simple request
        let result = arbiter.process_natural_language("Hello", None).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_create_arbiter_with_config() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
        let config = CognitiveEngineConfig::default();

        let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await;
        assert!(arbiter.is_ok());
    }

    #[tokio::test]
    async fn test_create_arbiter_invalid_config() {
        let intent_graph = Arc::new(Mutex::new(
            IntentGraph::new_async(IntentGraphConfig::default())
                .await
                .unwrap(),
        ));
        let mut config = CognitiveEngineConfig::default();
        config.engine_type = CognitiveEngineType::Llm;
        // Missing llm_config should cause validation to fail

        let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await;
        assert!(arbiter.is_err());
        assert!(arbiter.is_err());
    }
}
