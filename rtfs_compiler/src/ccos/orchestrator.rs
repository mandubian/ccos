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

use std::sync::{Arc, Mutex};
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::security::RuntimeContext;
use crate::runtime::evaluator::Evaluator;
use crate::runtime::host::RuntimeHost;
use crate::runtime::error::{RuntimeResult, RuntimeError};
use crate::parser::parse_expression;
use crate::runtime::microvm::config::{MicroVMConfig, NetworkPolicy, FileSystemPolicy};
use crate::ccos::execution_context::IsolationLevel;

use super::causal_chain::CausalChain;
use super::intent_graph::IntentGraph;
use super::types::{Plan, Action, ActionType, ExecutionResult, PlanLanguage, PlanBody, IntentStatus};
use crate::ast::{Expression, Literal, Symbol};

use crate::runtime::module_runtime::ModuleRegistry;
use crate::ccos::delegation::{DelegationEngine, StaticDelegationEngine};
use crate::runtime::host_interface::HostInterface;
use std::collections::HashMap;
use sha2::{Digest, Sha256};
use super::checkpoint_archive::{CheckpointArchive, CheckpointRecord};
use crate::runtime::values::Value as RtfsValue;
use chrono;

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
            },
            // Network operations requiring isolation
            Expression::List(exprs) if Self::contains_network_operations(exprs) => {
                IsolationLevel::Isolated
            },
            // File operations requiring isolation
            Expression::List(exprs) if Self::contains_file_operations(exprs) => {
                IsolationLevel::Isolated
            },
            // System operations requiring isolation
            Expression::List(exprs) if Self::contains_system_operations(exprs) => {
                IsolationLevel::Isolated
            },
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
            },
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
            },
            // Math operations are deterministic
            Expression::List(exprs) if Self::contains_math_operations(exprs) => {
                println!("DEBUG DETERMINISM: math_ops=true");
                true
            },
            // Data parsing/serialization is deterministic
            Expression::List(exprs) if Self::contains_data_operations(exprs) => {
                println!("DEBUG DETERMINISM: data_ops=true");
                true
            },
            // I/O operations are generally non-deterministic
            Expression::List(exprs) if Self::contains_io_operations(exprs) => {
                println!("DEBUG DETERMINISM: io_ops=true");
                false
            },
            // Network operations are non-deterministic
            Expression::List(exprs) if Self::contains_network_operations(exprs) => {
                println!("DEBUG DETERMINISM: network_ops=true");
                false
            },
            // Default to non-deterministic for safety
            _ => {
                println!("DEBUG DETERMINISM: default=false");
                false
            },
        }
    }

    /// Derive resource limits based on step complexity
    fn derive_resource_limits(step_expr: &Expression) -> ResourceLimits {
        let base_limits = ResourceLimits {
            max_execution_time_ms: 30000, // 30 seconds default
            max_memory_bytes: 256 * 1024 * 1024, // 256MB default
            max_cpu_usage: 1.0, // Single core default
            max_io_operations: Some(1000),
            max_network_bandwidth: Some(1024 * 1024), // 1MB/s default
        };

        match step_expr {
            Expression::List(exprs) => {
                if Self::is_computationally_intensive(exprs) {
                    ResourceLimits {
                        max_execution_time_ms: 300000, // 5 minutes for intensive tasks
                        max_memory_bytes: 1024 * 1024 * 1024, // 1GB
                        max_cpu_usage: 2.0, // Multi-core
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
            },
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
                
                println!("DEBUG: has_system={}, has_network={}, has_file={}", has_system, has_network, has_file);
                println!("DEBUG: Before flags - enable_syscall_filter={}", flags.enable_syscall_filter);
                
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
                
                println!("DEBUG: After flags - enable_syscall_filter={}", flags.enable_syscall_filter);
            },
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
        if !context.is_isolation_allowed(&profile.isolation_level) {
            match profile.isolation_level {
                IsolationLevel::Sandboxed => {
                    if context.allow_isolated_isolation {
                        profile.isolation_level = IsolationLevel::Isolated;
                    } else {
                        profile.isolation_level = IsolationLevel::Inherit;
                    }
                },
                IsolationLevel::Isolated => {
                    profile.isolation_level = IsolationLevel::Inherit;
                },
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
            profile.resource_limits.max_execution_time_ms,
            profile.resource_limits.max_memory_bytes
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
        let network_capabilities = ["http-fetch", "http.fetch", "network", "socket", "fetch", "http"]; // include generic http fallback
        if Self::contains_capabilities(exprs, &network_capabilities) { return true; }
        if let Some(name) = Self::extract_call_capability(exprs) {
            return network_capabilities.iter().any(|cap| name.contains(cap));
        }
        false
    }

    fn contains_file_operations(exprs: &[Expression]) -> bool {
        let file_capabilities = ["file", "io", "read", "write", "open"];
        if Self::contains_capabilities(exprs, &file_capabilities) { return true; }
        if let Some(name) = Self::extract_call_capability(exprs) {
            return file_capabilities.iter().any(|cap| name.contains(cap));
        }
        false
    }

    fn contains_system_operations(exprs: &[Expression]) -> bool {
        let system_capabilities = ["system", "exec", "shell", "process"];
        if Self::contains_capabilities(exprs, &system_capabilities) { return true; }
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
            if n.starts_with("data.") || n.contains("parse") || n.contains("serialize") || n.contains("json") {
                return true;
            }
        }
        // Scan symbols recursively but ignore string literals to avoid false positives like URLs containing "data"
        for e in exprs {
            match e {
                Expression::Symbol(sym) => {
                    let n = sym.0.to_lowercase();
                    if n.starts_with("data.") || n.contains("parse") || n.contains("serialize") || n.contains("json") {
                        return true;
                    }
                },
                Expression::List(list_exprs) => {
                    if Self::contains_data_operations(list_exprs) { return true; }
                },
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
        } else { false };
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
                if n == "system.execute" || n.contains("exec") || n.contains("shell") || n.contains("process.run") || n == "system" {
                    return true;
                }
            }
        }
        false
    }

    fn contains_capabilities(exprs: &[Expression], capabilities: &[&str]) -> bool {
        exprs.iter().any(|expr| {
            match expr {
                Expression::Symbol(name) => {
                    let name_lower = name.0.to_lowercase();
                    capabilities.iter().any(|cap| name_lower.contains(cap))
                },
                Expression::Literal(Literal::String(s)) => {
                    let s_lower = s.to_lowercase();
                    capabilities.iter().any(|cap| s_lower.contains(cap))
                },
                Expression::List(list_exprs) => Self::contains_capabilities(list_exprs, capabilities),
                _ => false,
            }
        })
    }
}

/// The Orchestrator is the stateful engine that drives plan execution.
pub struct Orchestrator {
    causal_chain: Arc<Mutex<CausalChain>>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    checkpoint_archive: Arc<CheckpointArchive>,
    /// Current step profile being executed (for step-level security enforcement)
    current_step_profile: Option<StepProfile>,
}

impl Orchestrator {
    /// Creates a new Orchestrator.
    pub fn new(
        causal_chain: Arc<Mutex<CausalChain>>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        capability_marketplace: Arc<CapabilityMarketplace>,
    ) -> Self {
        Self {
            causal_chain,
            intent_graph,
            capability_marketplace,
            checkpoint_archive: Arc::new(CheckpointArchive::new()),
            current_step_profile: None,
        }
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

        // --- 1. Log PlanStarted Action ---
        let plan_action_id = self.log_action(
            Action::new(
                ActionType::PlanStarted,
                plan_id.clone(),
                primary_intent_id.clone(),
            ).with_parent(None)
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
        host.set_execution_context(plan_id.clone(), plan.intent_ids.clone(), plan_action_id.clone());
    let module_registry = std::sync::Arc::new(ModuleRegistry::new());
        let delegation_engine: Arc<dyn DelegationEngine> = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        let host_iface: Arc<dyn HostInterface> = host.clone();
        let evaluator = Evaluator::new(module_registry, delegation_engine, context.clone(), host_iface);
        
        // Initialize context manager for the plan execution
        {
            let mut context_manager = evaluator.context_manager.borrow_mut();
            context_manager.initialize(Some(format!("plan-{}", plan_id)));
        }

        // --- 3. Parse and Execute the Plan Body ---
        let final_result = match &plan.language {
            PlanLanguage::Rtfs20 => {
                match &plan.body {
                    PlanBody::Rtfs(rtfs_code) => {
                        let code = rtfs_code.trim();
                        if code.is_empty() {
                            Err(RuntimeError::Generic("Empty RTFS plan body after trimming".to_string()))
                        } else {
                            match parse_expression(code) {
                                Ok(expr) => evaluator.evaluate(&expr),
                                Err(e) => Err(RuntimeError::Generic(format!("Failed to parse RTFS plan body: {:?}", e))),
                            }
                        }
                    }
                    PlanBody::Wasm(_) => Err(RuntimeError::Generic("RTFS plans must use Rtfs body format".to_string())),
                }
            }
            _ => Err(RuntimeError::Generic(format!("Unsupported plan language: {:?}", plan.language))),
        };

        host.clear_execution_context();

        // --- 4. Log Final Plan Status ---
        // Construct execution_result while ensuring we still update the IntentGraph on failure.
        let (execution_result, error_opt) = match final_result {
            Ok(value) => {
                let res = ExecutionResult { success: true, value, metadata: Default::default() };
                self.log_action(
                    Action::new(
                        ActionType::PlanCompleted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_result(res.clone())
                )?;
                (res, None)
            },
            Err(e) => {
                // Log aborted action first
                self.log_action(
                    Action::new(
                        ActionType::PlanAborted,
                        plan_id.clone(),
                        primary_intent_id.clone(),
                    )
                    .with_parent(Some(plan_action_id.clone()))
                    .with_error(&e.to_string())
                )?;
                // Represent failure value explicitly (string) so ExecutionResult always has a Value
                let failure_value = RtfsValue::String(format!("error: {}", e));
                let res = ExecutionResult { success: false, value: failure_value, metadata: Default::default() };
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
                        .map_err(|e| RuntimeError::Generic(format!(
                            "IntentGraph update failed for {}: {:?}",
                            primary_intent_id, e
                        )))?;
                }
            }
        }

    // Propagate original error after updating intent status
    if let Some(err) = error_opt { Err(err) } else { Ok(execution_result) }
    }

    /// Serialize the current execution context from an evaluator (checkpoint helper)
    pub fn serialize_context(&self, evaluator: &Evaluator) -> RuntimeResult<String> {
        evaluator
            .context_manager
            .borrow()
            .serialize()
    }

    /// Restore a serialized execution context into an evaluator (resume helper)
    pub fn deserialize_context(&self, evaluator: &Evaluator, data: &str) -> RuntimeResult<()> {
        evaluator
            .context_manager
            .borrow_mut()
            .deserialize(data)
    }

    /// Create a checkpoint: serialize context and log PlanPaused with checkpoint id
    pub fn checkpoint_plan(
        &self,
        plan_id: &str,
        intent_id: &str,
        evaluator: &Evaluator,
    ) -> RuntimeResult<(String, String)> {
        let serialized = self.serialize_context(evaluator)?;
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let checkpoint_id = format!("cp-{:x}", hasher.finalize());

        // Log lifecycle event with checkpoint metadata
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        let action = Action::new(super::types::ActionType::PlanPaused, plan_id.to_string(), intent_id.to_string())
            .with_name("checkpoint")
            .with_args(vec![RtfsValue::String(checkpoint_id.clone())]);
        let _ = chain.append(&action)?;

        // Persist checkpoint
        let record = CheckpointRecord {
            checkpoint_id: checkpoint_id.clone(),
            plan_id: plan_id.to_string(),
            intent_id: intent_id.to_string(),
            serialized_context: serialized.clone(),
            created_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            metadata: HashMap::new(),
        };
        let _id = self.checkpoint_archive.store(record)
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
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        let action = Action::new(super::types::ActionType::PlanResumed, plan_id.to_string(), intent_id.to_string())
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
            return Err(RuntimeError::Generic("Checkpoint does not match plan/intent".to_string()));
        }
        self.resume_plan(plan_id, intent_id, evaluator, &rec.serialized_context)
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
        profile_action.metadata.insert("step_name".to_string(), RtfsValue::String(step_name.to_string()));
        profile_action.metadata.insert("network_policy".to_string(), RtfsValue::String(format!("{:?}", profile.microvm_config.network_policy)));
        profile_action.metadata.insert("fs_policy".to_string(), RtfsValue::String(format!("{:?}", profile.microvm_config.fs_policy)));
        profile_action.metadata.insert("resource_limits".to_string(), RtfsValue::String(format!(
            "time: {}ms, mem: {}MB, cpu: {}x",
            profile.resource_limits.max_execution_time_ms,
            profile.resource_limits.max_memory_bytes / (1024 * 1024),
            profile.resource_limits.max_cpu_usage
        )));

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
    fn log_action(&self, action: Action) -> RuntimeResult<String> {
        let mut chain = self.causal_chain.lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))?;
        chain.append(&action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::event_sink::CausalChainIntentEventSink;
    use crate::ccos::types::{PlanStatus, StorableIntent};
    use crate::runtime::security::SecurityLevel;

    fn test_context() -> RuntimeContext {
        RuntimeContext {
            security_level: SecurityLevel::Controlled,
            ..RuntimeContext::pure()
        }
    }

    fn make_graph_with_sink(chain: Arc<Mutex<CausalChain>>) -> Arc<Mutex<IntentGraph>> {
        let sink = Arc::new(CausalChainIntentEventSink::new(Arc::clone(&chain)));
        Arc::new(Mutex::new(IntentGraph::with_event_sink(sink).expect("intent graph")))
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
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let result = _orchestrator.execute_plan(&plan, &ctx).await.expect("exec ok");
        assert!(result.success);

        // Verify status changes were audited: Active->Executing, Executing->Completed
        let changes = {
            let guard = chain.lock().unwrap();
            collect_status_changes(&guard, &intent_id)
        };
        assert!(changes.len() >= 2, "expected at least 2 status change actions, got {}", changes.len());

        // Find specific transitions via metadata
        let mut saw_active_to_executing = false;
        let mut saw_executing_to_completed = false;
        let mut saw_triggering = false;
        let mut saw_reason_set = false;
        for a in &changes {
            let old_s = a.metadata.get("old_status").and_then(|v| v.as_string());
            let new_s = a.metadata.get("new_status").and_then(|v| v.as_string());
            if a.metadata.get("triggering_action_id").is_some() { saw_triggering = true; }
            if let Some(reason) = a.metadata.get("reason").and_then(|v| v.as_string()) {
                if reason == "IntentGraph: explicit status set" || reason == "IntentGraph: update_intent result" {
                    saw_reason_set = true;
                }
            }
            match (old_s.as_deref(), new_s.as_deref()) {
                (Some("Active"), Some("Executing")) => saw_active_to_executing = true,
                (Some("Executing"), Some("Completed")) => saw_executing_to_completed = true,
                _ => {}
            }
        }
        assert!(saw_active_to_executing, "missing Active->Executing status change");
        assert!(saw_executing_to_completed, "missing Executing->Completed status change");
        assert!(saw_triggering, "missing triggering_action_id metadata on status change");
        assert!(saw_reason_set, "missing expected reason metadata on status change");
    }

    #[tokio::test]
    async fn orchestrator_emits_failed_on_error() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        assert!(changes.len() >= 2, "expected at least 2 status change actions, got {}", changes.len());

        let mut saw_active_to_executing = false;
        let mut saw_executing_to_failed = false;
        let mut saw_triggering = false;
        let mut saw_reason_set = false;
        for a in &changes {
            let old_s = a.metadata.get("old_status").and_then(|v| v.as_string());
            let new_s = a.metadata.get("new_status").and_then(|v| v.as_string());
            if a.metadata.get("triggering_action_id").is_some() { saw_triggering = true; }
            if let Some(reason) = a.metadata.get("reason").and_then(|v| v.as_string()) {
                if reason == "IntentGraph: explicit status set" || reason == "IntentGraph: update_intent result" {
                    saw_reason_set = true;
                }
            }
            match (old_s.as_deref(), new_s.as_deref()) {
                (Some("Active"), Some("Executing")) => saw_active_to_executing = true,
                (Some("Executing"), Some("Failed")) => saw_executing_to_failed = true,
                _ => {}
            }
        }
        assert!(saw_active_to_executing, "missing Active->Executing status change");
        assert!(saw_executing_to_failed, "missing Executing->Failed status change");
        assert!(saw_triggering, "missing triggering_action_id metadata on status change");
        assert!(saw_reason_set, "missing expected reason metadata on status change");
    }

    #[tokio::test]
    async fn test_step_profile_derivation_safe_operations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("add-numbers", &pure_expr, &context).unwrap();

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
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("fetch-data", &network_expr, &context).unwrap();

        assert_eq!(profile.isolation_level, IsolationLevel::Isolated);
        assert_eq!(profile.deterministic, false);
        assert_eq!(profile.security_flags.enable_network_acl, true);
        println!("TEST DEBUG PROFILE (network): {:?}", profile);
        println!("TEST DEBUG: profile.security_flags.enable_syscall_filter = {}", profile.security_flags.enable_syscall_filter);
        eprintln!("TEST DEBUG ERR: syscall_filter = {} flags = {:?}", profile.security_flags.enable_syscall_filter, profile.security_flags);
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
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("read-file", &file_expr, &context).unwrap();

        assert_eq!(profile.isolation_level, IsolationLevel::Isolated);
        assert_eq!(profile.deterministic, false);
        assert_eq!(profile.security_flags.enable_fs_acl, true);
        eprintln!("TEST DEBUG ERR (file): syscall_filter = {} flags = {:?}", profile.security_flags.enable_syscall_filter, profile.security_flags);
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
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("list-files", &system_expr, &context).unwrap();

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
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("analyze-data", &intensive_expr, &context).unwrap();

        // Intensive operations should get higher resource limits
        assert!(profile.resource_limits.max_execution_time_ms >= 300000); // 5+ minutes
        assert!(profile.resource_limits.max_memory_bytes >= 1024 * 1024 * 1024); // 1+ GB
        assert!(profile.resource_limits.max_cpu_usage >= 2.0); // Multi-core
    }

    #[tokio::test]
    async fn test_step_profile_runtime_context_constraints() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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

        let profile = StepProfileDeriver::derive_profile("read-file", &file_expr, &context).unwrap();

        // Should be downgraded to Inherit since Isolated is not allowed
        assert_eq!(profile.isolation_level, IsolationLevel::Inherit);
    }

    #[tokio::test]
    async fn test_step_profile_causal_chain_logging() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        _orchestrator.derive_step_profile("test-step", &network_expr, &context).unwrap();

        // Check that a StepProfileDerived action was logged
        let profile_action_exists = {
            let guard = chain.lock().unwrap();
            let actions = guard.get_actions_for_intent(&"step-security".to_string());
            actions.iter().any(|a| a.action_type == ActionType::StepProfileDerived)
        };
        assert!(profile_action_exists, "StepProfileDerived action should be logged");
    }

    #[tokio::test]
    async fn test_orchestrator_step_profile_management() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        _orchestrator.derive_step_profile("test-math", &expr, &context).unwrap();
        assert!(_orchestrator.get_current_step_profile().is_some());

        // Clear profile
        _orchestrator.clear_step_profile();
        assert!(_orchestrator.get_current_step_profile().is_none());
    }

    #[tokio::test]
    async fn test_security_flag_combinations() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("dangerous-op", &dangerous_expr, &context).unwrap();

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
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

        // Test that network operations get appropriate bandwidth limits
        let network_expr = Expression::List(vec![
            Expression::Symbol(Symbol("call".to_string())),
            Expression::Symbol(Symbol("http.download".to_string())),
            Expression::List(vec![
                Expression::Symbol(Symbol("values".to_string())),
                Expression::Literal(Literal::String("https://example.com/large-file.zip".to_string())),
            ]),
        ]);

        let context = test_context();
        let profile = StepProfileDeriver::derive_profile("download-file", &network_expr, &context).unwrap();

        // Network operations should have bandwidth limits
        assert!(profile.resource_limits.max_network_bandwidth.is_some());
        assert!(profile.resource_limits.max_network_bandwidth.unwrap() >= 10 * 1024 * 1024); // At least 10MB/s
    }

    #[tokio::test]
    async fn test_data_operations_deterministic() {
        let chain = Arc::new(Mutex::new(CausalChain::new().expect("chain")));
        let graph = make_graph_with_sink(Arc::clone(&chain));
        let marketplace = Arc::new(CapabilityMarketplace::new(Default::default()));
        let mut _orchestrator = Orchestrator::new(Arc::clone(&chain), Arc::clone(&graph), Arc::clone(&marketplace));

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
        let profile = StepProfileDeriver::derive_profile("parse-json", &data_expr, &context).unwrap();

        assert_eq!(profile.deterministic, true);
        assert_eq!(profile.isolation_level, IsolationLevel::Inherit);
    }
}
