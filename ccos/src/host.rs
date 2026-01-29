//! Runtime Host
//!
//! The concrete implementation of the `HostInterface` that connects the RTFS runtime
//! to the CCOS stateful components like the Causal Chain and Capability Marketplace.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::budget::{BudgetCheckResult, BudgetContext, ExhaustionPolicy, StepConsumption};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::causal_chain::CausalChain;
use crate::approval::storage_file::FileApprovalStorage;
use crate::approval::types::ApprovalCategory;
use crate::approval::{RiskAssessment, RiskLevel, UnifiedApprovalQueue};
use crate::governance_kernel::GovernanceKernel;
use crate::sandbox::ResourceMetrics;
use crate::orchestrator::Orchestrator;
use crate::types::{Action, ActionType, ExecutionResult};
use crate::utils::fs::get_workspace_root;
use crate::utils::value_conversion::{map_key_to_string, rtfs_value_to_json};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::execution_outcome::{CallMetadata, CausalContext, HostCall};
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::security::{default_effects_for_capability, RuntimeContext};
use rtfs::runtime::values::Value;
// futures::executor used via fully qualified path below
use rtfs::ast::{MapKey, TypeExpr};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
struct HostPlanContext {
    plan_id: String,
    intent_ids: Vec<String>,
    parent_action_id: String,
    step_context: HashMap<String, Value>,
}

#[derive(Debug, Default)]
struct UsageMetrics {
    llm_input_tokens: Option<u64>,
    llm_output_tokens: Option<u64>,
    cost_usd: Option<f64>,
    network_egress_bytes: Option<u64>,
    storage_write_bytes: Option<u64>,
    cpu_time_ms: Option<u64>,
    memory_peak_mb: Option<u64>,
    wall_clock_ms: Option<u64>,
}

/// The RuntimeHost is the bridge between the pure RTFS runtime and the stateful CCOS world.
pub struct RuntimeHost {
    causal_chain: Arc<Mutex<CausalChain>>,
    capability_marketplace: Arc<CapabilityMarketplace>,
    security_context: RuntimeContext,
    // Execution context and step override are now guarded by Mutex for thread safety
    execution_context: Mutex<Option<HostPlanContext>>,
    // Step-level exposure override stack for nested steps
    step_exposure_override: Mutex<Vec<(bool, Option<Vec<String>>)>>,
    // Step-scoped execution hints for capability calls (generic key-value pairs)
    execution_hints: Mutex<HashMap<String, Value>>,
    // Optional governance kernel for unified governance path (external calls)
    governance_kernel: Option<Arc<GovernanceKernel>>,
    // Optional orchestrator for internal orchestration (bypasses governance)
    orchestrator: Option<Arc<Orchestrator>>,
    // Budget context for resource governance
    budget_context: Mutex<Option<Arc<Mutex<BudgetContext>>>>,
}

impl RuntimeHost {
    pub fn new(
        causal_chain: Arc<Mutex<CausalChain>>,
        capability_marketplace: Arc<CapabilityMarketplace>,
        security_context: RuntimeContext,
    ) -> Self {
        Self {
            causal_chain,
            capability_marketplace,
            security_context,
            execution_context: Mutex::new(None),
            step_exposure_override: Mutex::new(Vec::new()),
            execution_hints: Mutex::new(HashMap::new()),
            governance_kernel: None,
            orchestrator: None,
            budget_context: Mutex::new(None),
        }
    }

    /// Sets the governance kernel for unified governance path.
    /// When set, capability calls are routed through the governance kernel
    /// for governance enforcement (hint validation, security checks, etc.).
    pub fn with_governance_kernel(mut self, governance_kernel: Arc<GovernanceKernel>) -> Self {
        self.governance_kernel = Some(governance_kernel);
        self
    }

    /// Sets the orchestrator for internal orchestration path.
    /// This bypasses governance enforcement and is used for internal recursive calls.
    pub fn with_orchestrator(mut self, orchestrator: Arc<Orchestrator>) -> Self {
        self.orchestrator = Some(orchestrator);
        self
    }

    /// Get a snapshot of capability metrics for a given capability id, if available.
    /// Returns a cloned `CapabilityMetrics` to avoid holding locks or lifetimes.
    pub fn get_capability_metrics(
        &self,
        capability_id: &str,
    ) -> Option<crate::causal_chain::metrics::CapabilityMetrics> {
        let guard = self.get_causal_chain().ok()?;
        guard
            .get_capability_metrics(&capability_id.to_string())
            .cloned()
    }

    /// Get function metrics by function name (e.g., step or capability id)
    pub fn get_function_metrics(
        &self,
        function_name: &str,
    ) -> Option<crate::causal_chain::metrics::FunctionMetrics> {
        let guard = self.get_causal_chain().ok()?;
        guard.get_function_metrics(function_name).cloned()
    }

    /// Get recent structured logs from the Causal Chain (test-friendly)
    pub fn get_recent_logs(&self, max: usize) -> Vec<String> {
        if let Ok(guard) = self.get_causal_chain() {
            guard.recent_logs(max)
        } else {
            Vec::new()
        }
    }

    /// Sets the budget context for the run
    pub fn with_budget(self, budget: Arc<Mutex<BudgetContext>>) -> Self {
        if let Ok(mut guard) = self.budget_context.lock() {
            *guard = Some(budget);
        }
        self
    }

    fn consume_budget_extensions(&self) -> Vec<(String, f64)> {
        let mut extensions = Vec::new();
        let mut hints_guard = match self.execution_hints.lock() {
            Ok(guard) => guard,
            Err(_) => return extensions,
        };

        let extension_value = hints_guard.remove("budget_extend");
        let Some(Value::Map(map)) = extension_value else {
            return extensions;
        };

        for (key, value) in map {
            let key_str = map_key_to_string(&key);
            let additional = match value {
                Value::Integer(i) => i as f64,
                Value::Float(f) => f,
                _ => continue,
            };
            extensions.push((key_str, additional));
        }

        extensions
    }

    fn check_budget_pre_call(&self) -> RuntimeResult<()> {
        let budget_ctx_arc = {
            let guard = self.budget_context.lock().map_err(|_| {
                RuntimeError::Generic("RuntimeHost budget_context lock poisoned".to_string())
            })?;
            guard.clone()
        };

        if let Some(ctx_mutex) = budget_ctx_arc {
            let mut ctx = ctx_mutex
                .lock()
                .map_err(|_| RuntimeError::Generic("BudgetContext lock poisoned".to_string()))?;

            for (dimension, additional) in self.consume_budget_extensions() {
                let mut extended = false;
                match dimension.as_str() {
                    "steps" => {
                        ctx.extend_steps(additional as u32);
                        extended = true;
                    }
                    "llm_tokens" => {
                        ctx.extend_llm_tokens(additional as u64);
                        extended = true;
                    }
                    "cost_usd" => {
                        ctx.extend_cost(additional);
                        extended = true;
                    }
                    "wall_clock_ms" => {
                        ctx.extend_wall_clock_ms(additional as u64);
                        extended = true;
                    }
                    "network_egress_bytes" => {
                        ctx.extend_network_egress_bytes(additional as u64);
                        extended = true;
                    }
                    "storage_write_bytes" => {
                        ctx.extend_storage_write_bytes(additional as u64);
                        extended = true;
                    }
                    "sandbox_cpu_ms" => {
                        ctx.extend_sandbox_cpu_ms(additional as u64);
                        extended = true;
                    }
                    "sandbox_memory_peak_mb" => {
                        ctx.extend_sandbox_memory_peak_mb(additional as u64);
                        extended = true;
                    }
                    _ => {}
                }

                if extended {
                    if let Ok(mut chain) = self.get_causal_chain() {
                        let plan_ctx = self.get_context().ok();
                        let action = Action::new(
                            ActionType::BudgetExtended,
                            plan_ctx
                                .as_ref()
                                .map(|c| c.plan_id.clone())
                                .unwrap_or_default(),
                            plan_ctx
                                .as_ref()
                                .and_then(|c| c.intent_ids.first())
                                .cloned()
                                .unwrap_or_default(),
                        )
                        .with_metadata("dimension", &dimension)
                        .with_metadata("additional", &additional.to_string());

                        chain.append(&action).ok();
                    }
                }
            }

            match ctx.check() {
                BudgetCheckResult::Ok => {}
                BudgetCheckResult::Warning { dimension, percent } => {
                    let mut chain = self.get_causal_chain()?;
                    let (consumed, limit) = ctx
                        .consumed_and_limit_for(&dimension)
                        .unwrap_or((0, 0));

                    // Log as a generic system action in causal chain for now
                    let plan_ctx = self.get_context().ok();
                    let action = Action::new(
                        ActionType::BudgetWarningIssued,
                        plan_ctx
                            .as_ref()
                            .map(|c| c.plan_id.clone())
                            .unwrap_or_default(),
                        plan_ctx
                            .as_ref()
                            .and_then(|c| c.intent_ids.first())
                            .cloned()
                            .unwrap_or_default(),
                    )
                    .with_metadata("dimension", &dimension)
                    .with_metadata("percent", &percent.to_string())
                    .with_metadata("consumed", &consumed.to_string())
                    .with_metadata("limit", &limit.to_string());

                    chain.append(&action).ok();
                }
                BudgetCheckResult::Exhausted { dimension, policy } => {
                    let mut chain = self.get_causal_chain()?;
                    let (consumed, limit) = ctx
                        .consumed_and_limit_for(&dimension)
                        .unwrap_or((0, 0));
                    let plan_ctx = self.get_context().ok();
                    let action = Action::new(
                        ActionType::BudgetExhausted,
                        plan_ctx
                            .as_ref()
                            .map(|c| c.plan_id.clone())
                            .unwrap_or_default(),
                        plan_ctx
                            .as_ref()
                            .and_then(|c| c.intent_ids.first())
                            .cloned()
                            .unwrap_or_default(),
                    )
                    .with_metadata("dimension", &dimension)
                    .with_metadata("policy", &format!("{:?}", policy))
                    .with_metadata("consumed", &consumed.to_string())
                    .with_metadata("limit", &limit.to_string());

                    chain.append(&action).ok();

                    match policy {
                        ExhaustionPolicy::HardStop => {
                            return Err(RuntimeError::BudgetExhausted {
                                dimension: dimension.clone(),
                                policy: "HardStop".to_string(),
                            });
                        }
                        ExhaustionPolicy::ApprovalRequired => {
                            if let Some(ctx) = plan_ctx.as_ref() {
                                self.queue_budget_extension_approval(
                                    ctx,
                                    &dimension,
                                    consumed,
                                    limit,
                                );
                            }
                            return Err(RuntimeError::BudgetExhausted {
                                dimension: dimension.clone(),
                                policy: "ApprovalRequired".to_string(),
                            });
                        }
                        ExhaustionPolicy::SoftWarn => {
                            // Log and continue
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn record_budget_consumption(
        &self,
        capability_id: &str,
        duration_ms: u64,
        args: &[Value],
        result: &RuntimeResult<Value>,
    ) {
        let budget_ctx_arc = {
            if let Ok(guard) = self.budget_context.lock() {
                guard.clone()
            } else {
                None
            }
        };

        if let Some(ctx_mutex) = budget_ctx_arc {
            if let Ok(mut ctx) = ctx_mutex.lock() {
                let usage = result
                    .as_ref()
                    .ok()
                    .and_then(Self::extract_usage_from_value);

                let mut consumption = StepConsumption {
                    capability_id: Some(capability_id.to_string()),
                    duration_ms,
                    ..Default::default()
                };

                let mut tokens_estimated = false;
                if let Some(usage) = &usage {
                    if let Some(input) = usage.llm_input_tokens {
                        consumption.llm_input_tokens = input;
                    }
                    if let Some(output) = usage.llm_output_tokens {
                        consumption.llm_output_tokens = output;
                    }
                    if let Some(cost) = usage.cost_usd {
                        consumption.cost_usd = cost;
                    }
                    if let Some(network) = usage.network_egress_bytes {
                        consumption.network_egress_bytes = network;
                    }
                    if let Some(storage) = usage.storage_write_bytes {
                        consumption.storage_write_bytes = storage;
                    }
                    if usage.cpu_time_ms.is_some()
                        || usage.memory_peak_mb.is_some()
                        || usage.wall_clock_ms.is_some()
                    {
                        let metrics = ResourceMetrics {
                            cpu_time_ms: usage.cpu_time_ms.unwrap_or(0),
                            memory_peak_mb: usage.memory_peak_mb.unwrap_or(0),
                            wall_clock_ms: usage.wall_clock_ms.unwrap_or(0),
                            network_egress_bytes: 0,
                            storage_write_bytes: 0,
                        };
                        ctx.record_sandbox_consumption(&metrics);
                    }
                }

                if consumption.llm_input_tokens == 0 && consumption.llm_output_tokens == 0 {
                    let (estimated_input, estimated_output) =
                        Self::estimate_tokens_from_values(args, result);
                    if estimated_input > 0 || estimated_output > 0 {
                        consumption.llm_input_tokens = estimated_input;
                        consumption.llm_output_tokens = estimated_output;
                        tokens_estimated = true;
                    }
                }

                ctx.record_step(consumption.clone());
                let remaining = ctx.remaining();

                // Log to causal chain
                if let Ok(mut chain) = self.get_causal_chain() {
                    let plan_ctx = self.get_context().ok();
                    let mut action = Action::new(
                        ActionType::BudgetConsumptionRecorded,
                        plan_ctx
                            .as_ref()
                            .map(|c| c.plan_id.clone())
                            .unwrap_or_default(),
                        plan_ctx
                            .as_ref()
                            .and_then(|c| c.intent_ids.first())
                            .cloned()
                            .unwrap_or_default(),
                    )
                    .with_name(capability_id)
                    .with_metadata("duration_ms", &duration_ms.to_string())
                    .with_metadata("remaining_steps", &remaining.steps.to_string())
                    .with_metadata("remaining_llm_tokens", &remaining.llm_tokens.to_string())
                    .with_metadata("remaining_cost_usd", &remaining.cost_usd.to_string())
                    .with_metadata(
                        "remaining_network_egress_bytes",
                        &remaining.network_egress_bytes.to_string(),
                    )
                    .with_metadata(
                        "remaining_storage_write_bytes",
                        &remaining.storage_write_bytes.to_string(),
                    );
                    action = action
                        .with_metadata(
                            "remaining_sandbox_cpu_ms",
                            &remaining.sandbox_cpu_ms.to_string(),
                        )
                        .with_metadata(
                            "remaining_sandbox_memory_peak_mb",
                            &remaining.sandbox_memory_peak_mb.to_string(),
                        );

                    if consumption.llm_input_tokens > 0 {
                        action = action.with_metadata(
                            "llm_input_tokens",
                            &consumption.llm_input_tokens.to_string(),
                        );
                    }
                    if consumption.llm_output_tokens > 0 {
                        action = action.with_metadata(
                            "llm_output_tokens",
                            &consumption.llm_output_tokens.to_string(),
                        );
                    }
                    if tokens_estimated {
                        action = action.with_metadata("llm_tokens_estimated", "true");
                    }
                    if consumption.cost_usd > 0.0 {
                        action =
                            action.with_metadata("cost_usd", &consumption.cost_usd.to_string());
                    }
                    if consumption.network_egress_bytes > 0 {
                        action = action.with_metadata(
                            "network_egress_bytes",
                            &consumption.network_egress_bytes.to_string(),
                        );
                    }
                    if consumption.storage_write_bytes > 0 {
                        action = action.with_metadata(
                            "storage_write_bytes",
                            &consumption.storage_write_bytes.to_string(),
                        );
                    }
                    if let Some(usage) = &usage {
                        if let Some(cpu_time_ms) = usage.cpu_time_ms {
                            action = action.with_metadata(
                                "sandbox_cpu_time_ms",
                                &cpu_time_ms.to_string(),
                            );
                        }
                        if let Some(memory_peak_mb) = usage.memory_peak_mb {
                            action = action.with_metadata(
                                "sandbox_memory_peak_mb",
                                &memory_peak_mb.to_string(),
                            );
                        }
                        if let Some(wall_clock_ms) = usage.wall_clock_ms {
                            action = action.with_metadata(
                                "sandbox_wall_clock_ms",
                                &wall_clock_ms.to_string(),
                            );
                        }
                    }

                    chain.append(&action).ok();
                }
            }
        }
    }

    fn extract_usage_from_value(value: &Value) -> Option<UsageMetrics> {
        let json = rtfs_value_to_json(value).ok()?;
        let mut usage = UsageMetrics::default();

        let read_u64 = |obj: &serde_json::Value, keys: &[&str]| -> Option<u64> {
            for key in keys {
                if let Some(val) = obj.get(*key) {
                    if let Some(num) = val.as_u64() {
                        return Some(num);
                    }
                    if let Some(num) = val.as_f64() {
                        return Some(num as u64);
                    }
                }
            }
            None
        };

        let read_f64 = |obj: &serde_json::Value, keys: &[&str]| -> Option<f64> {
            for key in keys {
                if let Some(val) = obj.get(*key) {
                    if let Some(num) = val.as_f64() {
                        return Some(num);
                    }
                    if let Some(num) = val.as_u64() {
                        return Some(num as f64);
                    }
                }
            }
            None
        };

        let usage_obj = json.get("usage").unwrap_or(&json);

        usage.llm_input_tokens = read_u64(
            usage_obj,
            &[
                "llm_input_tokens",
                "input_tokens",
                "prompt_tokens",
                "usage_input_tokens",
                "usage.llm_input_tokens",
            ],
        )
        .or_else(|| read_u64(&json, &["usage.llm_input_tokens", "usage_input_tokens"]));

        usage.llm_output_tokens = read_u64(
            usage_obj,
            &[
                "llm_output_tokens",
                "output_tokens",
                "completion_tokens",
                "usage_output_tokens",
                "usage.llm_output_tokens",
            ],
        )
        .or_else(|| read_u64(&json, &["usage.llm_output_tokens", "usage_output_tokens"]));

        if usage.llm_input_tokens.is_none() && usage.llm_output_tokens.is_none() {
            if let Some(total) = read_u64(usage_obj, &["total_tokens", "usage.total_tokens"]) {
                usage.llm_input_tokens = Some(total);
                usage.llm_output_tokens = Some(0);
            }
        }

        usage.cost_usd = read_f64(
            usage_obj,
            &[
                "cost_usd",
                "total_cost_usd",
                "usage_cost_usd",
                "usage.total_cost_usd",
            ],
        )
        .or_else(|| read_f64(&json, &["cost_usd", "total_cost_usd"]));

        usage.network_egress_bytes = read_u64(
            usage_obj,
            &[
                "network_egress_bytes",
                "egress_bytes",
                "usage.network_egress_bytes",
            ],
        )
        .or_else(|| read_u64(&json, &["network_egress_bytes"]));

        usage.storage_write_bytes = read_u64(
            usage_obj,
            &[
                "storage_write_bytes",
                "storage_bytes",
                "usage.storage_write_bytes",
            ],
        )
        .or_else(|| read_u64(&json, &["storage_write_bytes"]));

        usage.cpu_time_ms = read_u64(
            usage_obj,
            &["cpu_time_ms", "sandbox_cpu_ms", "usage.cpu_time_ms"],
        )
        .or_else(|| read_u64(&json, &["cpu_time_ms", "sandbox_cpu_ms"]));

        usage.memory_peak_mb = read_u64(
            usage_obj,
            &["memory_peak_mb", "sandbox_memory_peak_mb", "usage.memory_peak_mb"],
        )
        .or_else(|| read_u64(&json, &["memory_peak_mb", "sandbox_memory_peak_mb"]));

        usage.wall_clock_ms = read_u64(
            usage_obj,
            &["wall_clock_ms", "sandbox_wall_clock_ms", "usage.wall_clock_ms"],
        )
        .or_else(|| read_u64(&json, &["wall_clock_ms", "sandbox_wall_clock_ms"]));

        if usage.llm_input_tokens.is_some()
            || usage.llm_output_tokens.is_some()
            || usage.cost_usd.is_some()
            || usage.network_egress_bytes.is_some()
            || usage.storage_write_bytes.is_some()
            || usage.cpu_time_ms.is_some()
            || usage.memory_peak_mb.is_some()
            || usage.wall_clock_ms.is_some()
        {
            Some(usage)
        } else {
            None
        }
    }

    fn estimate_tokens_from_values(
        args: &[Value],
        result: &RuntimeResult<Value>,
    ) -> (u64, u64) {
        let input_tokens = Self::estimate_tokens_for_value(&Value::List(args.to_vec()));
        let output_tokens = match result {
            Ok(value) => Self::estimate_tokens_for_value(value),
            Err(_) => 0,
        };

        (input_tokens, output_tokens)
    }

    fn estimate_tokens_for_value(value: &Value) -> u64 {
        let serialized = rtfs_value_to_json(value)
            .ok()
            .and_then(|json| serde_json::to_string(&json).ok())
            .unwrap_or_else(|| format!("{:?}", value));

        let char_count = serialized.chars().count() as f64;
        (char_count / 4.0).ceil() as u64
    }

    fn queue_budget_extension_approval(
        &self,
        ctx: &HostPlanContext,
        dimension: &str,
        consumed: u64,
        limit: u64,
    ) {
        let storage_path = get_workspace_root().join("storage/approvals");
        let storage = match FileApprovalStorage::new(storage_path) {
            Ok(storage) => storage,
            Err(_) => return,
        };
        let queue = UnifiedApprovalQueue::new(std::sync::Arc::new(storage));

        let suggested = self.suggest_budget_extension(dimension, limit);
        let plan_id_owned = ctx.plan_id.clone();
        let intent_id = ctx.intent_ids.first().cloned().unwrap_or_default();
        let dimension_owned = dimension.to_string();
        let reason = format!("Budget exhausted: {} consumed {}/{}", dimension, consumed, limit);

        let fut = async move {
            if let Ok(pending) = queue.list_pending_budget_extensions().await {
                let already_pending = pending.iter().any(|request| {
                    matches!(
                        request.category,
                        ApprovalCategory::BudgetExtension {
                            ref plan_id,
                            ref dimension,
                            ..
                        } if plan_id == &plan_id_owned && dimension == &dimension_owned
                    )
                });
                if already_pending {
                    return;
                }
            }

            let risk = RiskAssessment {
                level: RiskLevel::Medium,
                reasons: vec![reason],
            };

            let _ = queue
                .add_budget_extension(
                    plan_id_owned,
                    intent_id,
                    dimension_owned,
                    suggested,
                    consumed,
                    limit,
                    risk,
                    24,
                    None,
                )
                .await;
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(fut);
        } else {
            futures::executor::block_on(fut);
        }
    }

    fn suggest_budget_extension(&self, dimension: &str, limit: u64) -> f64 {
        match dimension {
            "cost_usd" => {
                let limit_usd = limit as f64 / 1000.0;
                (limit_usd * 0.5).max(0.01)
            }
            _ => ((limit as f64) * 0.5).max(1.0),
        }
    }

    /// Test helper: record a delegation event into the chain
    pub fn record_delegation_event_for_test(
        &self,
        intent_id: &str,
        event_kind: &str,
        metadata: std::collections::HashMap<String, Value>,
    ) -> RuntimeResult<()> {
        let mut chain = self.get_causal_chain()?;
        chain
            .record_delegation_event(None, &intent_id.to_string(), event_kind, metadata)
            .map_err(|e| RuntimeError::Generic(format!("record_delegation_event error: {:?}", e)))
    }

    fn build_context_snapshot(
        &self,
        step_name: &str,
        args: &[Value],
        capability_id: &str,
    ) -> Option<Value> {
        // Policy gate: allow exposing read-only context for this capability?
        // Evaluate dynamic policy using manifest metadata (tags) when available.
        // Step-level override may force exposure or suppression
        if let Ok(ov) = self.step_exposure_override.lock() {
            if let Some((expose, _)) = ov.last() {
                if !*expose {
                    return None;
                }
            }
        }

        // Avoid async/await here to prevent nested runtimes; rely on static policy only
        let allow_exposure = if !self.security_context.expose_readonly_context {
            false
        } else {
            // Fallback to exact/prefix policy without tags or manifest lookup
            self.security_context
                .is_context_exposure_allowed_for(capability_id, None)
        };
        if !allow_exposure {
            return None;
        }
        let plan_ctx_owned = {
            let guard = self.execution_context.lock().ok()?;
            guard.clone()?
        };

        // Compute a small content hash over inputs for provenance/caching
        let mut hasher = Sha256::new();
        let arg_fingerprint = format!("{:?}", args);
        hasher.update(arg_fingerprint.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        let mut map = std::collections::HashMap::new();
        map.insert(
            MapKey::String("plan_id".to_string()),
            Value::String(plan_ctx_owned.plan_id.clone()),
        );
        map.insert(
            MapKey::String("primary_intent".to_string()),
            Value::String(
                plan_ctx_owned
                    .intent_ids
                    .first()
                    .cloned()
                    .unwrap_or_default(),
            ),
        );
        map.insert(
            MapKey::String("intent_ids".to_string()),
            Value::Vector(
                plan_ctx_owned
                    .intent_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        map.insert(
            MapKey::String("step".to_string()),
            Value::String(step_name.to_string()),
        );
        map.insert(
            MapKey::String("inputs_hash".to_string()),
            Value::String(hash),
        );
        if !plan_ctx_owned.step_context.is_empty() {
            let step_map: std::collections::HashMap<MapKey, Value> = plan_ctx_owned
                .step_context
                .iter()
                .map(|(k, v)| (MapKey::String(k.clone()), v.clone()))
                .collect();
            map.insert(
                MapKey::String("step_context".to_string()),
                Value::Map(step_map.clone()),
            );

            // Also flatten step context into top-level entries for prompt builders
            for (k, v) in step_map {
                if let Value::String(s) = v {
                    map.insert(MapKey::String(k.to_string()), Value::String(s));
                }
            }
        }

        // Apply step override key filtering if present
        if let Ok(ov) = self.step_exposure_override.lock() {
            if let Some((_, Some(keys))) = ov.last() {
                let allowed: std::collections::HashSet<&String> = keys.iter().collect();
                map.retain(|k, _| match k {
                    MapKey::String(s) => allowed.contains(s),
                    _ => true,
                });
            }
        }
        Some(Value::Map(map))
    }

    /// Sets the context for a new plan execution.
    pub fn set_execution_context(
        &self,
        plan_id: String,
        intent_ids: Vec<String>,
        parent_action_id: String,
    ) {
        if let Ok(mut guard) = self.execution_context.lock() {
            *guard = Some(HostPlanContext {
                plan_id,
                intent_ids,
                parent_action_id,
                step_context: HashMap::new(),
            });
        }
    }

    /// Clears the execution context after a plan has finished.
    pub fn clear_execution_context(&self) {
        if let Ok(mut guard) = self.execution_context.lock() {
            *guard = None;
        }
    }

    fn get_causal_chain(&self) -> RuntimeResult<MutexGuard<'_, CausalChain>> {
        self.causal_chain
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))
    }

    /// Returns a vector snapshot of all actions currently in the Causal Chain.
    /// Keeps the lock only for the duration of the copy; intended for read-only use.
    pub fn snapshot_actions(&self) -> RuntimeResult<Vec<Action>> {
        let chain = self.get_causal_chain()?;
        Ok(chain.get_all_actions().to_vec())
    }

    fn get_context(&self) -> RuntimeResult<HostPlanContext> {
        let guard = self
            .execution_context
            .lock()
            .map_err(|_| RuntimeError::Generic("FATAL: Host lock poisoned".to_string()))?;
        if let Some(ctx) = guard.clone() {
            return Ok(ctx);
        }

        // If tests opt-in via environment, provide a safe fallback context so feature
        // tests that don't set context explicitly can still exercise capabilities.
        // This avoids widespread, risky changes to test harnesses while keeping
        // production behavior unchanged.
        if std::env::var("CCOS_TEST_FALLBACK_CONTEXT")
            .map(|v| {
                let lv = v.to_lowercase();
                lv == "1" || lv == "true"
            })
            .unwrap_or(false)
        {
            return Ok(HostPlanContext {
                plan_id: "auto-generated-plan".to_string(),
                intent_ids: vec!["auto-intent".to_string()],
                parent_action_id: "0".to_string(),
                step_context: HashMap::new(),
            });
        }

        Err(RuntimeError::Generic(
            "FATAL: Host method called without a valid execution context".to_string(),
        ))
    }

    fn build_call_metadata(&self, snapshot: &Value) -> CallMetadata {
        let mut metadata = CallMetadata::new();
        let mut ctx_map = std::collections::HashMap::new();

        if let Value::Map(m) = snapshot {
            for (k, v) in m {
                let key = map_key_to_string(k);
                let val = match v {
                    Value::String(s) => s.clone(),
                    _ => {
                        if let Ok(json) = rtfs_value_to_json(v) {
                            serde_json::to_string(&json).unwrap_or_else(|_| format!("{:?}", v))
                        } else {
                            format!("{:?}", v)
                        }
                    }
                };
                let capped: String = val.chars().take(800).collect();
                let capped = if capped.len() < val.len() {
                    format!("{}... [truncated]", capped)
                } else {
                    capped
                };
                ctx_map.insert(key, capped);
            }
        }

        metadata.context = ctx_map;

        // Include any step-scoped execution hints
        if let Ok(guard) = self.execution_hints.lock() {
            metadata.execution_hints = guard.clone();
        }

        metadata
    }
}

impl HostInterface for RuntimeHost {
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // --- Resource Budget Enforcement ---
        self.check_budget_pre_call()?;
        let step_start_time = std::time::Instant::now();

        // 1. Security Validation
        if !self.security_context.is_capability_allowed(name) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: name.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }

        // Apply RTFS-level argument normalization if schema is available
        let normalized_args = if let Some(schema) = self.get_capability_input_schema(name) {
            let normalized = crate::capabilities::arg_normalization::normalize_args_to_map(
                args.to_vec(),
                &schema,
            )?;
            vec![normalized]
        } else {
            args.to_vec()
        };
        let args = &normalized_args;

        if self.security_context.allowed_effects.is_some()
            || !self.security_context.denied_effects.is_empty()
        {
            let manifest_effects = futures::executor::block_on(async {
                self.capability_marketplace
                    .get_capability(name)
                    .await
                    .map(|manifest| manifest.effects)
            });

            let effects: Vec<String> = match manifest_effects {
                Some(list) if !list.is_empty() => list,
                _ => default_effects_for_capability(name)
                    .iter()
                    .map(|effect| (*effect).to_string())
                    .collect(),
            };

            self.security_context
                .ensure_effects_allowed(name, &effects)?;
        }

        let context = self.get_context()?;
        // New calling convention: provide :args plus optional :context snapshot
        let mut call_map: std::collections::HashMap<MapKey, Value> =
            std::collections::HashMap::new();
        call_map.insert(
            MapKey::Keyword(rtfs::ast::Keyword("args".to_string())),
            Value::List(args.to_vec()),
        );
        let snapshot = self.build_context_snapshot(name, args, name);
        if let Some(snapshot_value) = snapshot.clone() {
            call_map.insert(
                MapKey::Keyword(rtfs::ast::Keyword("context".to_string())),
                snapshot_value,
            );
        }
        let _capability_args = Value::Map(call_map);

        // Prepare CallMetadata - ALWAYS include execution_hints for governance
        // Even if snapshot is None, we must pass hints to the Orchestrator
        let call_metadata: Option<CallMetadata> = {
            let mut metadata = snapshot
                .as_ref()
                .map(|snap| self.build_call_metadata(snap))
                .unwrap_or_else(CallMetadata::new);

            // Always include step-scoped execution hints (retry, timeout, fallback)
            if let Ok(guard) = self.execution_hints.lock() {
                metadata.execution_hints = guard.clone();
            }
            Some(metadata)
        };

        // 2. Check execution mode and security level for dry-run simulation
        let execution_mode = self.get_execution_mode();
        let security_level = self.detect_security_level(name);
        let security_level_clone = security_level.clone();
        let should_simulate = execution_mode == "dry-run"
            && (security_level == "high" || security_level == "critical");

        // 2. Create and log the CapabilityCall action
        let mut action = Action::new(
            ActionType::CapabilityCall,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(context.parent_action_id.clone()))
        .with_name(name)
        .with_arguments(&args);

        // Add execution mode metadata to action
        if should_simulate {
            action
                .metadata
                .insert("dry_run".to_string(), Value::Boolean(true));
            action
                .metadata
                .insert("simulated".to_string(), Value::Boolean(true));
            action.metadata.insert(
                "security_level".to_string(),
                Value::String(security_level_clone.clone()),
            );
        }

        let _action_id = self.get_causal_chain()?.append(&action)?;

        // 3. If dry-run and critical, simulate the result instead of executing
        if should_simulate {
            let simulated_result = self.generate_simulated_result(name, args)?;

            // Log the simulated result
            let execution_result = ExecutionResult {
                success: true,
                value: simulated_result.clone(),
                metadata: {
                    let mut meta = std::collections::HashMap::new();
                    meta.insert("dry_run".to_string(), Value::Boolean(true));
                    meta.insert("simulated".to_string(), Value::Boolean(true));
                    meta.insert(
                        "security_level".to_string(),
                        Value::String(security_level_clone),
                    );
                    meta
                },
            };

            self.get_causal_chain()?
                .record_result(action, execution_result)?;

            return Ok(simulated_result);
        }

        // 4. Execute the capability - route through Orchestrator for unified governance, or fallback to Marketplace
        let name_owned = name.to_string();
        let args_owned: Vec<Value> = args.to_vec();
        let runtime_handle = tokio::runtime::Handle::try_current().ok();
        let call_metadata_owned = call_metadata.clone();

        let result = if let Some(governance_kernel) = &self.governance_kernel {
            // Route through GovernanceKernel for governance enforcement (external calls)
            let governance_kernel = governance_kernel.clone();
            let security_context = self.security_context.clone();

            std::thread::spawn(move || {
                let fut = async move {
                    let host_call = HostCall {
                        capability_id: name_owned,
                        args: args_owned,
                        security_context,
                        causal_context: Some(CausalContext::default()),
                        metadata: call_metadata_owned,
                    };
                    governance_kernel
                        .handle_host_call_governed(&host_call)
                        .await
                };

                if let Some(handle) = runtime_handle {
                    handle.block_on(fut)
                } else {
                    futures::executor::block_on(fut)
                }
            })
            .join()
            .map_err(|_| {
                RuntimeError::Generic("Thread join error during capability execution".to_string())
            })?
        } else if let Some(orchestrator) = &self.orchestrator {
            // Route through Orchestrator internally (bypasses governance for recursive calls)
            let orchestrator = orchestrator.clone();
            let security_context = self.security_context.clone();

            std::thread::spawn(move || {
                let fut = async move {
                    let host_call = HostCall {
                        capability_id: name_owned,
                        args: args_owned,
                        security_context,
                        causal_context: Some(CausalContext::default()),
                        metadata: call_metadata_owned,
                    };
                    orchestrator.handle_host_call_internal(&host_call).await
                };

                if let Some(handle) = runtime_handle {
                    handle.block_on(fut)
                } else {
                    futures::executor::block_on(fut)
                }
            })
            .join()
            .map_err(|_| {
                RuntimeError::Generic("Thread join error during capability execution".to_string())
            })?
        } else {
            // Fallback: direct marketplace call (for tests without orchestrator/kernel)
            let marketplace = self.capability_marketplace.clone();

            std::thread::spawn(move || {
                let fut = async move {
                    let args_value = Value::List(args_owned);
                    let meta_ref = call_metadata_owned.as_ref();
                    marketplace
                        .execute_capability_enhanced(&name_owned, &args_value, meta_ref)
                        .await
                };

                if let Some(handle) = runtime_handle {
                    handle.block_on(fut)
                } else {
                    futures::executor::block_on(fut)
                }
            })
            .join()
            .map_err(|_| {
                RuntimeError::Generic("Thread join error during capability execution".to_string())
            })?
        };

        // 5. Log the result to the Causal Chain
        let execution_result = match &result {
            Ok(value) => ExecutionResult {
                success: true,
                value: value.clone(),
                metadata: Default::default(),
            },
            Err(e) => {
                let error_msg = e.to_string();
                let error_category = classify_error(&error_msg);
                ExecutionResult {
                    success: false,
                    value: Value::Nil,
                    metadata: std::collections::HashMap::from([
                        ("error".to_string(), Value::String(error_msg)),
                        ("error_category".to_string(), Value::String(error_category)),
                    ]),
                }
            }
        };

        self.get_causal_chain()?
            .record_result(action, execution_result)?;

        // --- Resource Budget Metering ---
        let duration_ms = step_start_time.elapsed().as_millis() as u64;
        self.record_budget_consumption(name, duration_ms, args, &result);

        result
    }

    fn get_capability_input_schema(&self, name: &str) -> Option<TypeExpr> {
        let marketplace = self.capability_marketplace.clone();
        let name_owned = name.to_string();

        let runtime_handle = tokio::runtime::Handle::try_current().ok();

        std::thread::spawn(move || {
            let fut = async move {
                let caps = marketplace.capabilities.read().await;
                caps.get(&name_owned).and_then(|m| m.input_schema.clone())
            };

            if let Some(handle) = runtime_handle {
                handle.block_on(fut)
            } else {
                futures::executor::block_on(fut)
            }
        })
        .join()
        .ok()
        .flatten()
    }

    fn notify_step_started(&self, step_name: &str) -> RuntimeResult<String> {
        let context = self.get_context()?;
        let action = Action::new(
            ActionType::PlanStepStarted,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(context.parent_action_id.clone()))
        .with_name(step_name);

        self.get_causal_chain()?.append(&action)
    }

    fn notify_step_completed(
        &self,
        step_action_id: &str,
        result: &rtfs::runtime::stubs::ExecutionResultStruct,
    ) -> RuntimeResult<()> {
        let context = self.get_context()?;
        // Convert ExecutionResultStruct to ExecutionResult
        let exec_result = ExecutionResult {
            success: result.success,
            value: result.value.clone(),
            metadata: result.metadata.clone(),
        };
        let action = Action::new(
            ActionType::PlanStepCompleted,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(step_action_id.to_string()))
        .with_result(exec_result);

        self.get_causal_chain()?.append(&action)?;
        Ok(())
    }

    fn notify_step_failed(&self, step_action_id: &str, error: &str) -> RuntimeResult<()> {
        let context = self.get_context()?;
        let action = Action::new(
            ActionType::PlanStepFailed,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(step_action_id.to_string()))
        .with_error(error);

        self.get_causal_chain()?.append(&action)?;
        Ok(())
    }

    fn set_execution_context(
        &self,
        plan_id: String,
        intent_ids: Vec<String>,
        parent_action_id: String,
    ) {
        // Call inherent method explicitly to avoid trait-method recursion
        RuntimeHost::set_execution_context(self, plan_id, intent_ids, parent_action_id);
    }

    fn clear_execution_context(&self) {
        // Call inherent method explicitly to avoid trait-method recursion
        RuntimeHost::clear_execution_context(self);
    }

    fn set_step_exposure_override(&self, expose: bool, context_keys: Option<Vec<String>>) {
        if let Ok(mut ov) = self.step_exposure_override.lock() {
            ov.push((expose, context_keys));
        }
    }

    fn set_step_context_value(&self, key: String, value: Value) -> RuntimeResult<()> {
        if let Ok(mut guard) = self.execution_context.lock() {
            if let Some(ctx) = guard.as_mut() {
                ctx.step_context.insert(key, value);
            }
        }
        Ok(())
    }

    fn clear_step_exposure_override(&self) {
        if let Ok(mut ov) = self.step_exposure_override.lock() {
            let _ = ov.pop();
        }
    }

    fn get_context_value(&self, key: &str) -> Option<Value> {
        let ctx = self.execution_context.lock().ok()?.clone()?;
        if let Some(val) = ctx.step_context.get(key) {
            return Some(val.clone());
        }
        match key {
            // Primary identifiers
            "plan-id" => Some(Value::String(ctx.plan_id.clone())),
            "intent-id" => Some(Value::String(
                ctx.intent_ids.first().cloned().unwrap_or_default(),
            )),
            "intent-ids" => Some(Value::Vector(
                ctx.intent_ids.iter().cloned().map(Value::String).collect(),
            )),
            // Parent action ID
            "parent-action-id" => Some(Value::String(ctx.parent_action_id.clone())),
            _ => None,
        }
    }

    fn set_execution_hint(&self, key: &str, value: Value) -> RuntimeResult<()> {
        if let Ok(mut guard) = self.execution_hints.lock() {
            guard.insert(key.to_string(), value);
        }
        Ok(())
    }

    fn clear_execution_hint(&self, key: &str) -> RuntimeResult<()> {
        if let Ok(mut guard) = self.execution_hints.lock() {
            guard.remove(key);
        }
        Ok(())
    }

    fn clear_all_execution_hints(&self) -> RuntimeResult<()> {
        if let Ok(mut guard) = self.execution_hints.lock() {
            guard.clear();
        }
        Ok(())
    }
}

impl RuntimeHost {
    /// Get execution mode from RuntimeContext cross_plan_params
    pub fn get_execution_mode(&self) -> String {
        // Check cross_plan_params for execution_mode
        // This is set by Orchestrator based on Governance Kernel detection
        if let Some(Value::String(mode)) = self
            .security_context
            .cross_plan_params
            .get("execution_mode")
        {
            return mode.clone();
        }
        // Default: full execution
        "full".to_string()
    }

    /// Detect security level for a capability based on ID patterns
    /// Returns: "low", "medium", "high", or "critical"
    /// This mirrors the logic in Governance Kernel for consistency
    pub fn detect_security_level(&self, capability_id: &str) -> String {
        let id_lower = capability_id.to_lowercase();

        // Critical operations: payments, billing, charges, transfers
        if id_lower.contains("payment")
            || id_lower.contains("billing")
            || id_lower.contains("charge")
            || id_lower.contains("transfer")
            || id_lower.contains("refund")
        {
            return "critical".to_string();
        }

        // Critical operations: deletions, removals, destructive operations
        if id_lower.contains("delete")
            || id_lower.contains("remove")
            || id_lower.contains("destroy")
            || id_lower.contains("drop")
            || id_lower.contains("truncate")
        {
            return "critical".to_string();
        }

        // High-risk operations: system-level changes
        if id_lower.contains("exec")
            || id_lower.contains("shell")
            || id_lower.contains("system")
            || id_lower.contains("admin")
            || id_lower.contains("root")
        {
            return "high".to_string();
        }

        // Moderate operations: writes, creates, updates
        if id_lower.contains("write")
            || id_lower.contains("create")
            || id_lower.contains("update")
            || id_lower.contains("modify")
            || id_lower.contains("edit")
        {
            return "medium".to_string();
        }

        // Default: read operations are safe
        "low".to_string()
    }

    /// Generate a simulated result for a capability in dry-run mode
    /// Returns a mock value based on the capability's expected output schema
    pub fn generate_simulated_result(
        &self,
        capability_id: &str,
        _args: &[Value],
    ) -> RuntimeResult<Value> {
        // For now, return a simple mock result
        // TODO: In the future, this could:
        // 1. Look up capability manifest to get output schema
        // 2. Generate a value matching that schema
        // 3. Include realistic mock data based on capability type

        let id_lower = capability_id.to_lowercase();

        // Generate appropriate mock based on capability type
        if id_lower.contains("list") || id_lower.contains("get") || id_lower.contains("fetch") {
            // Read operations: return empty list or empty map
            Ok(Value::List(vec![]))
        } else if id_lower.contains("create") || id_lower.contains("write") {
            // Write operations: return success indicator
            let mut map = std::collections::HashMap::new();
            map.insert(MapKey::String("success".to_string()), Value::Boolean(true));
            map.insert(
                MapKey::String("id".to_string()),
                Value::String("simulated-id".to_string()),
            );
            Ok(Value::Map(map))
        } else if id_lower.contains("delete") || id_lower.contains("remove") {
            // Delete operations: return success
            Ok(Value::Boolean(true))
        } else if id_lower.contains("payment") || id_lower.contains("charge") {
            // Payment operations: return transaction ID
            let mut map = std::collections::HashMap::new();
            map.insert(
                MapKey::String("transaction_id".to_string()),
                Value::String("simulated-txn-12345".to_string()),
            );
            map.insert(
                MapKey::String("status".to_string()),
                Value::String("simulated".to_string()),
            );
            Ok(Value::Map(map))
        } else {
            // Default: return nil
            Ok(Value::Nil)
        }
    }
}

impl std::fmt::Debug for RuntimeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Avoid locking issues in Debug; only show whether context is set
        let ctx_state = self
            .execution_context
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        f.debug_struct("RuntimeHost")
            .field("security_context", &self.security_context)
            .field("has_execution_context", &ctx_state)
            .finish()
    }
}

/// Classify an error message into a category for learning and analysis.
///
/// Categories:
/// - SchemaError: Input/output validation failures
/// - MissingCapability: Required capability not found
/// - TimeoutError: Execution timed out
/// - NetworkError: Network/connection issues
/// - LLMError: LLM generation or synthesis failures
/// - RuntimeError: General RTFS execution errors (default)
fn classify_error(error_msg: &str) -> String {
    let msg = error_msg.to_lowercase();

    // Schema and validation errors
    if msg.contains("schema")
        || msg.contains("validation failed")
        || msg.contains("missing field")
        || msg.contains("missing required")
        || msg.contains("type mismatch")
        || msg.contains("does not match schema")
    {
        return "SchemaError".to_string();
    }

    // Missing capability errors
    if msg.contains("unknown capability")
        || msg.contains("not found")
        || msg.contains("missing capability")
        || msg.contains("no capability")
        || msg.contains("capability not registered")
    {
        return "MissingCapability".to_string();
    }

    // Timeout errors
    if msg.contains("timeout") || msg.contains("timed out") || msg.contains("deadline exceeded") {
        return "TimeoutError".to_string();
    }

    // Network errors
    if msg.contains("network")
        || msg.contains("connection")
        || msg.contains("http")
        || msg.contains("request failed")
        || msg.contains("dns")
        || msg.contains("socket")
    {
        return "NetworkError".to_string();
    }

    // LLM-related errors
    if msg.contains("llm")
        || msg.contains("generation failed")
        || msg.contains("synthesis failed")
        || msg.contains("openai")
        || msg.contains("anthropic")
        || msg.contains("model")
    {
        return "LLMError".to_string();
    }

    // Default: general runtime error
    "RuntimeError".to_string()
}
