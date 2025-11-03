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
pub mod legacy_arbiter;
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
pub use legacy_arbiter::Arbiter;
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
