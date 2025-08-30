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
//! use rtfs_compiler::ccos::arbiter::{
//!     ArbiterConfig,
//!     ArbiterFactory,
//!     ArbiterEngine,
//! };
//! use rtfs_compiler::ccos::intent_graph::core::IntentGraph;
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
pub mod dummy_arbiter;
pub mod delegating_arbiter;
pub mod legacy_arbiter;
pub mod llm_provider;
pub mod llm_arbiter;
pub mod template_arbiter;
pub mod hybrid_arbiter;
pub mod prompt;
pub mod plan_generation;

// Re-export main types for easy access
pub use arbiter_config::ArbiterConfig;
pub use arbiter_engine::ArbiterEngine;
pub use arbiter_factory::ArbiterFactory;
pub use dummy_arbiter::DummyArbiter;
pub use delegating_arbiter::DelegatingArbiter;
pub use legacy_arbiter::Arbiter;
pub use llm_provider::{LlmProvider, LlmProviderConfig, StubLlmProvider, LlmProviderFactory};
pub use llm_arbiter::LlmArbiter;
pub use template_arbiter::TemplateArbiter;
pub use hybrid_arbiter::HybridArbiter;
pub use prompt::{PromptManager, PromptConfig};
pub use plan_generation::{PlanGenerationProvider, PlanGenerationResult, StubPlanGenerationProvider};

// Re-export configuration types
pub use arbiter_config::{
    ArbiterEngineType,
    LlmConfig,
    DelegationConfig,
    CapabilityConfig,
    SecurityConfig,
    TemplateConfig,
    LlmProviderType,
    MarketplaceType,
    CacheConfig,
    IntentPattern,
    PlanTemplate,
    FallbackBehavior,
};
