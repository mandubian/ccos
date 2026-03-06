//! Autonoetic Gateway — core daemon crate.
//!
//! Re-exports shared types from `autonoetic_types` and provides
//! Gateway-specific logic: config loading, agent scanning, runtime
//! lock resolution, and sandbox management.

pub mod agent;
pub mod artifact_store;
pub mod causal_chain;
pub mod config;
pub mod llm;
pub mod log_redaction;
pub mod policy;
pub mod router;
pub mod runtime;
pub mod runtime_lock;
pub mod sandbox;
pub mod server;
pub mod vault;

pub use agent::scan_agents;
pub use artifact_store::ArtifactStore;
pub use autonoetic_types::agent::AgentMeta;
pub use autonoetic_types::config::GatewayConfig;
pub use autonoetic_types::runtime_lock::RuntimeLock;
pub use causal_chain::CausalLogger;
pub use llm::{build_driver, LlmDriver};
pub use policy::PolicyEngine;
pub use router::{JsonRpcRequest, JsonRpcResponse, JsonRpcRouter};
pub use runtime_lock::resolve_runtime_lock;
pub use sandbox::SandboxRunner;
pub use server::GatewayServer;
pub use vault::Vault;
