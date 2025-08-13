//! Secure Runtime Configuration
//!
//! This module defines security policies and execution contexts for RTFS programs.

use std::collections::HashSet;
use crate::ccos::execution_context::IsolationLevel;

/// Security levels for RTFS execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Maximum security - only pure functions allowed
    Pure,
    /// Controlled access - limited capabilities with permission checks
    Controlled,
    /// Full access - all capabilities available (for trusted code)
    Full,
}

/// Execution context for RTFS programs
#[derive(Debug, Clone)]
pub struct RuntimeContext {
    /// Security level for this execution
    pub security_level: SecurityLevel,
    /// Allowed capabilities for this context
    pub allowed_capabilities: HashSet<String>,
    /// Whether to run dangerous operations in microVM
    pub use_microvm: bool,
    /// Maximum execution time (milliseconds)
    pub max_execution_time: Option<u64>,
    /// Maximum memory usage (bytes)
    pub max_memory_usage: Option<u64>,
    /// Whether to log all capability calls
    pub log_capability_calls: bool,
    /// Isolation policy: which step isolation levels are allowed
    pub allow_inherit_isolation: bool,
    pub allow_isolated_isolation: bool,
    pub allow_sandboxed_isolation: bool,
    /// When true, attach a sanitized, read-only execution context snapshot to capability calls
    pub expose_readonly_context: bool,
    /// Allowlist of capability IDs that may receive the read-only context snapshot
    pub exposed_context_caps: HashSet<String>,
    /// Allowlist of capability ID prefixes (e.g., "ccos.ai.") eligible for context exposure
    pub exposed_context_prefixes: Vec<String>,
    /// Allowlist of capability "tags" eligible for context exposure (matched against capability metadata)
    pub exposed_context_tags: HashSet<String>,
}

impl RuntimeContext {
    /// Create a pure (secure) runtime context
    pub fn pure() -> Self {
        Self {
            security_level: SecurityLevel::Pure,
            allowed_capabilities: HashSet::new(),
            use_microvm: false,
            max_execution_time: Some(1000), // 1 second
            max_memory_usage: Some(16 * 1024 * 1024), // 16MB
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: HashSet::new(),
        }
    }
    
    /// Create a controlled runtime context with specific capabilities
    pub fn controlled(allowed_capabilities: Vec<String>) -> Self {
        Self {
            security_level: SecurityLevel::Controlled,
            allowed_capabilities: allowed_capabilities.into_iter().collect(),
            use_microvm: true,
            max_execution_time: Some(5000), // 5 seconds
            max_memory_usage: Some(64 * 1024 * 1024), // 64MB
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: HashSet::new(),
        }
    }
    
    /// Create a full-access runtime context (for trusted code)
    pub fn full() -> Self {
        Self {
            security_level: SecurityLevel::Full,
            allowed_capabilities: HashSet::new(), // Empty means all allowed
            use_microvm: false,
            max_execution_time: None,
            max_memory_usage: None,
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: HashSet::new(),
        }
    }
    
    /// Check if a capability is allowed in this context
    pub fn is_capability_allowed(&self, capability_id: &str) -> bool {
        match self.security_level {
            SecurityLevel::Pure => false, // No capabilities allowed
            SecurityLevel::Controlled => self.allowed_capabilities.contains(capability_id),
            SecurityLevel::Full => true, // All capabilities allowed
        }
    }
    
    /// Check if dangerous operations should run in microVM
    pub fn requires_microvm(&self, capability_id: &str) -> bool {
        if !self.use_microvm {
            return false;
        }
        
        // Define which capabilities require microVM execution
        let dangerous_capabilities = [
            "ccos.io.open-file",
            "ccos.io.read-line",
            "ccos.io.write-line",
            "ccos.io.close-file",
            "ccos.network.http-fetch",
            "ccos.system.get-env",
        ];
        
        dangerous_capabilities.contains(&capability_id)
    }

    /// Check if the requested step isolation level is permitted by policy
    pub fn is_isolation_allowed(&self, level: &IsolationLevel) -> bool {
        match level {
            IsolationLevel::Inherit => self.allow_inherit_isolation,
            IsolationLevel::Isolated => self.allow_isolated_isolation,
            IsolationLevel::Sandboxed => self.allow_sandboxed_isolation,
        }
    }

    /// Check whether a capability may receive the sanitized context snapshot (exact ID allowlist only)
    pub fn is_context_exposure_allowed(&self, capability_id: &str) -> bool {
        self.expose_readonly_context && self.exposed_context_caps.contains(capability_id)
    }

    /// Check whether a capability may receive the sanitized context snapshot using
    /// dynamic policies: exact ID allowlist, prefix allowlist, or tag allowlist.
    /// `capability_tags` is derived from capability metadata (e.g., manifest.metadata["tags"]).
    pub fn is_context_exposure_allowed_for(&self, capability_id: &str, capability_tags: Option<&[String]>) -> bool {
        if !self.expose_readonly_context {
            return false;
        }
        if self.exposed_context_caps.contains(capability_id) {
            return true;
        }
        if self.exposed_context_prefixes.iter().any(|p| capability_id.starts_with(p)) {
            return true;
        }
        if let Some(tags) = capability_tags {
            if tags.iter().any(|t| self.exposed_context_tags.contains(t)) {
                return true;
            }
        }
        false
    }

    /// Enable read-only context exposure for a set of capability IDs (builder-style)
    pub fn with_context_exposure(mut self, capability_ids: &[&str]) -> Self {
        self.expose_readonly_context = true;
        for id in capability_ids { self.exposed_context_caps.insert((*id).to_string()); }
        self
    }

    /// Mutably enable exposure for a single capability ID
    pub fn enable_context_exposure_for(&mut self, capability_id: &str) {
        self.expose_readonly_context = true;
        self.exposed_context_caps.insert(capability_id.to_string());
    }

    /// Enable exposure for capabilities matching any of the provided prefixes (builder-style)
    pub fn with_context_prefixes(mut self, prefixes: &[&str]) -> Self {
        self.expose_readonly_context = true;
        for p in prefixes { self.exposed_context_prefixes.push((*p).to_string()); }
        self
    }

    /// Mutably enable a single exposure prefix
    pub fn enable_context_exposure_prefix(&mut self, prefix: &str) {
        self.expose_readonly_context = true;
        self.exposed_context_prefixes.push(prefix.to_string());
    }

    /// Enable exposure for capabilities that declare any of the provided tags (builder-style)
    pub fn with_context_tags(mut self, tags: &[&str]) -> Self {
        self.expose_readonly_context = true;
        for t in tags { self.exposed_context_tags.insert((*t).to_string()); }
        self
    }

    /// Mutably enable a single exposure tag
    pub fn enable_context_exposure_tag(&mut self, tag: &str) {
        self.expose_readonly_context = true;
        self.exposed_context_tags.insert(tag.to_string());
    }
}

/// Predefined security policies for common use cases
pub struct SecurityPolicies;

impl SecurityPolicies {
    /// Policy for running user-provided RTFS code
    pub fn user_code() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.data.parse-json".to_string(),
            "ccos.data.serialize-json".to_string(),
            // Allow safe LLM calls in user code
            "ccos.ai.llm-execute".to_string(),
        ])
    }
    
    /// Policy for running system management code
    pub fn system_management() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.io.print".to_string(),
            "ccos.io.println".to_string(),
            "ccos.io.file-exists".to_string(),
            "ccos.data.parse-json".to_string(),
            "ccos.data.serialize-json".to_string(),
            "ccos.system.current-time".to_string(),
            "ccos.system.current-timestamp-ms".to_string(),
            // Allow LLM calls for system prompts (audited)
            "ccos.ai.llm-execute".to_string(),
        ])
    }
    
    /// Policy for running data processing code
    pub fn data_processing() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.data.parse-json".to_string(),
            "ccos.data.serialize-json".to_string(),
            "ccos.network.http-fetch".to_string(),
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            "ccos.ask-human".to_string(),
            // Allow LLM calls for summarization/extraction
            "ccos.ai.llm-execute".to_string(),
        ])
    }
    
    /// Policy for running agent coordination code
    pub fn agent_coordination() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.agent.discover-agents".to_string(),
            "ccos.agent.task-coordination".to_string(),
            "ccos.agent.ask-human".to_string(),
            "ccos.agent.discover-and-assess-agents".to_string(),
            "ccos.agent.establish-system-baseline".to_string(),
            // Allow LLM calls for negotiation/coordination
            "ccos.ai.llm-execute".to_string(),
        ])
    }
    
    /// Policy for running file operations (high security)
    pub fn file_operations() -> RuntimeContext {
        let mut ctx = RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.io.file-exists".to_string(),
            "ccos.io.open-file".to_string(),
            "ccos.io.read-line".to_string(),
            "ccos.io.write-line".to_string(),
            "ccos.io.close-file".to_string(),
            // LLM execution disabled here by default for tighter isolation
        ]);
        
        // Force microVM for all file operations
        ctx.use_microvm = true;
        ctx.max_execution_time = Some(10000); // 10 seconds
        ctx.max_memory_usage = Some(32 * 1024 * 1024); // 32MB
        
        ctx
    }
    
    /// Policy for testing capabilities (includes all test capabilities)
    pub fn test_capabilities() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            "ccos.ask-human".to_string(),
            "ccos.io.log".to_string(),
            "ccos.data.parse-json".to_string(),
            "ccos.data.serialize-json".to_string(),
            // Enable LLM for tests
            "ccos.ai.llm-execute".to_string(),
        ])
    }
}

/// Security validator for runtime contexts
pub struct SecurityValidator;

impl SecurityValidator {
    /// Validate a runtime context for security compliance
    pub fn validate(ctx: &RuntimeContext) -> Result<(), String> {
        // Check execution time limits
        if let Some(time_limit) = ctx.max_execution_time {
            if time_limit > 60000 { // 60 seconds
                return Err("Execution time limit too high".to_string());
            }
        }
        
        // Check memory limits
        if let Some(memory_limit) = ctx.max_memory_usage {
            if memory_limit > 512 * 1024 * 1024 { // 512MB
                return Err("Memory limit too high".to_string());
            }
        }
        
        // Validate capability combinations
        if ctx.allowed_capabilities.contains("ccos.io.open-file") 
            && !ctx.use_microvm 
            && ctx.security_level != SecurityLevel::Full {
            return Err("File operations require microVM execution".to_string());
        }
        
        if ctx.allowed_capabilities.contains("ccos.network.http-fetch")
            && !ctx.use_microvm
            && ctx.security_level != SecurityLevel::Full {
            return Err("Network operations require microVM execution".to_string());
        }
        
        Ok(())
    }
    
    /// Check if a capability requires additional permissions
    pub fn requires_elevated_permissions(capability_id: &str) -> bool {
        let elevated_capabilities = [
            "ccos.io.open-file",
            "ccos.io.read-line", 
            "ccos.io.write-line",
            "ccos.io.close-file",
            "ccos.network.http-fetch",
            "ccos.system.get-env",
        ];
        
        elevated_capabilities.contains(&capability_id)
    }
    
    /// Get recommended security level for a capability
    pub fn recommended_security_level(capability_id: &str) -> SecurityLevel {
        match capability_id {
            // Pure capabilities
            "ccos.io.log" | "ccos.io.print" | "ccos.io.println" => SecurityLevel::Controlled,
            
            // Data processing capabilities
            "ccos.data.parse-json" | "ccos.data.serialize-json" => SecurityLevel::Controlled,
            
            // Time capabilities
            "ccos.system.current-time" | "ccos.system.current-timestamp-ms" => SecurityLevel::Controlled,
            
            // LLM execution is controlled with auditing
            "ccos.ai.llm-execute" => SecurityLevel::Controlled,
            
            // Dangerous capabilities
            "ccos.io.open-file" | "ccos.io.read-line" | "ccos.io.write-line" | "ccos.io.close-file" |
            "ccos.network.http-fetch" | "ccos.system.get-env" => SecurityLevel::Full,
            
            // Agent capabilities
            "ccos.agent.discover-agents" | "ccos.agent.task-coordination" | "ccos.agent.ask-human" |
            "ccos.agent.discover-and-assess-agents" | "ccos.agent.establish-system-baseline" => SecurityLevel::Controlled,
            
            // Default to full security for unknown capabilities
            _ => SecurityLevel::Full,
        }
    }
}
