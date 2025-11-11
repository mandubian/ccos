// Minimal stubs for CCOS types used by RTFS
// These are placeholders - CCOS provides full implementations

// Note: IsolationLevel is now defined in security.rs as an RTFS-local type
// This stub enum has been removed - use crate::runtime::security::IsolationLevel instead

/// Agent discovery query
#[derive(Debug, Clone)]
pub struct SimpleDiscoveryQuery {
    pub capability_id: Option<String>,
    pub version_constraint: Option<String>,
    pub agent_id: Option<String>,
    pub discovery_tags: Option<Vec<String>>,
    pub discovery_query: Option<String>,
    pub limit: Option<usize>,
}

/// Agent discovery options
#[derive(Debug, Clone)]
pub struct SimpleDiscoveryOptions {
    pub timeout_ms: Option<u64>,
    pub cache_policy: Option<String>,
    pub include_offline: Option<bool>,
    pub max_results: Option<usize>,
}

/// Agent card
#[derive(Debug, Clone)]
pub struct SimpleAgentCard {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub agent_id: Option<String>,
    pub version: Option<String>,
    pub endpoint: Option<String>,
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// Execution result type alias
pub type ExecutionResult<T> = Result<T, crate::runtime::error::RuntimeError>;

/// CCOS ExecutionResult struct (stub for standalone RTFS)
#[derive(Debug, Clone)]
pub struct ExecutionResultStruct {
    pub success: bool,
    pub value: crate::runtime::values::Value,
    pub metadata: std::collections::HashMap<String, crate::runtime::values::Value>,
}

impl Default for ExecutionResultStruct {
    fn default() -> Self {
        ExecutionResultStruct {
            success: true,
            value: crate::runtime::values::Value::Nil,
            metadata: std::collections::HashMap::new(),
        }
    }
}

/// Conflict resolution policy for execution contexts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    KeepExisting,
    Overwrite,
    Merge,
}

/// Cache policy for agent discovery
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleCachePolicy {
    UseCache,
    NoCache,
    BypassCache,
    RefreshCache,
}
