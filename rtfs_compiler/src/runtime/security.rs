//! Secure Runtime Configuration
//!
//! This module defines security policies and execution contexts for RTFS programs.

use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::microvm::config::MicroVMConfig;
use crate::runtime::values::Value;
use std::collections::{HashMap, HashSet};

/// RTFS-local isolation levels for security contexts
///
/// This enum defines the isolation levels that RTFS can express.
/// CCOS will map these to its own isolation levels during execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IsolationLevel {
    /// Inherit isolation from parent context
    Inherit,
    /// Isolated execution with limited capabilities
    Isolated,
    /// Sandboxed execution with strict security boundaries
    Sandboxed,
}

impl IsolationLevel {
    /// Convert from CCOS execution context isolation level to RTFS security isolation level
    pub fn from_ccos(ccos_level: &crate::ccos::execution_context::IsolationLevel) -> Self {
        match ccos_level {
            crate::ccos::execution_context::IsolationLevel::Inherit => IsolationLevel::Inherit,
            crate::ccos::execution_context::IsolationLevel::Isolated => IsolationLevel::Isolated,
            crate::ccos::execution_context::IsolationLevel::Sandboxed => IsolationLevel::Sandboxed,
        }
    }
}

/// Central security authorizer for capability execution
pub struct SecurityAuthorizer;

impl SecurityAuthorizer {
    /// Authorize a capability execution request
    ///
    /// This is the central point for all capability authorization decisions.
    /// It validates the request against the runtime context and returns
    /// the minimal set of permissions needed for this specific execution.
    pub fn authorize_capability(
        runtime_context: &RuntimeContext,
        capability_id: &str,
        args: &[crate::runtime::values::Value],
    ) -> RuntimeResult<Vec<String>> {
        // Check if the capability is allowed by the runtime context
        if !runtime_context.is_capability_allowed(capability_id) {
            return Err(RuntimeError::SecurityViolation {
                operation: "capability_authorization".to_string(),
                capability: capability_id.to_string(),
                context: format!(
                    "Capability '{}' not allowed by runtime context",
                    capability_id
                ),
            });
        }

        if runtime_context.allowed_effects.is_some() || !runtime_context.denied_effects.is_empty()
        {
            let inferred_effects: Vec<String> = default_effects_for_capability(capability_id)
                .iter()
                .map(|effect| (*effect).to_string())
                .collect();
            runtime_context.ensure_effects_allowed(capability_id, &inferred_effects)?;
        }

        // Determine the minimal set of permissions needed for this execution
        let mut required_permissions = vec![capability_id.to_string()];

        // Add additional permissions based on capability type and arguments
        match capability_id {
            // File operations might need additional file-specific permissions
            "ccos.io.open-file" | "ccos.io.read-line" | "ccos.io.write-line" => {
                if let Some(crate::runtime::values::Value::String(path)) = args.get(0) {
                    // Could add path-based permissions here
                    required_permissions.push("ccos.io.file-access".to_string());
                }
            }
            // Network operations might need additional network permissions
            "ccos.network.http-fetch" => {
                required_permissions.push("ccos.network.outbound".to_string());
            }
            // System operations might need additional system permissions
            "ccos.system.get-env" => {
                required_permissions.push("ccos.system.environment".to_string());
            }
            _ => {
                // For other capabilities, just the capability ID is sufficient
            }
        }

        Ok(required_permissions)
    }

    /// Authorize a program execution request
    ///
    /// This validates program execution against the runtime context and
    /// determines what permissions are needed for the program to run.
    pub fn authorize_program(
        runtime_context: &RuntimeContext,
        program: &crate::runtime::microvm::core::Program,
        capability_id: Option<&str>,
    ) -> RuntimeResult<Vec<String>> {
        let mut required_permissions = Vec::new();

        // If a specific capability is requested, authorize it
        if let Some(cap_id) = capability_id {
            if !runtime_context.is_capability_allowed(cap_id) {
                return Err(RuntimeError::SecurityViolation {
                    operation: "program_authorization".to_string(),
                    capability: cap_id.to_string(),
                    context: format!("Capability '{}' not allowed for program execution", cap_id),
                });
            }
            if runtime_context.allowed_effects.is_some()
                || !runtime_context.denied_effects.is_empty()
            {
                let inferred_effects: Vec<String> =
                    default_effects_for_capability(cap_id)
                        .iter()
                        .map(|effect| (*effect).to_string())
                        .collect();
                runtime_context.ensure_effects_allowed(cap_id, &inferred_effects)?;
            }
            required_permissions.push(cap_id.to_string());
        }

        // Analyze the program to determine additional permissions needed
        match program {
            crate::runtime::microvm::core::Program::ExternalProgram { path, args } => {
                // External programs require special permission
                if !runtime_context.is_capability_allowed("external_program") {
                    return Err(RuntimeError::SecurityViolation {
                        operation: "external_program_authorization".to_string(),
                        capability: "external_program".to_string(),
                        context: "External program execution not permitted".to_string(),
                    });
                }
                required_permissions.push("external_program".to_string());
            }
            crate::runtime::microvm::core::Program::NativeFunction(_) => {
                // Native functions require special permission
                if !runtime_context.is_capability_allowed("native_function") {
                    return Err(RuntimeError::SecurityViolation {
                        operation: "native_function_authorization".to_string(),
                        capability: "native_function".to_string(),
                        context: "Native function execution not permitted".to_string(),
                    });
                }
                required_permissions.push("native_function".to_string());
            }
            _ => {
                // RTFS programs are generally allowed if the capability is authorized
            }
        }

        Ok(required_permissions)
    }

    /// Validate that the execution context has the required permissions
    ///
    /// This is a final validation step that ensures the execution context
    /// contains all the permissions that were authorized.
    pub fn validate_execution_context(
        required_permissions: &[String],
        execution_context: &crate::runtime::microvm::core::ExecutionContext,
    ) -> RuntimeResult<()> {
        for permission in required_permissions {
            if !execution_context
                .capability_permissions
                .contains(permission)
            {
                return Err(RuntimeError::SecurityViolation {
                    operation: "execution_context_validation".to_string(),
                    capability: permission.clone(),
                    context: format!(
                        "Required permission '{}' not in execution context permissions: {:?}",
                        permission, execution_context.capability_permissions
                    ),
                });
            }
        }
        Ok(())
    }
}

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
    /// Optional allowlist of effects permitted in this context (None means all effects allowed)
    pub allowed_effects: Option<HashSet<String>>,
    /// Deny list of effects that are always disallowed in this context
    pub denied_effects: HashSet<String>,
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
    /// Optional override for per-step MicroVM configuration
    pub microvm_config_override: Option<MicroVMConfig>,
    /// Cross-plan parameters (merged by orchestrator across plans)
    pub cross_plan_params: HashMap<String, Value>,
}

impl RuntimeContext {
    /// Create a pure (secure) runtime context
    pub fn pure() -> Self {
        Self {
            security_level: SecurityLevel::Pure,
            allowed_capabilities: HashSet::new(),
            allowed_effects: None,
            denied_effects: HashSet::new(),
            use_microvm: false,
            max_execution_time: Some(1000),           // 1 second
            max_memory_usage: Some(16 * 1024 * 1024), // 16MB
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: HashMap::new(),
        }
    }

    /// Create a controlled runtime context with specific capabilities
    pub fn controlled(allowed_capabilities: Vec<String>) -> Self {
        Self {
            security_level: SecurityLevel::Controlled,
            allowed_capabilities: allowed_capabilities.into_iter().collect(),
            allowed_effects: None,
            denied_effects: HashSet::new(),
            use_microvm: true,
            max_execution_time: Some(5000),           // 5 seconds
            max_memory_usage: Some(64 * 1024 * 1024), // 64MB
            log_capability_calls: true,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: HashMap::new(),
        }
    }

    /// Create a full-access runtime context (for trusted code)
    pub fn full() -> Self {
        Self {
            security_level: SecurityLevel::Full,
            allowed_capabilities: HashSet::new(), // Empty means all allowed
            allowed_effects: None,
            denied_effects: HashSet::new(),
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
            microvm_config_override: None,
            cross_plan_params: HashMap::new(),
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

    /// Create a new RuntimeContext with cross-plan parameters enabled
    pub fn with_cross_plan_context(mut self) -> Self {
        self.cross_plan_params.clear();
        self
    }

    /// Add a cross-plan parameter
    pub fn add_cross_plan_param(&mut self, key: String, value: Value) {
        self.cross_plan_params.insert(key, value);
    }

    /// Get a cross-plan parameter
    pub fn get_cross_plan_param(&self, key: &str) -> Option<&Value> {
        self.cross_plan_params.get(key)
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
    pub fn is_context_exposure_allowed_for(
        &self,
        capability_id: &str,
        capability_tags: Option<&[String]>,
    ) -> bool {
        if !self.expose_readonly_context {
            return false;
        }
        if self.exposed_context_caps.contains(capability_id) {
            return true;
        }
        if self
            .exposed_context_prefixes
            .iter()
            .any(|p| capability_id.starts_with(p))
        {
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
        for id in capability_ids {
            self.exposed_context_caps.insert((*id).to_string());
        }
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
        for p in prefixes {
            self.exposed_context_prefixes.push((*p).to_string());
        }
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
        for t in tags {
            self.exposed_context_tags.insert((*t).to_string());
        }
        self
    }

    /// Mutably enable a single exposure tag
    pub fn enable_context_exposure_tag(&mut self, tag: &str) {
        self.expose_readonly_context = true;
        self.exposed_context_tags.insert(tag.to_string());
    }

    /// Builder: attach a MicroVM configuration override
    pub fn with_microvm_config(mut self, config: MicroVMConfig) -> Self {
        self.microvm_config_override = Some(config);
        self
    }

    /// Mutably set a MicroVM configuration override
    pub fn set_microvm_config(&mut self, config: MicroVMConfig) {
        self.microvm_config_override = Some(config);
    }

    /// Replace the effect allowlist with the provided set. Passing an empty slice removes the allowlist.
    pub fn with_effect_allowlist(mut self, effects: &[&str]) -> Self {
        if effects.is_empty() {
            self.allowed_effects = None;
        } else {
            let mut set = HashSet::with_capacity(effects.len());
            for effect in effects {
                let normalized = normalize_effect_label(effect);
                if !normalized.is_empty() {
                    set.insert(normalized);
                }
            }
            self.allowed_effects = Some(set);
        }
        self
    }

    /// Append a single effect to the allowlist, creating it if necessary.
    pub fn allow_effect(&mut self, effect: &str) {
        let normalized = normalize_effect_label(effect);
        if normalized.is_empty() {
            return;
        }
        match &mut self.allowed_effects {
            Some(set) => {
                set.insert(normalized);
            }
            None => {
                let mut set = HashSet::with_capacity(1);
                set.insert(normalized);
                self.allowed_effects = Some(set);
            }
        }
    }

    /// Replace the deny list with the provided set of effects.
    pub fn with_effect_denies(mut self, effects: &[&str]) -> Self {
        self.denied_effects.clear();
        for effect in effects {
            let normalized = normalize_effect_label(effect);
            if !normalized.is_empty() {
                self.denied_effects.insert(normalized);
            }
        }
        self
    }

    /// Append a single effect to the deny list.
    pub fn deny_effect(&mut self, effect: &str) {
        let normalized = normalize_effect_label(effect);
        if !normalized.is_empty() {
            self.denied_effects.insert(normalized);
        }
    }

    /// Ensure the provided effects are permitted in this runtime context.
    pub fn ensure_effects_allowed(
        &self,
        capability_id: &str,
        effects: &[String],
    ) -> RuntimeResult<()> {
        for effect in effects {
            let normalized = normalize_effect_label(effect);
            if normalized.is_empty() {
                continue;
            }
            if self.denied_effects.contains(&normalized) {
                return Err(RuntimeError::SecurityViolation {
                    operation: "effect_policy".to_string(),
                    capability: capability_id.to_string(),
                    context: format!(
                        "Effect '{}' denied by runtime context",
                        normalized
                    ),
                });
            }
        }

        if let Some(allowlist) = &self.allowed_effects {
            for effect in effects {
                let normalized = normalize_effect_label(effect);
                if normalized.is_empty() {
                    continue;
                }
                if !allowlist.contains(&normalized) {
                    return Err(RuntimeError::SecurityViolation {
                        operation: "effect_policy".to_string(),
                        capability: capability_id.to_string(),
                        context: format!(
                            "Effect '{}' not permitted by runtime context allowlist",
                            normalized
                        ),
                    });
                }
            }
        }

        Ok(())
    }
}

/// Normalize effect labels to the canonical `:effect` format.
fn normalize_effect_label(effect: &str) -> String {
    let trimmed = effect.trim().trim_matches(|c| c == '\"' || c == '\'');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with(':') {
        trimmed.to_lowercase()
    } else {
        format!(":{}", trimmed.to_lowercase())
    }
}

/// Default effect mapping for core CCOS capabilities when manifests do not supply metadata.
pub fn default_effects_for_capability(capability_id: &str) -> &'static [&'static str] {
    match capability_id {
        // File system related capabilities
        "ccos.io.file-exists"
        | "ccos.io.open-file"
        | "ccos.io.read-line"
        | "ccos.io.write-line"
        | "ccos.io.close-file" => &[":filesystem"],
        // Logging and data utilities are treated as compute
        "ccos.io.log"
        | "ccos.io.print"
        | "ccos.io.println"
        | "ccos.data.parse-json"
        | "ccos.data.serialize-json"
        | "ccos.math.add"
        | "ccos.echo" => &[":compute"],
        // Network operations
        "ccos.network.http-fetch" => &[":network"],
        // System introspection
        "ccos.system.get-env"
        | "ccos.system.current-time"
        | "ccos.system.current-timestamp-ms" => &[":system"],
        // AI and agent operations
        "ccos.ai.llm-execute" => &[":ai"],
        cap if cap.starts_with("ccos.agent.") => &[":agent"],
        // Streaming capabilities default to streaming effect
        cap if cap.starts_with("ccos.stream.") => &[":streaming"],
        // Fallback to compute for unknown capabilities
        _ => &[":compute"],
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
            if time_limit > 60000 {
                // 60 seconds
                return Err("Execution time limit too high".to_string());
            }
        }

        // Check memory limits
        if let Some(memory_limit) = ctx.max_memory_usage {
            if memory_limit > 512 * 1024 * 1024 {
                // 512MB
                return Err("Memory limit too high".to_string());
            }
        }

        // Validate capability combinations
        if ctx.allowed_capabilities.contains("ccos.io.open-file")
            && !ctx.use_microvm
            && ctx.security_level != SecurityLevel::Full
        {
            return Err("File operations require microVM execution".to_string());
        }

        if ctx.allowed_capabilities.contains("ccos.network.http-fetch")
            && !ctx.use_microvm
            && ctx.security_level != SecurityLevel::Full
        {
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
            "ccos.system.current-time" | "ccos.system.current-timestamp-ms" => {
                SecurityLevel::Controlled
            }

            // LLM execution is controlled with auditing
            "ccos.ai.llm-execute" => SecurityLevel::Controlled,

            // Dangerous capabilities
            "ccos.io.open-file"
            | "ccos.io.read-line"
            | "ccos.io.write-line"
            | "ccos.io.close-file"
            | "ccos.network.http-fetch"
            | "ccos.system.get-env" => SecurityLevel::Full,

            // Agent capabilities
            "ccos.agent.discover-agents"
            | "ccos.agent.task-coordination"
            | "ccos.agent.ask-human"
            | "ccos.agent.discover-and-assess-agents"
            | "ccos.agent.establish-system-baseline" => SecurityLevel::Controlled,

            // Default to full security for unknown capabilities
            _ => SecurityLevel::Full,
        }
    }
}
