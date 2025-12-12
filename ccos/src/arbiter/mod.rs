//! CCOS Arbiter Module
//!
//! This module contains the Arbiter implementation for CCOS, which is responsible for
//! converting natural language requests into structured intents and executable RTFS plans.
//!
//! ## Architecture
//!
//! The Arbiter module provides:
//! - **Configuration-driven architecture**: TOML-based configuration for different arbiter types
//! - **Multiple engine types**: Template, LLM, Delegating, Hybrid, and Dummy implementations
//! - **Factory pattern**: Dynamic creation of arbiter instances based on configuration
//! - **Standalone operation**: Can run independently of full CCOS
//! - **AI-first design**: Optimized for AI systems using RTFS
//!
//! ## Usage
//!
//! ```rust,no_run
//! use ccos::arbiter::{
//!     ArbiterConfig,
//!     ArbiterFactory,
//!     ArbiterEngine,
//! };
//! use ccos::intent_graph::core::IntentGraph;
//! use std::sync::{Arc, Mutex};
//!
//! // Create a Tokio runtime for async operations in this example
//! let rt = tokio::runtime::Runtime::new().unwrap();
//! rt.block_on(async {
//!     // Create arbiter from configuration
//!     let config = ArbiterConfig::default();
//!     let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
//!
//!     // Instantiate arbiter
//!     let arbiter = ArbiterFactory::create_arbiter(config, intent_graph, None)
//!         .await
//!         .expect("failed to create arbiter");
//!
//!     // Process natural language request
//!     let _result = arbiter
//!         .process_natural_language("Analyze sentiment", None)
//!         .await
//!         .expect("arbiter processing failed");
//! });
//! ```

pub mod arbiter_config;
pub mod arbiter_engine;
pub mod arbiter_factory;
pub mod delegating_arbiter;
pub mod dummy_arbiter;
pub mod hybrid_arbiter;
pub mod learning_augmenter;
pub mod llm_arbiter;
pub mod llm_provider;
pub mod plan_generation;
pub mod prompt;
pub mod template_arbiter;

// Re-export main types for easy access
pub use arbiter_config::ArbiterConfig;
pub use arbiter_engine::ArbiterEngine;
pub use arbiter_factory::ArbiterFactory;
pub use delegating_arbiter::DelegatingArbiter;
pub use dummy_arbiter::DummyArbiter;
pub use hybrid_arbiter::HybridArbiter;
pub use llm_arbiter::LlmArbiter;
pub use llm_provider::{LlmProvider, LlmProviderConfig, LlmProviderFactory, StubLlmProvider};
pub use plan_generation::{
    PlanGenerationProvider, PlanGenerationResult, StubPlanGenerationProvider,
};
pub use prompt::{PromptConfig, PromptManager};
pub use template_arbiter::TemplateArbiter;

// Re-export configuration types
pub use arbiter_config::{
    ArbiterEngineType, CacheConfig, CapabilityConfig, DelegationConfig, FallbackBehavior,
    IntentPattern, LlmConfig, LlmProviderType, MarketplaceType, PlanTemplate, SecurityConfig,
    TemplateConfig,
};

/// Get a default LLM provider from environment variables.
/// Tries OPENAI_API_KEY, then ANTHROPIC_API_KEY, then OPENROUTER_API_KEY.
/// Returns None if no API key is configured.
pub async fn get_default_llm_provider() -> Option<Box<dyn LlmProvider + Send + Sync>> {
    // Try OpenAI first
    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::OpenAI,
            model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            api_key: Some(api_key),
            base_url: std::env::var("OPENAI_BASE_URL").ok(),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            timeout_seconds: None,
            retry_config: Default::default(),
        };
        if let Ok(provider) = LlmProviderFactory::create_provider(config).await {
            return Some(provider);
        }
    }

    // Try Anthropic
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::Anthropic,
            model: std::env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-3-haiku-20240307".to_string()),
            api_key: Some(api_key),
            base_url: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            timeout_seconds: None,
            retry_config: Default::default(),
        };
        if let Ok(provider) = LlmProviderFactory::create_provider(config).await {
            return Some(provider);
        }
    }

    // Try OpenRouter
    if let Ok(api_key) = std::env::var("OPENROUTER_API_KEY") {
        let config = LlmProviderConfig {
            provider_type: LlmProviderType::OpenAI, // OpenRouter uses OpenAI-compatible API
            model: std::env::var("OPENROUTER_MODEL")
                .unwrap_or_else(|_| "anthropic/claude-3-haiku".to_string()),
            api_key: Some(api_key),
            base_url: Some("https://openrouter.ai/api/v1".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            timeout_seconds: None,
            retry_config: Default::default(),
        };
        if let Ok(provider) = LlmProviderFactory::create_provider(config).await {
            return Some(provider);
        }
    }

    None
}
