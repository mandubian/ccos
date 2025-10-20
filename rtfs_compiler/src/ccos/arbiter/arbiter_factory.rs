use std::sync::{Arc, Mutex};

use crate::runtime::error::RuntimeError;

use super::arbiter_config::{ArbiterConfig, ArbiterEngineType};
use super::arbiter_engine::ArbiterEngine;
use super::delegating_arbiter::DelegatingArbiter;
use super::dummy_arbiter::DummyArbiter;
use super::hybrid_arbiter::HybridArbiter;
use super::llm_arbiter::LlmArbiter;
use super::template_arbiter::TemplateArbiter;
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::ccos::intent_graph::IntentGraph;

/// Factory for creating different types of arbiters based on configuration.
pub struct ArbiterFactory;

impl ArbiterFactory {
    /// Create an arbiter based on the provided configuration.
    pub async fn create_arbiter(
        config: ArbiterConfig,
        intent_graph: Arc<Mutex<IntentGraph>>,
        _capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn ArbiterEngine>, RuntimeError> {
        // Validate the configuration first
        config.validate().map_err(|errors| {
            RuntimeError::Generic(format!("Invalid arbiter config: {}", errors.join(", ")))
        })?;

        match config.engine_type {
            ArbiterEngineType::Dummy => {
                let arbiter = DummyArbiter::new(config, intent_graph);
                Ok(Box::new(arbiter))
            }
            ArbiterEngineType::Template => {
                let template_config = config.template_config.as_ref().ok_or_else(|| {
                    RuntimeError::Generic(
                        "Template config required for template arbiter".to_string(),
                    )
                })?;
                let arbiter = TemplateArbiter::new(template_config.clone(), intent_graph)?;
                Ok(Box::new(arbiter))
            }
            ArbiterEngineType::Llm => {
                let arbiter = LlmArbiter::new(config, intent_graph).await?;
                Ok(Box::new(arbiter))
            }
            ArbiterEngineType::Delegating => {
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
                let arbiter = DelegatingArbiter::new(
                    llm_config.clone(),
                    delegation_config.clone(),
                    capability_marketplace,
                    intent_graph,
                )
                .await?;
                Ok(Box::new(arbiter))
            }
            ArbiterEngineType::Hybrid => {
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
    pub fn create_dummy_arbiter(intent_graph: Arc<Mutex<IntentGraph>>) -> Box<dyn ArbiterEngine> {
        let config = ArbiterConfig::default();
        let arbiter = DummyArbiter::new(config, intent_graph);
        Box::new(arbiter)
    }

    /// Create an arbiter from a configuration file.
    pub async fn create_arbiter_from_file(
        config_path: &str,
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn ArbiterEngine>, RuntimeError> {
        let config = ArbiterConfig::from_file(config_path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to load config from {}: {}", config_path, e))
        })?;

        Self::create_arbiter(config, intent_graph, capability_marketplace).await
    }

    /// Create an arbiter from environment variables.
    pub async fn create_arbiter_from_env(
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Option<Arc<CapabilityMarketplace>>,
    ) -> Result<Box<dyn ArbiterEngine>, RuntimeError> {
        let config = ArbiterConfig::from_env()
            .map_err(|e| RuntimeError::Generic(format!("Failed to load config from env: {}", e)))?;

        Self::create_arbiter(config, intent_graph, capability_marketplace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::intent_graph::IntentGraphConfig;

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
        let config = ArbiterConfig::default();

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
        let mut config = ArbiterConfig::default();
        config.engine_type = ArbiterEngineType::Llm;
        // Missing llm_config should cause validation to fail

        let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None).await;
        assert!(arbiter.is_err());
        assert!(arbiter.is_err());
    }
}
