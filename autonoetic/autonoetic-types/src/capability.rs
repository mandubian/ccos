//! Capability enums for agent permission declarations.
//!
//! Capability categories:
//! - **SandboxFunctions**: MCP tool access by prefix (web.*, sandbox.*)
//! - **ReadAccess**: Read content, memory, knowledge (includes search)
//! - **WriteAccess**: Write content, memory, knowledge (includes share)
//! - **CodeExecution**: Execute scripts in sandbox
//! - **NetworkAccess**: Make HTTP requests
//! - **AgentSpawn**: Create child agent sessions
//! - **AgentMessage**: Send messages to other agents
//! - **BackgroundReevaluation**: Periodic wake-ups

use serde::{Deserialize, Serialize};

/// A typed capability that an Agent may request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Capability {
    /// MCP tool access by prefix.
    /// Controls which external tools (from MCP servers) can be invoked.
    /// Example: ["web.", "sandbox.exec"] allows web tools and sandbox.exec.
    SandboxFunctions { allowed: Vec<String> },

    /// Read access to all storage: content, memory, knowledge.
    /// Includes search operations.
    /// The `scopes` field restricts which paths/areas can be read.
    ReadAccess { scopes: Vec<String> },

    /// Write access to all storage: content, memory, knowledge.
    /// Includes sharing with other agents.
    /// The `scopes` field restricts which paths/areas can be written.
    WriteAccess { scopes: Vec<String> },

    /// HTTP/network access - escapes the sandbox boundary.
    /// Use ["*"] for all hosts, or specific domains.
    NetworkAccess { hosts: Vec<String> },

    /// Create child agent sessions.
    /// The `max_children` field limits concurrent children.
    AgentSpawn { max_children: u32 },

    /// Send messages to other agents.
    AgentMessage { patterns: Vec<String> },

    /// Periodic wake-ups for background processing.
    BackgroundReevaluation {
        min_interval_secs: u64,
        allow_reasoning: bool,
    },

    /// Execute scripts/code in the sandbox.
    /// The `patterns` field limits which commands can be run.
    CodeExecution { patterns: Vec<String> },
}
