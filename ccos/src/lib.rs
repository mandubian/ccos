// CCOS Library
// Cognitive Computing Operating System - orchestration layer built on RTFS

// Include the CCOS module structure
pub mod agent;
pub mod arbiter;
pub mod archivable_types;
pub mod catalog;
pub mod causal_chain;
pub mod checkpoint_archive;
pub mod discovery;
pub mod event_sink;
pub mod execution_context;
pub mod governance_kernel;
pub mod intent_archive;
pub mod intent_graph;
pub mod intent_storage;
pub mod orchestrator;
pub mod plan_archive;
pub mod planner;
pub mod rtfs_bridge;
pub mod security_policies;
pub mod storage;
pub mod storage_backends;
pub mod synthesis;
pub mod types;
pub mod wm_integration;

// Delegation and execution stack
pub mod adaptive_threshold;
pub mod delegation;
pub mod delegation_keys;
pub mod delegation_l4;
pub mod local_models;
pub mod remote_models;

// Infrastructure
pub mod caching;

// Capability system
pub mod capabilities;
pub mod capability_marketplace;
pub mod environment;
pub mod examples_common;
pub mod host;
pub mod observability;
pub mod prelude;
pub mod state_provider;
pub mod streaming;

// Advanced components
pub mod context_horizon;
pub mod subconscious;

// Working Memory
pub mod working_memory;

// Runtime service
pub mod runtime_service;

// Re-export some arbiter sub-modules for historic import paths
pub use crate::arbiter::arbiter_engine;
pub use crate::arbiter::delegating_arbiter;

// CCOS core implementation (formerly mod.rs)
pub mod ccos_core;

// Re-export the main CCOS system
pub use crate::ccos_core::PlanAutoRepairOptions;
pub use crate::ccos_core::CCOS;
pub use crate::types::ExecutionResult;
