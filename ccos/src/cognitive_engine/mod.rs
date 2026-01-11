//! CCOS Cognitive Engine Module (formerly Arbiter)
//!
//! This module contains the Cognitive Engine implementation for CCOS, which is responsible for
//! converting natural language requests into structured intents and executable RTFS plans.
//!
//! ## Architecture
//!
//! The Cognitive Engine module provides:
//! - **Configuration-driven architecture**: TOML-based configuration for different engine types
//! - **Multiple engine types**: Template, LLM, Delegating, Hybrid, and Dummy implementations
//! - **Factory pattern**: Dynamic creation of engine instances based on configuration
//! - **Standalone operation**: Can run independently of full CCOS
//! - **AI-first design**: Optimized for AI systems using RTFS
//!
//! ## Usage
//!
//! Note: the primary CCOS runtime path uses [`DelegatingCognitiveEngine`]. Alternate/older
//! engines are available under [`legacy`].
//!
//! ```rust,no_run
//! use ccos::cognitive_engine::{
//!     CognitiveEngineConfig,
//!     legacy::CognitiveEngineFactory,
//!     CognitiveEngine,
//! };
//! use ccos::intent_graph::core::IntentGraph;
//! use std::sync::{Arc, Mutex};
//!
//! // Create a Tokio runtime for async operations in this example
//! let rt = tokio::runtime::Runtime::new().unwrap();
//! rt.block_on(async {
//!     // Create engine from configuration
//!     let config = CognitiveEngineConfig::default();
//!     let intent_graph = Arc::new(Mutex::new(IntentGraph::new().unwrap()));
//!
//!     // Instantiate engine
//!     let engine = CognitiveEngineFactory::create_engine(config, intent_graph, None)
//!         .await
//!         .expect("failed to create engine");
//!
//!     // Process natural language request
//!     let _result = engine
//!         .process_natural_language("Analyze sentiment", None)
//!         .await
//!         .expect("engine processing failed");
//! });
//! ```

pub mod config;
pub mod delegating_engine;
pub mod delegation_analysis;
pub mod engine;
pub mod intent_parsing;
pub mod learning_augmenter;
pub mod legacy;
pub mod llm_provider;
pub mod plan_generation;
pub mod prompt;

// Re-export main types for easy access
pub use config::CognitiveEngineConfig;
pub use delegating_engine::DelegatingCognitiveEngine;
pub use engine::CognitiveEngine;
pub use llm_provider::{LlmProvider, LlmProviderConfig, LlmProviderFactory, StubLlmProvider};
pub use plan_generation::{
    PlanGenerationProvider, PlanGenerationResult, StubPlanGenerationProvider,
};
pub use prompt::{PromptConfig, PromptManager};

// Re-export configuration types
pub use config::{
    CacheConfig, CapabilityConfig, CognitiveEngineType, DelegationConfig, FallbackBehavior,
    IntentPattern, LlmConfig, LlmProviderType, MarketplaceType, PlanTemplate, SecurityConfig,
    TemplateConfig,
};

// Re-export new modules
pub use delegation_analysis::{DelegationAnalysis, DelegationAnalyzer};

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
