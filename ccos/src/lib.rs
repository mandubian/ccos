// CCOS Library
// Cognitive Computing Operating System - orchestration layer built on RTFS

/// Quiet-mode-aware printing macros
/// When CCOS_QUIET=1|true|on, suppress console output (for TUI mode)
#[macro_export]
macro_rules! ccos_println {
    ($($arg:tt)*) => {{
        let quiet = std::env::var("CCOS_QUIET")
            .map(|v| { let v = v.to_lowercase(); v == "1" || v == "true" || v == "on" })
            .unwrap_or(false);
        if !quiet {
            eprintln!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! ccos_eprintln {
    ($($arg:tt)*) => {{
        let quiet = std::env::var("CCOS_QUIET")
            .map(|v| { let v = v.to_lowercase(); v == "1" || v == "true" || v == "on" })
            .unwrap_or(false);
        if !quiet {
            eprintln!($($arg)*);
        }
    }};
}

// Include the CCOS module structure
// AgentRegistry migration: agent module removed (deprecated, use CapabilityMarketplace with :kind :agent)
pub mod approval;
pub mod cognitive_engine;
pub use cognitive_engine as arbiter; // Backward compatibility alias
pub mod archivable_types;
pub mod budget;
pub mod catalog;
pub mod causal_chain;
pub mod checkpoint_archive;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod event_sink;
pub mod execution_context;
pub mod governance_judge;
pub mod governance_kernel;
pub mod intent_archive;
pub mod intent_graph;
pub mod intent_storage;
pub mod introspect;
pub mod learning;
pub mod mcp;
pub mod orchestrator;
pub mod plan_archive;
pub mod planner;
pub mod rtfs_bridge;
pub mod secrets;
pub mod security_policies;
pub mod storage;
pub mod storage_backends;
pub mod synthesis;
#[cfg(feature = "tui")]
pub mod tui;
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
pub mod hints;
pub mod host;
pub mod observability;
pub mod ops;
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

// Utilities
pub mod utils;

// Example helpers
#[cfg(feature = "tui")]
pub mod planner_viz_common;

// Re-export some arbiter sub-modules for historic import paths
pub use crate::cognitive_engine::delegating_engine;
pub use crate::cognitive_engine::engine;

// CCOS core implementation (formerly mod.rs)
pub mod ccos_core;

// Re-export the main CCOS system
pub use crate::ccos_core::PlanAutoRepairOptions;
pub use crate::ccos_core::CCOS;
pub use crate::types::ExecutionResult;
