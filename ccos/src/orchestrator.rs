//! CCOS Orchestrator
//!
//! This module implements the Orchestrator, the component responsible for driving the
//! execution of a `Plan`. It interprets orchestration primitives like `(step ...)`
//! and ensures that all actions are securely executed and logged to the Causal Chain.
//!
//! The Orchestrator acts as the stateful engine for a plan, sitting between the
//! high-level cognitive reasoning of the Arbiter and the low-level execution of
//! the RTFS runtime and Capability Marketplace.
//!
//! ## MicroVM Security Integration
//!
//! The Orchestrator now includes per-step MicroVM profile derivation, which analyzes
//! each step's operations and derives appropriate security profiles including:
//! - Network access control lists (ACLs)
//! - File system policies
//! - Determinism flags for reproducible execution
//! - Resource limits and isolation levels

use crate::capability_marketplace::CapabilityMarketplace;
use crate::execution_context::IsolationLevel;
use crate::host::RuntimeHost;
use rtfs::ast::MapKey;
use rtfs::parser::parse_expression;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::evaluator::Evaluator;
use rtfs::runtime::execution_outcome::ExecutionOutcome;
use rtfs::runtime::microvm::config::{FileSystemPolicy, MicroVMConfig, NetworkPolicy};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use serde_json::{self, Value as JsonValue};
use std::sync::{Arc, Mutex};

use super::causal_chain::CausalChain;
use super::intent_graph::IntentGraph;
use super::types::{
    Action, ActionType, ExecutionResult, IntentId, IntentStatus, Plan, PlanBody, PlanId,
    PlanLanguage,
};
use rtfs::ast::{Expression, Literal};

use super::checkpoint_archive::{CheckpointArchive, CheckpointRecord};
use super::plan_archive::PlanArchive;
use super::types::StorableIntent;
use chrono;
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::values::Value as RtfsValue;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Full execution context reconstructed from causal chain for replay
/// Contains the plan, all referenced intents, and all actions in chronological order
#[derive(Debug, Clone)]
pub struct ReplayContext {
    pub plan: Plan,
    pub intents: Vec<StorableIntent>,
    pub actions: Vec<Action>,
}

/// Represents the security and isolation profile for a single step execution
#[derive(Debug, Clone)]
pub struct StepProfile {
    /// Unique identifier for this profile
    pub profile_id: String,
    /// Step name or description
    pub step_name: String,
    /// Required isolation level for this step
    pub isolation_level: IsolationLevel,
    /// MicroVM configuration derived for this step
    pub microvm_config: MicroVMConfig,
    /// Whether this step requires deterministic execution
    pub deterministic: bool,
    /// Resource limits specific to this step
    pub resource_limits: ResourceLimits,
    /// Security flags and constraints
    pub security_flags: SecurityFlags,
}

/// Resource limits for step execution
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum execution time in milliseconds
    pub max_execution_time_ms: u64,
    /// Maximum memory usage in bytes
    pub max_memory_bytes: u64,
    /// Maximum CPU usage (as a multiplier of single core)
    pub max_cpu_usage: f64,
    /// Maximum disk I/O operations
    pub max_io_operations: Option<u64>,
    /// Maximum network bandwidth in bytes per second
    pub max_network_bandwidth: Option<u64>,
}

/// Security flags for step execution
#[derive(Debug, Clone)]
pub struct SecurityFlags {
    /// Whether to enable system call filtering
    pub enable_syscall_filter: bool,
    /// Whether to enable network access control
    pub enable_network_acl: bool,
    /// Whether to enable file system access control
    pub enable_fs_acl: bool,
    /// Whether to enable memory protection
    pub enable_memory_protection: bool,
    /// Whether to enable CPU usage monitoring
    pub enable_cpu_monitoring: bool,
    /// Whether to log all system calls
    pub log_syscalls: bool,
    /// Whether to enforce read-only file system for this step
    pub read_only_fs: bool,
}

/// Derives security profiles for individual steps based on their operations
pub struct StepProfileDeriver;

impl StepProfileDeriver {
    /// Derive a security profile for a given step expression
    pub fn derive_profile(
        step_name: &str,
        step_expr: &Expression,
        base_context: &RuntimeContext,
    ) -> RuntimeResult<StepProfile> {
        let mut profile = StepProfile {
            profile_id: format!("profile-{}-{}", step_name, chrono::Utc::now().timestamp()),
            step_name: step_name.to_string(),
            isolation_level: Self::derive_isolation_level(step_expr),
            microvm_config: Self::derive_microvm_config(step_expr),
            deterministic: Self::is_deterministic_operation(step_expr),
            resource_limits: Self::derive_resource_limits(step_expr),
            security_flags: Self::derive_security_flags(step_expr),
        };

        // Adjust profile based on runtime context
        Self::adjust_for_runtime_context(&mut profile, base_context);

        // Final enforcement: syscall filter only for explicit system operations
        if let Expression::List(exprs) = step_expr {
            let final_has_system = Self::contains_system_operations(exprs);
            profile.security_flags.enable_syscall_filter = final_has_system;
            if !final_has_system {
                profile.security_flags.log_syscalls = false;
            } else {
                profile.security_flags.log_syscalls = true;
                profile.security_flags.read_only_fs = true;
            }
        }

        // Debug: print final security flags before returning
        println!(
            "DEBUG FINAL FLAGS: step={} syscall_filter={} net_acl={} fs_acl={} deterministic={}",
            step_name,
            profile.security_flags.enable_syscall_filter,
            profile.security_flags.enable_network_acl,
            profile.security_flags.enable_fs_acl,
            profile.deterministic
        );
        println!(
            "DEBUG FINAL LIMITS: step={} time_ms={} mem_bytes={} cpu={}",
            step_name,
            profile.resource_limits.max_execution_time_ms,
            profile.resource_limits.max_memory_bytes,
            profile.resource_limits.max_cpu_usage
        );

        Ok(profile)
    }

    /// Analyze the step expression to determine required isolation level
    fn derive_isolation_level(step_expr: &Expression) -> IsolationLevel {
        match step_expr {
            // Dangerous operations requiring sandboxed execution
            Expression::List(exprs) if Self::contains_dangerous_operations(exprs) => {
                IsolationLevel::Sandboxed
            }
            // Network operations requiring isolation
            Expression::List(exprs) if Self::contains_network_operations(exprs) => {
                IsolationLevel::Isolated
            }
            // File operations requiring isolation
            Expression::List(exprs) if Self::contains_file_operations(exprs) => {
                IsolationLevel::Isolated
            }
            // System operations requiring isolation
            Expression::List(exprs) if Self::contains_system_operations(exprs) => {
                IsolationLevel::Isolated
            }
            // Safe operations can inherit parent context
            _ => IsolationLevel::Inherit,
        }
    }

    /// Derive MicroVM configuration based on step operations
    fn derive_microvm_config(step_expr: &Expression) -> MicroVMConfig {
        let mut config = MicroVMConfig::default();

        match step_expr {
            Expression::List(exprs) => {
                // Check for network operations
                if Self::contains_network_operations(exprs) {
                    config.network_policy = NetworkPolicy::AllowList(vec![
                        "api.example.com".to_string(), // Default allowlist
                    ]);
                } else {
                    config.network_policy = NetworkPolicy::Denied;
                }

                // Check for file operations
                if Self::contains_file_operations(exprs) {
                    config.fs_policy = FileSystemPolicy::ReadWrite(vec![
                        "/tmp".to_string(), // Default writable paths
                        "/app/data".to_string(),
                    ]);
                } else {
                    config.fs_policy = FileSystemPolicy::None;
                }

                // Adjust resource limits based on operation complexity
                if Self::is_computationally_intensive(exprs) {
                    config.timeout = std::time::Duration::from_secs(60);
                    config.memory_limit_mb = 1024;
                    config.cpu_limit = 2.0;
                }
            }
            _ => {
                // Conservative defaults for unknown expressions
                config.network_policy = NetworkPolicy::Denied;
                config.fs_policy = FileSystemPolicy::None;
            }
        }

        config
    }

    /// Determine if the step requires deterministic execution
    fn is_deterministic_operation(step_expr: &Expression) -> bool {
        match step_expr {
            // Pure functions and data transformations are deterministic
            Expression::List(exprs) if Self::is_pure_function_call(exprs) => {
                println!("DEBUG DETERMINISM: pure_function_call=true");
                true
            }
            // Math operations are deterministic
            Expression::List(exprs) if Self::contains_math_operations(exprs) => {
                println!("DEBUG DETERMINISM: math_ops=true");
                true
            }
            // Data parsing/serialization is deterministic
            Expression::List(exprs) if Self::contains_data_operations(exprs) => {
                println!("DEBUG DETERMINISM: data_ops=true");
                true
            }
            // I/O operations are generally non-deterministic
            Expression::List(exprs) if Self::contains_io_operations(exprs) => {
                println!("DEBUG DETERMINISM: io_ops=true");
                false
            }
            // Network operations are non-deterministic
            Expression::List(exprs) if Self::contains_network_operations(exprs) => {
                println!("DEBUG DETERMINISM: network_ops=true");
                false
            }
            // Default to non-deterministic for safety
            _ => {
                println!("DEBUG DETERMINISM: default=false");
                false
            }
        }
    }

    /// Derive resource limits based on step complexity
    fn derive_resource_limits(step_expr: &Expression) -> ResourceLimits {
        let base_limits = ResourceLimits {
            max_execution_time_ms: 30000,        // 30 seconds default
            max_memory_bytes: 256 * 1024 * 1024, // 256MB default
            max_cpu_usage: 1.0,                  // Single core default
            max_io_operations: Some(1000),
            max_network_bandwidth: Some(1024 * 1024), // 1MB/s default
        };

        match step_expr {
            Expression::List(exprs) => {
                if Self::is_computationally_intensive(exprs) {
                    ResourceLimits {
                        max_execution_time_ms: 300000,        // 5 minutes for intensive tasks
                        max_memory_bytes: 1024 * 1024 * 1024, // 1GB
                        max_cpu_usage: 2.0,                   // Multi-core
                        ..base_limits
                    }
                } else if Self::contains_network_operations(exprs) {
                    ResourceLimits {
                        max_execution_time_ms: 120000, // 2 minutes for network ops
                        max_network_bandwidth: Some(10 * 1024 * 1024), // 10MB/s
                        ..base_limits
                    }
                } else if Self::contains_file_operations(exprs) {
                    ResourceLimits {
                        max_execution_time_ms: 60000, // 1 minute for file ops
                        max_io_operations: Some(5000),
                        ..base_limits
                    }
                } else {
                    base_limits
                }
            }
            _ => base_limits,
        }
    }

    /// Derive security flags based on step requirements
    fn derive_security_flags(step_expr: &Expression) -> SecurityFlags {
        let mut flags = SecurityFlags {
            enable_syscall_filter: false,
            enable_network_acl: false,
            enable_fs_acl: false,
            enable_memory_protection: true, // Always enable memory protection
            enable_cpu_monitoring: true,    // Always enable CPU monitoring
            log_syscalls: false,
            read_only_fs: false,
        };

        match step_expr {
            Expression::List(exprs) => {
                // Debug: Check what operations are detected
                let has_system = Self::contains_system_operations(exprs);
                let has_network = Self::contains_network_operations(exprs);
                let has_file = Self::contains_file_operations(exprs);

                println!(
                    "DEBUG: has_system={}, has_network={}, has_file={}",
                    has_system, has_network, has_file
                );
                println!(
                    "DEBUG: Before flags - enable_syscall_filter={}",
                    flags.enable_syscall_filter
                );

                // Explicitly set syscall filter strictly for system/exec operations
                flags.enable_syscall_filter = has_system;
                if has_system {
                    flags.log_syscalls = true;
                    flags.read_only_fs = true;
                    println!("DEBUG: Set syscall filter to true because has_system=true");
                } else {
                    // Ensure these remain consistent when not a system op
                    flags.log_syscalls = false;
                    // Do not force read_only_fs for non-system ops
                }

                if has_network {
                    flags.enable_network_acl = true;
                }

                if has_file {
                    flags.enable_fs_acl = true;
                }

                println!(
                    "DEBUG: After flags - enable_syscall_filter={}",
                    flags.enable_syscall_filter
                );
            }
            _ => {}
        }

        flags
    }

    /// Adjust profile based on runtime context constraints
    fn adjust_for_runtime_context(profile: &mut StepProfile, context: &RuntimeContext) {
        println!(
            "DEBUG ADJUST START: ctx.max_time={:?} ctx.max_mem={:?} profile.time={} profile.mem={}",
            context.max_execution_time,
            context.max_memory_usage,
            profile.resource_limits.max_execution_time_ms,
            profile.resource_limits.max_memory_bytes
        );
        // If runtime context doesn't allow the derived isolation level, downgrade
        // Convert CCOS isolation level to RTFS isolation level
        let rtfs_isolation_level = match &profile.isolation_level {
            IsolationLevel::Inherit => rtfs::runtime::security::IsolationLevel::Inherit,
            IsolationLevel::Isolated => rtfs::runtime::security::IsolationLevel::Isolated,
            IsolationLevel::Sandboxed => rtfs::runtime::security::IsolationLevel::Sandboxed,
        };
        if !context.is_isolation_allowed(&rtfs_isolation_level) {
            match profile.isolation_level {
                IsolationLevel::Sandboxed => {
                    if context.allow_isolated_isolation {
                        profile.isolation_level = IsolationLevel::Isolated;
                    } else {
                        profile.isolation_level = IsolationLevel::Inherit;
                    }
                }
                IsolationLevel::Isolated => {
                    profile.isolation_level = IsolationLevel::Inherit;
                }
                _ => {}
            }
        }

        // Adjust resource limits based on context
        if let Some(max_time) = context.max_execution_time {
            if profile.resource_limits.max_execution_time_ms > max_time {
                profile.resource_limits.max_execution_time_ms = max_time;
            }
        }

        if let Some(max_memory) = context.max_memory_usage {
            if profile.resource_limits.max_memory_bytes > max_memory {
                profile.resource_limits.max_memory_bytes = max_memory;
            }
        }
        println!(
            "DEBUG ADJUST END: profile.time={} profile.mem={}",
            profile.resource_limits.max_execution_time_ms, profile.resource_limits.max_memory_bytes
        );
    }

    // Helper methods for operation detection
    fn contains_dangerous_operations(exprs: &[Expression]) -> bool {
        Self::contains_system_operations(exprs) || Self::contains_external_programs(exprs)
    }

    /// When forms are desugared as `(call capability (values ...))`, this extracts `capability`
    fn extract_call_capability(exprs: &[Expression]) -> Option<String> {
        if exprs.len() >= 2 {
            if let Expression::Symbol(sym0) = &exprs[0] {
                if sym0.0.eq_ignore_ascii_case("call") {
                    if let Expression::Symbol(cap_sym) = &exprs[1] {
                        return Some(cap_sym.0.to_lowercase());
                    }
                }
            }
        }
        None
    }

    fn contains_network_operations(exprs: &[Expression]) -> bool {
        let network_capabilities = [
            "http-fetch",
            "http.fetch",
            "network",
            "socket",
            "fetch",
            "http",
        ]; // include generic http fallback
        if Self::contains_capabilities(exprs, &network_capabilities) {
            return true;
        }
        if let Some(name) = Self::extract_call_capability(exprs) {
            return network_capabilities.iter().any(|cap| name.contains(cap));
        }
        false
    }

    fn contains_file_operations(exprs: &[Expression]) -> bool {
        let file_capabilities = ["file", "io", "read", "write", "open"];
        if Self::contains_capabilities(exprs, &file_capabilities) {
            return true;
        }
        if let Some(name) = Self::extract_call_capability(exprs) {
            return file_capabilities.iter().any(|cap| name.contains(cap));
        }
        false
    }

    fn contains_system_operations(exprs: &[Expression]) -> bool {
        let system_capabilities = ["system", "exec", "shell", "process"];
        if Self::contains_capabilities(exprs, &system_capabilities) {
            return true;
        }
        if let Some(name) = Self::extract_call_capability(exprs) {
            return system_capabilities.iter().any(|cap| name.contains(cap));
        }
        false
    }

    fn contains_io_operations(exprs: &[Expression]) -> bool {
        Self::contains_file_operations(exprs) || Self::contains_network_operations(exprs)
    }

    fn contains_math_operations(exprs: &[Expression]) -> bool {
        let math_capabilities = ["math", "add", "multiply", "divide", "calculate"];
        Self::contains_capabilities(exprs, &math_capabilities)
    }

    fn contains_data_operations(exprs: &[Expression]) -> bool {
        // Prefer explicit capability names like data.* and common data ops in capability names
        if let Some(name) = Self::extract_call_capability(exprs) {
            let n = name.to_lowercase();
            if n.starts_with("data.")
                || n.contains("parse")
                || n.contains("serialize")
                || n.contains("json")
            {
                return true;
            }
        }
        // Scan symbols recursively but ignore string literals to avoid false positives like URLs containing "data"
        for e in exprs {
            match e {
                Expression::Symbol(sym) => {
                    let n = sym.0.to_lowercase();
                    if n.starts_with("data.")
                        || n.contains("parse")
                        || n.contains("serialize")
                        || n.contains("json")
                    {
                        return true;
                    }
                }
                Expression::List(list_exprs) => {
                    if Self::contains_data_operations(list_exprs) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn is_pure_function_call(exprs: &[Expression]) -> bool {
        // Pure functions don't contain I/O operations
        !Self::contains_io_operations(exprs) && !Self::contains_system_operations(exprs)
    }

    fn is_computationally_intensive(exprs: &[Expression]) -> bool {
        let intensive_patterns = ["loop", "iterate", "compute", "process", "analyze"];
        let has_cap = Self::contains_capabilities(exprs, &intensive_patterns);
        let call_name_matches = if let Some(name) = Self::extract_call_capability(exprs) {
            intensive_patterns.iter().any(|cap| name.contains(cap))
        } else {
            false
        };
        let result = has_cap || call_name_matches;
        println!(
            "DEBUG INTENSIVE: has_cap={} call_name_matches={} result={}",
            has_cap, call_name_matches, result
        );
        result
    }

    fn contains_external_programs(exprs: &[Expression]) -> bool {
        // Be strict: only detect explicit external/system calls, not arbitrary strings
        if let Some(name) = Self::extract_call_capability(exprs) {
            let name = name.as_str();
            return name == "system.execute"
                || name.contains("exec")
                || name.contains("shell")
                || name.contains("process.run")
                || name == "system";
        }
        // Also scan symbols directly at top-level
        for e in exprs {
            if let Expression::Symbol(sym) = e {
                let n = sym.0.to_lowercase();
                if n == "system.execute"
                    || n.contains("exec")
                    || n.contains("shell")
                    || n.contains("process.run")
                    || n == "system"
                {
                    return true;
                }
            }
        }
        false
    }

    fn contains_capabilities(exprs: &[Expression], capabilities: &[&str]) -> bool {
        exprs.iter().any(|expr| match expr {
            Expression::Symbol(name) => {
                let name_lower = name.0.to_lowercase();
                capabilities.iter().any(|cap| name_lower.contains(cap))
            }
            Expression::Literal(Literal::String(s)) => {
                let s_lower = s.to_lowercase();
                capabilities.iter().any(|cap| s_lower.contains(cap))
            }
            Expression::List(list_exprs) => Self::contains_capabilities(list_exprs, capabilities),
            _ => false,
        })
    }
}

/// The Orchestrator is the stateful engine that drives plan execution.
pub struct Orchestrator {
    causal_chain: Arc<Mutex<CausalChain>>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    checkpoint_archive: Arc<CheckpointArchive>,
    plan_archive: Arc<PlanArchive>,
    /// Current step profile being executed (for step-level security enforcement)
    current_step_profile: Option<StepProfile>,
}

impl Orchestrator {
    /// Creates a new Orchestrator.
    pub fn new(
        causal_chain: Arc<Mutex<CausalChain>>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Arc<CapabilityMarketplace>,
        plan_archive: Arc<PlanArchive>,
    ) -> Self {
        Self {
            causal_chain,
            intent_graph,
            capability_marketplace,
            checkpoint_archive: Arc::new(CheckpointArchive::new()),
            plan_archive,
            current_step_profile: None,
        }
    }

    /// Execute RTFS expression with yield-based control flow handling.
    /// This implements the top-level execution loop that handles RequiresHost outcomes.
    async fn execute_with_yield_handling(
        &self,
        evaluator: &Evaluator,
        expr: &Expression,
    ) -> RuntimeResult<ExecutionOutcome> {
        let current_expr = expr.clone();
        let mut max_iterations = 1000; // Prevent infinite loops

        loop {
            if max_iterations == 0 {
                return Err(RuntimeError::Generic(
                    "Maximum execution iterations reached".to_string(),
                ));
            }
            max_iterations -= 1;

            // Execute the current expression
            let result = evaluator.evaluate(&current_expr)?;

            match result {
                ExecutionOutcome::Complete(value) => {
                    // Execution completed successfully
                    return Ok(ExecutionOutcome::Complete(value));
                }
                ExecutionOutcome::RequiresHost(host_call) => {
                    // Propagate the RequiresHost up so the orchestrator can checkpoint
                    // and allow CCOS to prompt the user / agent before resuming.
                    // This avoids performing the host call inline here and then
                    // prematurely completing execution without a resumable state.
                    return Ok(ExecutionOutcome::RequiresHost(host_call));
                }
            }
        }
    }

    // handle_effect_request removed - unified into handle_host_call

    async fn handle_host_call(
        &self,
        host_call: &rtfs::runtime::execution_outcome::HostCall,
    ) -> RuntimeResult<Value> {
        // Unified capability handling - all host calls go through capability marketplace
        let args_value = Value::Vector(host_call.args.clone());

        // Use enhanced execution with metadata
        self.capability_marketplace
            .execute_capability_enhanced(
                &host_call.capability_id,
                &args_value,
                host_call.metadata.as_ref(),
            )
            .await
    }

    /// Executes a given `Plan` within a specified `RuntimeContext`.
    /// This is the main entry point for the Orchestrator.
    pub async fn execute_plan(
        &self,
        plan: &Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        let plan_id = plan.plan_id.clone();
        let primary_intent_id = plan.intent_ids.first().cloned().unwrap_or_default();

        // --- 0. Ensure Plan is archived BEFORE logging to causal chain ---
        // This guarantees causal chain consistency: every plan_id referenced in actions
        // must exist in the plan archive for replay to work.
        self.ensure_plan_archived(plan)?;

        // Verify intent exists in IntentGraph if referenced
        self.ensure_intent_exists(&primary_intent_id)?;

        // --- 1. Log PlanStarted Action ---
        // Now safe to log - plan and intent are guaranteed to be stored
        let plan_action_id = self.log_action(
            Action::new(
                ActionType::PlanStarted,
                plan_id.clone(),
                primary_intent_id.clone(),
            )
            .with_parent(None),
        )?;

        // Mark primary intent as Executing (transition Active -> Executing) before evaluation begins
        if !primary_intent_id.is_empty() {
            if let Ok(mut graph) = self.intent_graph.lock() {
                let _ = graph.set_intent_status_with_audit(
                    &primary_intent_id,
                    IntentStatus::Executing,
                    Some(&plan_id),
                    Some(&plan_action_id),
                );
            }
        }

        // --- 2. Set up the Host and Evaluator ---
        let host = Arc::new(RuntimeHost::new(
            self.causal_chain.clone(),
            self.capability_marketplace.clone(),
            context.clone(),
        ));
        host.set_execution_context(
            plan_id.clone(),
            plan.intent_ids.clone(),
            plan_action_id.clone(),
        );
        let module_registry = std::sync::Arc::new(ModuleRegistry::new());
        let host_iface: Arc<dyn HostInterface> = host.clone();
        let mut evaluator = Evaluator::new(
            module_registry.clone(),
            context.clone(),
            host_iface.clone(),
            rtfs::compiler::expander::MacroExpander::default(),
        );
        // Load CCOS prelude (effectful helpers) into the evaluator's environment
        crate::prelude::load_prelude(&mut evaluator.env);

        // Bind cross-plan parameters to the evaluator environment
        // This makes plan inputs (like owner, repository, language) available as variables
        for (key, value) in &context.cross_plan_params {
            let symbol = rtfs::ast::Symbol(key.clone());
            evaluator.env.define(&symbol, value.clone());
        }

        // ContextManager removed from RTFS - step lifecycle now handled by host
        // Plan context initialization is managed through set_execution_context

        // --- 3. Parse and Execute the Plan Body with Yield-Based Control Flow ---
        let final_result = match &plan.language {
            PlanLanguage::Rtfs20 => match &plan.body {
                PlanBody::Rtfs(rtfs_code) => {
                    let code = rtfs_code.trim();
                    if code.is_empty() {
                        Err(RuntimeError::Generic(
                            "Empty RTFS plan body after trimming".to_string(),
                        ))
                    } else {
                        match parse_expression(code) {
                            Ok(expr) => self.execute_with_yield_handling(&evaluator, &expr).await,
                            Err(e) => Err(RuntimeError::Generic(format!(
                                "Failed to parse RTFS plan body: {:?}",
                                e
                            ))),
                        }
                    }
                }
                PlanBody::Wasm(_) => Err(RuntimeError::Generic(
                    "RTFS plans must use Rtfs body format".to_string(),
                )),
            },
            _ => Err(RuntimeError::Generic(format!(
                "Unsupported plan language: {:?}",
                plan.language
            ))),
        };

        let mut evaluator = Evaluator::new(
            module_registry.clone(),
            context.clone(),
            host_iface.clone(),
            rtfs::compiler::expander::MacroExpander::default(),
        );

        // --- 4. Log Final Plan Status ---
        // If the evaluator yielded RequiresHost, we checkpoint and emit a PlanPaused
        // action so the caller (CCOS) can perform the required host interaction
        // (e.g., ask the user) and later resume execution. Otherwise, handle
        // completion or errors as before.
        let (execution_result, error_opt) = match final_result {
            Ok(ExecutionOutcome::Complete(value)) => {
                let res = ExecutionResult {
                    success: true,
                    value,
                    metadata: Default::default(),
                };
                self.log_action(
                    Action::new(
                        ActionType::PlanCompleted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_result(res.clone()),
                )?;
                (res, None)
            }
            Ok(ExecutionOutcome::RequiresHost(host_call)) => {
                // Unified checkpoint path: persist context + log PlanPaused via helper so
                // resume_and_continue_from_checkpoint can locate checkpoint.
                let (checkpoint_id, _serialized) =
                    self.checkpoint_plan(&plan_id, &primary_intent_id, &evaluator, None)?;

                // Build metadata describing required host capability.
                let mut metadata_map: std::collections::HashMap<String, RtfsValue> =
                    std::collections::HashMap::new();
                metadata_map.insert(
                    "requires_capability".to_string(),
                    RtfsValue::String(host_call.capability_id.clone()),
                );
                if host_call.metadata.is_some() {
                    metadata_map.insert(
                        "has_metadata".to_string(),
                        RtfsValue::String("true".to_string()),
                    );
                }
                metadata_map.insert(
                    "checkpoint_id".to_string(),
                    RtfsValue::String(checkpoint_id),
                );

                let res = ExecutionResult {
                    success: false,
                    value: RtfsValue::String("paused: requires host interaction".to_string()),
                    metadata: metadata_map,
                };
                (res, None)
            }
            Err(e) => {
                // Log aborted action first
                self.log_action(
                    Action::new(
                        ActionType::PlanAborted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_error(&e.to_string()),
                )?;
                // Represent failure value explicitly (string) so ExecutionResult always has a Value
                let failure_value = RtfsValue::String(format!("error: {}", e));
                let res = ExecutionResult {
                    success: false,
                    value: failure_value,
                    metadata: Default::default(),
                };
                (res, Some(e))
            }
        };

        // --- 5. Update Intent Graph ---
        // Update the primary intent(s) associated with this plan so the IntentGraph
        // reflects the final outcome of plan execution. We attempt to lock the
        // IntentGraph, load the primary intent, and call the graph's
        // update_intent helper which will set status/updated_at and persist the
        // change via storage.
        {
            let mut graph = self
                .intent_graph
                .lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

            if !primary_intent_id.is_empty() {
                if let Some(pre_intent) = graph.get_intent(&primary_intent_id) {
                    graph
                        .update_intent_with_audit(
                            pre_intent,
                            &execution_result,
                            Some(&plan_id),
                            Some(&plan_action_id),
                        )
                        .map_err(|e| {
                            RuntimeError::Generic(format!(
                                "IntentGraph update failed for {}: {:?}",
                                primary_intent_id, e
                            ))
                        })?;
                }
            }
        }

        // Propagate original error after updating intent status
        if let Some(err) = error_opt {
            Err(err)
        } else {
            Ok(execution_result)
        }
    }

    /// Execute an entire intent graph with cross-plan parameter merging
    /// This method orchestrates the execution of child intents and manages shared context
    pub async fn execute_intent_graph(
        &self,
        root_intent_id: &str,
        initial_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // Debug logging
        eprintln!(
            "DEBUG: execute_intent_graph called for root_intent_id: {}",
            root_intent_id
        );

        // 1. Start with an empty cross-plan param bag
        let mut enhanced_context = initial_context.clone();
        enhanced_context.cross_plan_params.clear();

        // 2. Execute children and merge exported vars
        let mut child_results = Vec::new();
        let children = self.get_children_order(root_intent_id)?;
        eprintln!("DEBUG: Found {} children: {:?}", children.len(), children);

        for child_id in children {
            eprintln!("DEBUG: Looking for plan for child_id: {}", child_id);
            if let Some(child_plan) = self.get_plan_for_intent(&child_id)? {
                eprintln!(
                    "DEBUG: Found plan for child_id {}: {:?}",
                    child_id, child_plan.plan_id
                );
                let child_result = self.execute_plan(&child_plan, &enhanced_context).await?;
                let exported = self.extract_exported_variables(&child_result);
                enhanced_context.cross_plan_params.extend(exported);
                child_results.push((child_id.clone(), child_result));
            } else {
                eprintln!("DEBUG: No plan found for child_id: {}", child_id);
            }
        }

        // 3. Optionally execute root plan (if any)
        let mut root_result = None;
        if let Some(root_plan) = self.get_plan_for_intent(root_intent_id)? {
            eprintln!("DEBUG: Found root plan: {:?}", root_plan.plan_id);
            root_result = Some(self.execute_plan(&root_plan, &enhanced_context).await?);
        } else {
            eprintln!("DEBUG: No root plan found");
        }

        // 4. Build a meaningful result that summarizes the execution
        let mut result_summary = Vec::new();

        // Add child results
        for (child_id, result) in &child_results {
            if result.success {
                // Use Display to render Value in RTFS syntax, not Debug/AST
                result_summary.push(format!("{}: {}", child_id, result.value));
            } else {
                result_summary.push(format!("{}: failed", child_id));
            }
        }

        // Add root result if any
        if let Some(ref root) = root_result {
            if root.success {
                // Use Display to render Value in RTFS syntax, not Debug/AST
                result_summary.push(format!("root: {}", root.value));
            } else {
                result_summary.push("root: failed".to_string());
            }
        }

        // Create a meaningful result value. If nothing was executed, mark as failure
        // so callers can detect that no plans ran rather than treating it as success.
        if result_summary.is_empty() {
            let result_value = RtfsValue::String("No plans executed".to_string());
            eprintln!("DEBUG: No plans executed, returning failure");
            Ok(ExecutionResult {
                success: false,
                value: result_value,
                metadata: Default::default(),
            })
        } else {
            let result_value = RtfsValue::String(format!(
                "Orchestrated {} plans: {}",
                child_results.len(),
                result_summary.join(", ")
            ));
            eprintln!(
                "DEBUG: Returning success with {} plans: {}",
                child_results.len(),
                result_summary.join(", ")
            );
            Ok(ExecutionResult {
                success: true,
                value: result_value,
                metadata: Default::default(),
            })
        }
    }

    /// Simple method to get children order
    fn get_children_order(&self, root_id: &str) -> RuntimeResult<Vec<String>> {
        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        // Use the authoritative get_child_intents method instead of the denormalized field
        let children = graph.get_child_intents(&root_id.to_string());

        Ok(children.into_iter().map(|child| child.intent_id).collect())
    }

    #[cfg(test)]
    fn test_get_children_order(&self, root_id: &str) -> Result<Vec<String>, String> {
        self.get_children_order(root_id)
            .map_err(|e| format!("get_children_order failed: {:?}", e))
    }

    /// Convert serde_json::Value to runtime::values::Value
    fn json_value_to_runtime_value(json_val: JsonValue) -> Value {
        match json_val {
            JsonValue::Null => Value::Nil,
            JsonValue::Bool(b) => Value::Boolean(b),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::String(n.to_string())
                }
            }
            JsonValue::String(s) => Value::String(s),
            JsonValue::Array(arr) => {
                let runtime_vec: Vec<Value> = arr
                    .into_iter()
                    .map(Self::json_value_to_runtime_value)
                    .collect();
                Value::Vector(runtime_vec)
            }
            JsonValue::Object(obj) => {
                let mut runtime_map = std::collections::HashMap::new();
                for (k, v) in obj {
                    runtime_map.insert(MapKey::String(k), Self::json_value_to_runtime_value(v));
                }
                Value::Map(runtime_map)
            }
        }
    }

    /// Get plan for a specific intent
    pub fn get_plan_for_intent(&self, intent_id: &str) -> RuntimeResult<Option<Plan>> {
        // Query the plan archive for plans associated with this intent
        let archivable_plans = self
            .plan_archive
            .get_plans_for_intent(&intent_id.to_string());

        // Convert the first available plan back to a Plan object
        if let Some(archivable_plan) = archivable_plans.first() {
            Ok(Some(Self::archivable_plan_to_plan(archivable_plan)))
        } else {
            Ok(None)
        }
    }

    /// Get plan by its plan_id
    pub fn get_plan_by_id(&self, plan_id: &str) -> RuntimeResult<Option<Plan>> {
        // Query the plan archive for the plan by ID
        if let Some(archivable_plan) = self.plan_archive.get_plan_by_id(&plan_id.to_string()) {
            Ok(Some(Self::archivable_plan_to_plan(&archivable_plan)))
        } else {
            Ok(None)
        }
    }

    /// Helper function to convert ArchivablePlan to Plan
    pub fn archivable_plan_to_plan(
        archivable_plan: &super::archivable_types::ArchivablePlan,
    ) -> Plan {
        // Helper function to parse RTFS or JSON strings (tries RTFS first, falls back to JSON)
        // Uses a simple evaluator to convert RTFS expressions to values
        fn deserialize_value(value_str: &str) -> Option<Value> {
            // Try parsing as RTFS expression first
            if let Ok(expr) = parse_expression(value_str) {
                // Use a simple evaluator to convert expression to value
                // For literals and simple structures, this works directly
                use rtfs::runtime::evaluator::Evaluator;
                use rtfs::runtime::execution_outcome::ExecutionOutcome;
                use rtfs::runtime::module_runtime::ModuleRegistry;
                use rtfs::runtime::pure_host::create_pure_host;
                use rtfs::runtime::security::RuntimeContext;

                let module_registry = ModuleRegistry::new();
                let security_context = RuntimeContext::pure();
                let host = create_pure_host();
                let evaluator = Evaluator::new(
                    std::sync::Arc::new(module_registry),
                    security_context,
                    host,
                    rtfs::compiler::expander::MacroExpander::default(),
                );

                // Try to evaluate the expression
                match evaluator.evaluate(&expr) {
                    Ok(ExecutionOutcome::Complete(value)) => Some(value),
                    _ => {
                        // If evaluation fails, try JSON fallback
                        serde_json::from_str::<JsonValue>(value_str)
                            .ok()
                            .map(Orchestrator::json_value_to_runtime_value)
                    }
                }
            } else {
                // Fall back to JSON parsing for backward compatibility
                serde_json::from_str::<JsonValue>(value_str)
                    .ok()
                    .map(Orchestrator::json_value_to_runtime_value)
            }
        }

        // Helper function to safely deserialize optional JSON/RTFS strings
        let deserialize_optional = |value_str: &Option<String>| -> Option<Value> {
            value_str.as_ref().and_then(|s| deserialize_value(s))
        };

        // Extract the plan body, handling both new String format and legacy steps array format
        let raw_body = match &archivable_plan.body {
            crate::archivable_types::ArchivablePlanBody::String(s) => s.clone(),
            crate::archivable_types::ArchivablePlanBody::Legacy { steps, .. } => {
                steps.first().cloned().unwrap_or_else(|| "()".to_string())
            }
        };

        // If the body is a (plan ...) form, extract the :body property
        // This happens when the plan was saved with the full (plan ...) declaration (legacy format)
        let plan_body = if raw_body.trim().starts_with("(plan") {
            // Try to parse as top-level construct to extract :body from (plan ...) form
            match rtfs::parser::parse(&raw_body) {
                Ok(top_levels) => {
                    // Look for a Plan top-level construct
                    if let Some(rtfs::ast::TopLevel::Plan(plan_def)) = top_levels.first() {
                        // Find the :body property in the plan definition
                        if let Some(body_prop) =
                            plan_def.properties.iter().find(|p| p.key.0 == "body")
                        {
                            // Format the body expression as RTFS string
                            crate::rtfs_bridge::expression_to_rtfs_string(&body_prop.value)
                        } else {
                            raw_body // No :body property found, use as-is
                        }
                    } else {
                        raw_body // Not a Plan top-level, use as-is
                    }
                }
                Err(_) => raw_body, // Parse failed, use as-is
            }
        } else {
            raw_body
        };

        // Convert ArchivablePlan back to Plan
        Plan {
            plan_id: archivable_plan.plan_id.clone(),
            name: archivable_plan.name.clone(),
            intent_ids: archivable_plan.intent_ids.clone(),
            language: super::types::PlanLanguage::Rtfs20, // Default to RTFS 2.0
            body: super::types::PlanBody::Rtfs(plan_body),
            status: archivable_plan.status.clone(),
            created_at: archivable_plan.created_at,
            metadata: archivable_plan
                .metadata
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
            input_schema: deserialize_optional(&archivable_plan.input_schema),
            output_schema: deserialize_optional(&archivable_plan.output_schema),
            policies: archivable_plan
                .policies
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
            capabilities_required: archivable_plan.capabilities_required.clone(),
            annotations: archivable_plan
                .annotations
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
        }
    }

    /// Store a plan in the plan archive
    pub fn store_plan(&self, plan: &Plan) -> RuntimeResult<String> {
        self.plan_archive
            .archive_plan(plan)
            .map_err(|e| RuntimeError::Generic(format!("Failed to archive plan: {}", e)))
    }

    /// Ensure plan is archived - required for causal chain consistency
    /// Returns true if plan was already archived, false if newly archived
    pub fn ensure_plan_archived(&self, plan: &Plan) -> RuntimeResult<bool> {
        if self.plan_archive.get_plan_by_id(&plan.plan_id).is_some() {
            Ok(true) // Already archived
        } else {
            self.store_plan(plan)?;
            Ok(false) // Newly archived
        }
    }

    /// Ensure intent exists in IntentGraph - required for causal chain consistency
    pub fn ensure_intent_exists(&self, intent_id: &IntentId) -> RuntimeResult<()> {
        if intent_id.is_empty() {
            return Ok(()); // Empty intent ID is valid (for capability-internal plans)
        }

        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        if graph.get_intent(intent_id).is_none() {
            return Err(RuntimeError::Generic(format!(
                "Intent {} not found in IntentGraph - cannot ensure causal chain consistency",
                intent_id
            )));
        }

        Ok(())
    }

    /// Validate that all referenced entities (plan, intent) exist before logging action
    /// This ensures causal chain consistency for replay
    pub fn validate_action_prerequisites(
        &self,
        plan_id: &PlanId,
        intent_id: &IntentId,
    ) -> RuntimeResult<()> {
        // Validate plan exists in archive
        if self.plan_archive.get_plan_by_id(plan_id).is_none() {
            return Err(RuntimeError::Generic(format!(
                "Plan {} referenced in action does not exist in PlanArchive - causal chain inconsistency",
                plan_id
            )));
        }

        // Validate intent exists (if provided)
        if !intent_id.is_empty() {
            self.ensure_intent_exists(intent_id)?;
        }

        Ok(())
    }

    /// Reconstruct full execution context from causal chain for replay
    /// Returns actions with their associated Plans and Intents
    pub fn reconstruct_replay_context(&self, plan_id: &PlanId) -> RuntimeResult<ReplayContext> {
        // Get all actions for this plan (clone to own the data)
        let actions = {
            let causal_chain = self
                .causal_chain
                .lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
            causal_chain
                .export_plan_actions(plan_id)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };

        // Get the plan
        let plan = self.get_plan_by_id(plan_id)?.ok_or_else(|| {
            RuntimeError::Generic(format!(
                "Plan {} not found in archive - cannot reconstruct execution context",
                plan_id
            ))
        })?;

        // Get all referenced intents
        let mut intents = Vec::new();
        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        for intent_id in &plan.intent_ids {
            if let Some(intent) = graph.get_intent(intent_id) {
                intents.push(intent.clone());
            }
        }

        // Also collect unique intent IDs from actions (in case some are referenced but not in plan.intent_ids)
        let mut referenced_intent_ids: std::collections::HashSet<String> =
            plan.intent_ids.iter().cloned().collect();
        for action in &actions {
            if !action.intent_id.is_empty() {
                referenced_intent_ids.insert(action.intent_id.clone());
            }
        }

        // Add any missing intents
        for intent_id in referenced_intent_ids {
            if !plan.intent_ids.contains(&intent_id) {
                if let Some(intent) = graph.get_intent(&intent_id) {
                    // Only add if not already collected
                    if !intents.iter().any(|i| i.intent_id == intent_id) {
                        intents.push(intent.clone());
                    }
                }
            }
        }

        Ok(ReplayContext {
            plan,
            intents,
            actions,
        })
    }

    /// Extract exported variables from execution result
    /// This is a simplified version - in practice, you'd analyze the result more carefully
    fn extract_exported_variables(&self, result: &ExecutionResult) -> HashMap<String, RtfsValue> {
        let mut exported = HashMap::new();

        // For now, we'll just extract the result value as a simple export
        // In practice, you'd want to analyze the execution context for variables
        // that were set during execution
        if result.success {
            exported.insert("result".to_string(), result.value.clone());
        }

        exported
    }

    /// Serialize the current execution context from an evaluator (checkpoint helper)
    /// ContextManager removed from RTFS - context serialization now handled by host
    pub fn serialize_context(&self, _evaluator: &Evaluator) -> RuntimeResult<String> {
        // TODO: Implement context serialization at CCOS level
        Ok(String::new())
    }

    /// Restore a serialized execution context into an evaluator (resume helper)
    /// ContextManager removed from RTFS - context deserialization now handled by host
    pub fn deserialize_context(&self, _evaluator: &Evaluator, _data: &str) -> RuntimeResult<()> {
        // TODO: Implement context deserialization at CCOS level
        Ok(())
    }

    /// Create a checkpoint: serialize context and log PlanPaused with checkpoint id
    pub fn checkpoint_plan(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
        missing_capabilities: Option<Vec<String>>,
    ) -> RuntimeResult<(String, String)> {
        let serialized = self.serialize_context(evaluator)?;
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let checkpoint_id = format!("cp-{:x}", hasher.finalize());

        // Log lifecycle event with checkpoint metadata
        let mut chain = self
            .causal_chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        let action = Action::new(
            super::types::ActionType::PlanPaused,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name("checkpoint")
        .with_args(vec![RtfsValue::String(checkpoint_id.clone())]);
        let _ = chain.append(&action)?;

        // Persist checkpoint
        let record = CheckpointRecord {
            checkpoint_id: checkpoint_id.clone(),
            plan_id: plan_id.to_string(),
            intent_id: intent_id.to_string(),
            serialized_context: serialized.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
            missing_capabilities: missing_capabilities.unwrap_or_default(),
            auto_resume_enabled: true, // Enable auto-resume for missing capability checkpoints
        };
        let _id = self
            .checkpoint_archive
            .store(record)
            .map_err(|e| RuntimeError::Generic(format!("Failed to store checkpoint: {}", e)))?;

        Ok((checkpoint_id, serialized))
    }

    /// Resume from a checkpoint: restore context and log PlanResumed with checkpoint id
    pub fn resume_plan(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
        serialized_context: &str,
    ) -> RuntimeResult<()> {
        // Restore
        self.deserialize_context(evaluator, serialized_context)?;

        // Compute checkpoint id for audit linkage
        let mut hasher = Sha256::new();
        hasher.update(serialized_context.as_bytes());
        let checkpoint_id = format!("cp-{:x}", hasher.finalize());

        // Log resume event
        let mut chain = self
            .causal_chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        let action = Action::new(
            super::types::ActionType::PlanResumed,
            plan_id.to_string(),
            intent_id.to_string(),
        )
        .with_name("resume_from_checkpoint")
        .with_args(vec![RtfsValue::String(checkpoint_id)]);
        let _ = chain.append(&action)?;
        Ok(())
    }

    /// Load a checkpoint by id (if present) and resume
    pub fn resume_plan_from_checkpoint(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
        checkpoint_id: &str,
    ) -> RuntimeResult<()> {
        // Try in-memory first, then disk fallback
        let rec = if let Some(rec) = self.checkpoint_archive.get_by_id(checkpoint_id) {
            rec
        } else {
            self.checkpoint_archive
                .load_from_disk(checkpoint_id)
                .ok_or_else(|| RuntimeError::Generic("Checkpoint not found".to_string()))?
        };
        if rec.plan_id != plan_id || rec.intent_id != intent_id {
            return Err(RuntimeError::Generic(
                "Checkpoint does not match plan/intent".to_string(),
            ));
        }
        self.resume_plan(plan_id, intent_id, evaluator, &rec.serialized_context)
    }

    /// Resume execution from a checkpoint and continue running the plan until
    /// either it completes or it yields another RequiresHost (in which case a
    /// new checkpoint is created). This helper will:
    /// - create a fresh Evaluator/RuntimeHost using the provided `context`
    /// - restore the serialized execution context from `checkpoint_id`
    /// - log a PlanResumed event (via resume helpers)
    /// - continue executing the plan body until completion or the next host pause
    ///
    /// Inputs:
    /// - plan: the Plan to execute/continue
    /// - context: the RuntimeContext to use for creating the evaluator/host
    /// - checkpoint_id: id of the previously created checkpoint (e.g. "cp-...")
    ///
    /// Returns: ExecutionResult mirroring `execute_plan` semantics: success=true
    /// on completion, success=false and metadata describing the paused capability
    /// when a new host interaction is required.
    pub async fn resume_and_continue_from_checkpoint(
        &self,
        plan: &Plan,
        context: &RuntimeContext,
        checkpoint_id: &str,
    ) -> RuntimeResult<ExecutionResult> {
        let plan_id = plan.plan_id.clone();
        let primary_intent_id = plan.intent_ids.first().cloned().unwrap_or_default();

        // Ensure the primary intent is marked as Executing so audits are correct
        if !primary_intent_id.is_empty() {
            if let Ok(mut graph) = self.intent_graph.lock() {
                let _ = graph.set_intent_status_with_audit(
                    &primary_intent_id,
                    IntentStatus::Executing,
                    Some(&plan_id),
                    None,
                );
            }
        }

        // --- Recreate Host & Evaluator ---
        let host = Arc::new(RuntimeHost::new(
            self.causal_chain.clone(),
            self.capability_marketplace.clone(),
            context.clone(),
        ));
        host.set_execution_context(plan_id.clone(), plan.intent_ids.clone(), "".to_string());
        let module_registry = std::sync::Arc::new(ModuleRegistry::new());
        let host_iface: Arc<dyn HostInterface> = host.clone();
        let mut evaluator = Evaluator::new(
            module_registry,
            context.clone(),
            host_iface,
            rtfs::compiler::expander::MacroExpander::default(),
        );
        // Load CCOS prelude (effectful helpers) into the evaluator's environment
        crate::prelude::load_prelude(&mut evaluator.env);

        // ContextManager removed from RTFS - step lifecycle now handled by host
        // Resumed context initialization is managed through set_execution_context

        // Restore checkpoint into evaluator (this also logs PlanResumed via resume helpers)
        // Use resume_plan_from_checkpoint which will lookup the checkpoint and call resume_plan
        self.resume_plan_from_checkpoint(&plan_id, &primary_intent_id, &evaluator, checkpoint_id)?;

        // Parse and continue executing the plan body
        let final_result = match &plan.language {
            PlanLanguage::Rtfs20 => match &plan.body {
                PlanBody::Rtfs(rtfs_code) => {
                    let code = rtfs_code.trim();
                    if code.is_empty() {
                        Err(RuntimeError::Generic(
                            "Empty RTFS plan body after trimming".to_string(),
                        ))
                    } else {
                        match parse_expression(code) {
                            Ok(expr) => self.execute_with_yield_handling(&evaluator, &expr).await,
                            Err(e) => Err(RuntimeError::Generic(format!(
                                "Failed to parse RTFS plan body: {:?}",
                                e
                            ))),
                        }
                    }
                }
                PlanBody::Wasm(_) => Err(RuntimeError::Generic(
                    "RTFS plans must use Rtfs body format".to_string(),
                )),
            },
            _ => Err(RuntimeError::Generic(format!(
                "Unsupported plan language: {:?}",
                plan.language
            ))),
        };

        host.clear_execution_context();

        // --- Finalize & audit similar to execute_plan ---
        let (execution_result, error_opt) = match final_result {
            Ok(ExecutionOutcome::Complete(value)) => {
                let res = ExecutionResult {
                    success: true,
                    value,
                    metadata: Default::default(),
                };
                let _ = self.log_action(
                    Action::new(
                        ActionType::PlanCompleted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(None)
                    .with_result(res.clone()),
                );
                (res, None)
            }
            Ok(ExecutionOutcome::RequiresHost(host_call)) => {
                // Create a new checkpoint and emit PlanPaused
                let missing_capabilities = vec![host_call.capability_id.clone()];
                let (checkpoint_id, _serialized) = self.checkpoint_plan(
                    &plan_id,
                    &primary_intent_id,
                    &evaluator,
                    Some(missing_capabilities),
                )?;

                let mut metadata_map: std::collections::HashMap<String, RtfsValue> =
                    std::collections::HashMap::new();
                metadata_map.insert(
                    "requires_capability".to_string(),
                    RtfsValue::String(host_call.capability_id.clone()),
                );
                if host_call.metadata.is_some() {
                    metadata_map.insert(
                        "has_metadata".to_string(),
                        RtfsValue::String("true".to_string()),
                    );
                }

                let res = ExecutionResult {
                    success: false,
                    value: RtfsValue::String("paused: requires host interaction".to_string()),
                    metadata: metadata_map,
                };
                (res, None)
            }
            Err(e) => {
                let _ = self.log_action(
                    Action::new(
                        ActionType::PlanAborted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(None)
                    .with_error(&e.to_string()),
                );
                let failure_value = RtfsValue::String(format!("error: {}", e));
                let res = ExecutionResult {
                    success: false,
                    value: failure_value,
                    metadata: Default::default(),
                };
                (res, Some(e))
            }
        };

        // Update Intent Graph with final outcome
        {
            let mut graph = self
                .intent_graph
                .lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

            if !primary_intent_id.is_empty() {
                if let Some(pre_intent) = graph.get_intent(&primary_intent_id) {
                    graph
                        .update_intent_with_audit(
                            pre_intent,
                            &execution_result,
                            Some(&plan_id),
                            None,
                        )
                        .map_err(|e| {
                            RuntimeError::Generic(format!(
                                "IntentGraph update failed for {}: {:?}",
                                primary_intent_id, e
                            ))
                        })?;
                }
            }
        }

        if let Some(err) = error_opt {
            Err(err)
        } else {
            Ok(execution_result)
        }
    }

    /// Derive and set the security profile for a step before execution
    pub fn derive_step_profile(
        &mut self,
        step_name: &str,
        step_expr: &Expression,
        runtime_context: &RuntimeContext,
    ) -> RuntimeResult<()> {
        let profile = StepProfileDeriver::derive_profile(step_name, step_expr, runtime_context)?;
        self.current_step_profile = Some(profile.clone());

        // Log the step profile derivation to the causal chain
        let mut profile_action = Action::new(
            ActionType::StepProfileDerived,
            "plan-execution".to_string(),
            "step-security".to_string(),
        )
        .with_name(&format!("derive_step_profile_{}", step_name))
        .with_args(vec![
            RtfsValue::String(profile.profile_id.clone()),
            RtfsValue::String(format!("{:?}", profile.isolation_level)),
            RtfsValue::String(format!("deterministic: {}", profile.deterministic)),
        ]);

        // Set metadata directly
        profile_action.metadata.insert(
            "step_name".to_string(),
            RtfsValue::String(step_name.to_string()),
        );
        profile_action.metadata.insert(
            "network_policy".to_string(),
            RtfsValue::String(format!("{:?}", profile.microvm_config.network_policy)),
        );
        profile_action.metadata.insert(
            "fs_policy".to_string(),
            RtfsValue::String(format!("{:?}", profile.microvm_config.fs_policy)),
        );
        profile_action.metadata.insert(
            "resource_limits".to_string(),
            RtfsValue::String(format!(
                "time: {}ms, mem: {}MB, cpu: {}x",
                profile.resource_limits.max_execution_time_ms,
                profile.resource_limits.max_memory_bytes / (1024 * 1024),
                profile.resource_limits.max_cpu_usage
            )),
        );

        self.log_action(profile_action)?;
        Ok(())
    }

    /// Get the current step profile (for use by runtime components)
    pub fn get_current_step_profile(&self) -> Option<&StepProfile> {
        self.current_step_profile.as_ref()
    }

    /// Clear the current step profile after step completion
    pub fn clear_step_profile(&mut self) {
        self.current_step_profile = None;
    }

    /// Helper to log an action to the Causal Chain.
    /// Validates that referenced plan and intent exist before logging to ensure consistency.
    fn log_action(&self, action: Action) -> RuntimeResult<String> {
        // Validate prerequisites before logging - ensures causal chain consistency
        self.validate_action_prerequisites(&action.plan_id, &action.intent_id)?;

        let mut chain = self
            .causal_chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        chain.append(&action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_sink::CausalChainIntentEventSink;
    use crate::types::{PlanStatus, StorableIntent};
    use rtfs::ast::Symbol;
    use rtfs::runtime::security::SecurityLevel;

    fn test_context() -> RuntimeContext {
        RuntimeContext {
            security_level: SecurityLevel::Controlled,
            ..RuntimeContext::pure()
        }
    }

    fn make_graph_with_sink(chain: Arc<Mutex<CausalChain>>) -> Arc<Mutex<IntentGraph>> {
        let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&chain)));
        Arc::new(Mutex::new(
            IntentGraph::with_event_sink(sink).expect("intent graph"),
        ))
    }

    fn collect_status_changes(chain: &CausalChain, intent_id: &str) -> Vec<Action> {
        let mut out = Vec::new();
        for a in chain.get_actions_for_intent(&intent_id.to_string()) {
            if a.action_type == ActionType::IntentStatusChanged {
                out.push((*a).clone());
            }
        }
        out
    }

    #[tokio::test]
    async fn orchestrator_emits_executing_and_completed() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Seed an Active intent
        let stored = StorableIntent::new("test goal".to_string());
        let intent_id = stored.intent_id.clone();
        {
            let mut g = graph.lock().unwrap();
            g.store_intent(stored.clone()).expect("store intent");
        }

        // Minimal RTFS plan body that evaluates successfully
        let mut plan = Plan::new_rtfs("42".to_string(), vec![intent_id.clone()]);
        plan.status = PlanStatus::Active;

        let ctx = test_context();
        let result = _orchestrator
            .execute_plan(&plan, &ctx)
            .await
            .expect("exec ok");
        assert!(result.success);

        // Verify status changes were audited: Active->Executing, Executing->Completed
        let changes = {
            let guard = chain.lock().unwrap();
            collect_status_changes(&guard, &intent_id)
        };
        assert!(
            changes.len() >= 2,
            "expected at least 2 status change actions, got {}",
            changes.len()
        );

        // Find specific transitions via metadata
        let mut saw_active_to_executing = false;
        let mut saw_executing_to_completed = false;
        let mut saw_triggering = false;
        let mut saw_reason_set = false;
        for a in &changes {
            let old_s = a.metadata.get("old_status").and_then(|v| v.as_string());
            let new_s = a.metadata.get("new_status").and_then(|v| v.as_string());
            if a.metadata.get("triggering_action_id").is_some() {
                saw_triggering = true;
            }
            if let Some(reason) = a.metadata.get("reason").and_then(|v| v.as_string()) {
                if reason == "IntentGraph: explicit status set"
                    || reason == "IntentGraph: update_intent result"
                {
                    saw_reason_set = true;
                }
            }
            match (old_s.as_deref(), new_s.as_deref()) {
                (Some("Active"), Some("Executing")) => saw_active_to_executing = true,
                (Some("Executing"), Some("Completed")) => saw_executing_to_completed = true,
                _ => {}
            }
        }
        assert!(
            saw_active_to_executing,
            "missing Active->Executing status change"
        );
        assert!(
            saw_executing_to_completed,
            "missing Executing->Completed status change"
        );
        assert!(
            saw_triggering,
            "missing triggering_action_id metadata on status change"
        );
        assert!(
            saw_reason_set,
            "missing expected reason metadata on status change"
        );
    }

    #[tokio::test]
    async fn orchestrator_emits_failed_on_error() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Seed an Active intent
        let stored = StorableIntent::new("test goal".to_string());
        let intent_id = stored.intent_id.clone();
        {
            let mut g = graph.lock().unwrap();
            g.store_intent(stored).expect("store intent");
        }

        // Invalid RTFS to trigger parse error
        let mut plan = Plan::new_rtfs("(this is not valid".to_string(), vec![intent_id.clone()]);
        plan.status = PlanStatus::Active;

        let ctx = test_context();
        let res = _orchestrator.execute_plan(&plan, &ctx).await;
        assert!(res.is_err(), "expected parse error");

        // Verify status changes were audited: Active->Executing, Executing->Failed
        let changes = {
            let guard = chain.lock().unwrap();
            collect_status_changes(&guard, &intent_id)
        };
        assert!(
            changes.len() >= 2,
            "expected at least 2 status change actions, got {}",
            changes.len()
        );

        let mut saw_active_to_executing = false;
        let mut saw_executing_to_failed = false;
        let mut saw_triggering = false;
        let mut saw_reason_set = false;
        for a in &changes {
            let old_s = a.metadata.get("old_status").and_then(|v| v.as_string());
            let new_s = a.metadata.get("new_status").and_then(|v| v.as_string());
            if a.metadata.get("triggering_action_id").is_some() {
                saw_triggering = true;
            }
            if let Some(reason) = a.metadata.get("reason").and_then(|v| v.as_string()) {
                if reason == "IntentGraph: explicit status set"
                    || reason == "IntentGraph: update_intent result"
                {
                    saw_reason_set = true;
                }
            }
            match (old_s.as_deref(), new_s.as_deref()) {
                (Some("Active"), Some("Executing")) => saw_active_to_executing = true,
                (Some("Executing"), Some("Failed")) => saw_executing_to_failed = true,
                _ => {}
            }
        }
        assert!(
            saw_active_to_executing,
            "missing Active->Executing status change"
        );
        assert!(
            saw_executing_to_failed,
            "missing Executing->Failed status change"
        );
        assert!(
            saw_triggering,
            "missing triggering_action_id metadata on status change"
        );
        assert!(
            saw_reason_set,
            "missing expected reason metadata on status change"
        );
    }

    #[tokio::test]
    async fn test_step_profile_derivation_safe_operations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test pure function call - should get Inherit isolation
        let pure_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("math.add".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("add-numbers", &pure_expr, &context).unwrap();

        assert_eq!(profile.isolation_level, IsolationLevel::Inherit);
        assert_eq!(profile.deterministic, true);
        assert_eq!(profile.security_flags.enable_syscall_filter, false);
        assert_eq!(profile.microvm_config.network_policy, NetworkPolicy::Denied);
        assert_eq!(profile.microvm_config.fs_policy, FileSystemPolicy::None);
    }

    #[tokio::test]
    async fn test_step_profile_derivation_network_operations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test network operation - should get Isolated isolation
        let network_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("http.fetch".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("https://api.example.com/data".to_string())),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("fetch-data", &network_expr, &context).unwrap();

        assert_eq!(profile.isolation_level, IsolationLevel::Isolated);
        assert_eq!(profile.deterministic, false);
        assert_eq!(profile.security_flags.enable_network_acl, true);
        println!("TEST DEBUG PROFILE (network): {:?}", profile);
        println!(
            "TEST DEBUG: profile.security_flags.enable_syscall_filter = {}",
            profile.security_flags.enable_syscall_filter
        );
        eprintln!(
            "TEST DEBUG ERR: syscall_filter = {} flags = {:?}",
            profile.security_flags.enable_syscall_filter, profile.security_flags
        );
        dbg!(profile.security_flags.enable_syscall_filter);
        assert_eq!(profile.security_flags.enable_syscall_filter, false);

        // Check that network policy allows specific domains
        if let NetworkPolicy::AllowList(domains) = &profile.microvm_config.network_policy {
            assert!(domains.contains(&"api.example.com".to_string()));
        } else {
            panic!("Expected AllowList network policy");
        }
    }

    #[tokio::test]
    async fn test_step_profile_derivation_file_operations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test file operation - should get Isolated isolation
        let file_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("file.read".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("/data/input.txt".to_string())),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("read-file", &file_expr, &context).unwrap();

        assert_eq!(profile.isolation_level, IsolationLevel::Isolated);
        assert_eq!(profile.deterministic, false);
        assert_eq!(profile.security_flags.enable_fs_acl, true);
        eprintln!(
            "TEST DEBUG ERR (file): syscall_filter = {} flags = {:?}",
            profile.security_flags.enable_syscall_filter, profile.security_flags
        );
        println!("TEST DEBUG PROFILE (file): {:?}", profile);

        // Check that filesystem policy allows specific paths
        if let FileSystemPolicy::ReadWrite(paths) = &profile.microvm_config.fs_policy {
            assert!(paths.contains(&"/tmp".to_string()));
            assert!(paths.contains(&"/app/data".to_string()));
        } else {
            panic!("Expected ReadWrite filesystem policy");
        }
    }

    #[tokio::test]
    async fn test_step_profile_derivation_system_operations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test system operation - should get Sandboxed isolation
        let system_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("system.execute".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("ls -la".to_string())),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("list-files", &system_expr, &context).unwrap();

        assert_eq!(profile.isolation_level, IsolationLevel::Sandboxed);
        assert_eq!(profile.deterministic, false);
        assert_eq!(profile.security_flags.enable_syscall_filter, true);
        assert_eq!(profile.security_flags.log_syscalls, true);
        assert_eq!(profile.security_flags.read_only_fs, true);
    }

    #[tokio::test]
    async fn test_step_profile_resource_limits() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test computationally intensive operation gets higher limits
        let intensive_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("compute.analyze".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Symbol(Symbol("big_dataset".to_string())),
            ]),
        ]);

        let mut context = test_context();
        context.max_execution_time = None;
        context.max_memory_usage = None;
        let profile =
            StepProfileDeriver::derive_profile("analyze-data", &intensive_expr, &context).unwrap();

        // Intensive operations should get higher resource limits
        assert!(profile.resource_limits.max_execution_time_ms >= 300000); // 5+ minutes
        assert!(profile.resource_limits.max_memory_bytes >= 1024 * 1024 * 1024); // 1+ GB
        assert!(profile.resource_limits.max_cpu_usage >= 2.0); // Multi-core
    }

    #[tokio::test]
    async fn test_step_profile_runtime_context_constraints() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test that runtime context constraints are respected
        let file_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("file.read".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("/data/input.txt".to_string())),
            ]),
        ]);

        // Create a context that doesn't allow isolated isolation
        let mut context = test_context();
        context.allow_isolated_isolation = false;

        let profile =
            StepProfileDeriver::derive_profile("read-file", &file_expr, &context).unwrap();

        // Should be downgraded to Inherit since Isolated is not allowed
        assert_eq!(profile.isolation_level, IsolationLevel::Inherit);
    }

    #[tokio::test]
    async fn test_step_profile_causal_chain_logging() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        let network_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("http.fetch".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("https://api.example.com/data".to_string())),
            ]),
        ]);

        let context = test_context();

        // Derive profile and check that it's logged to causal chain
        _orchestrator
            .derive_step_profile("test-step", &network_expr, &context)
            .unwrap();

        // Check that a StepProfileDerived action was logged
        let profile_action_exists = {
            let guard = chain.lock().unwrap();
            let actions = guard.get_actions_for_intent(&"step-security".to_string());
            actions
                .iter()
                .any(|a| a.action_type == ActionType::StepProfileDerived)
        };
        assert!(
            profile_action_exists,
            "StepProfileDerived action should be logged"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_step_profile_management() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        let expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("math.add".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::Integer(1)),
                Expression::Literal(Literal::Integer(2)),
            ]),
        ]);

        let context = test_context();

        // Initially no profile should be set
        assert!(_orchestrator.get_current_step_profile().is_none());

        // Derive and set profile
        _orchestrator
            .derive_step_profile("test-math", &expr, &context)
            .unwrap();
        assert!(_orchestrator.get_current_step_profile().is_some());

        // Clear profile
        _orchestrator.clear_step_profile();
        assert!(_orchestrator.get_current_step_profile().is_none());
    }

    #[tokio::test]
    async fn test_security_flag_combinations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test that dangerous operations get comprehensive security flags
        let dangerous_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("system.execute".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("sudo rm -rf /".to_string())),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("dangerous-op", &dangerous_expr, &context).unwrap();

        // Dangerous operations should have all security flags enabled
        assert_eq!(profile.security_flags.enable_syscall_filter, true);
        assert_eq!(profile.security_flags.log_syscalls, true);
        assert_eq!(profile.security_flags.read_only_fs, true);
        assert_eq!(profile.security_flags.enable_memory_protection, true);
        assert_eq!(profile.security_flags.enable_cpu_monitoring, true);
    }

    #[tokio::test]
    async fn test_network_bandwidth_limits() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test that network operations get appropriate bandwidth limits
        let network_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("http.download".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String(
                    "https://example.com/large-file.zip".to_string(),
                )),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("download-file", &network_expr, &context).unwrap();

        // Network operations should have bandwidth limits
        assert!(profile.resource_limits.max_network_bandwidth.is_some());
        assert!(profile.resource_limits.max_network_bandwidth.unwrap() >= 10 * 1024 * 1024);
        // At least 10MB/s
    }

    #[tokio::test]
    async fn test_data_operations_deterministic() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Arc::new(
            tokio::sync::RwLock::new(crate::capabilities::registry::CapabilityRegistry::new()),
        )));
        let plan_archive = Arc::new(PlanArchive::new());
        let mut _orchestrator = Orchestrator::new(
            Arc::clone(&chain),
            Arc::clone(&graph),
            Arc::clone(&marketplace),
            Arc::clone(&plan_archive),
        );

        // Test that data operations are marked as deterministic
        let data_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("data.parse-json".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("{\"key\": \"value\"}".to_string())),
            ]),
        ]);

        let context = test_context();
        let profile =
            StepProfileDeriver::derive_profile("parse-json", &data_expr, &context).unwrap();

        assert_eq!(profile.deterministic, true);
        assert_eq!(profile.isolation_level, IsolationLevel::Inherit);
    }
}
