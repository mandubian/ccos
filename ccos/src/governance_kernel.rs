//! CCOS Governance Kernel
//!
//! This module defines the Governance Kernel, the high-privilege, secure component
//! responsible for enforcing the system's `Constitution`. It acts as the mandatory
//! intermediary between the low-privilege Arbiter and the Orchestrator.
//!
//! The Kernel's primary responsibilities include:
//! - Validating proposed plans against the Constitution.
//! - Sanitizing intents and scaffolding plans for safety.
//! - Verifying capability attestations.
//! - Logging all decisions and actions to the Causal Chain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

use crate::capability_marketplace::types::CapabilityProvenance;
use super::orchestrator::Orchestrator;

use super::intent_graph::IntentGraph;
use super::types::Intent; // for delegation validation
use super::types::{ExecutionResult, Plan, PlanBody, StorableIntent};
use rtfs::runtime::error::RuntimeError;

/// Represents the system's constitution, a set of human-authored rules.
// TODO: This should be loaded from a secure, signed configuration file.
pub struct Constitution {
    rules: Vec<String>,
}

impl Default for Constitution {
    fn default() -> Self {
        Self { rules: vec![] }
    }
}

/// The Governance Kernel is the root of trust in the CCOS.
/// Its logic is designed to be simple, verifiable, and secure.
pub struct GovernanceKernel {
    orchestrator: Arc<Orchestrator>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    constitution: Constitution,
}

impl GovernanceKernel {
    /// Creates a new Governance Kernel.
    pub fn new(orchestrator: Arc<Orchestrator>, intent_graph: Arc<Mutex<IntentGraph>>) -> Self {
        Self {
            orchestrator,
            intent_graph,
            constitution: Constitution::default(),
        }
    }

    /// The primary entry point for processing a plan from the Arbiter.
    /// It validates the plan and, if successful, passes it to the Orchestrator.
    pub async fn validate_and_execute(
        &self,
        plan: Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // --- 1. Intent Sanitization (SEP-012) ---
        // For capability-internal plans, intent may be None. Only sanitize if present.
        let intent_opt = self.get_intent(&plan)?;
        if let Some(ref intent) = intent_opt {
            self.sanitize_intent(intent, &plan)?;
        }

        // --- 2. Plan Scaffolding (SEP-012) ---
        let safe_plan = self.scaffold_plan(plan)?;

        // --- 3. Constitution Validation (SEP-010) ---
        self.validate_against_constitution(&safe_plan)?;

        // --- 4. Execution Mode Detection (Criticality-Based Execution) ---
        // Read execution mode from plan policies or intent constraints
        // This determines how critical actions should be handled
        let execution_mode = self.detect_execution_mode(&safe_plan, intent_opt.as_ref())?;
        
        // Validate execution mode is compatible with plan security requirements
        self.validate_execution_mode(&safe_plan, intent_opt.as_ref(), &execution_mode)?;

        // --- 5. Attestation Verification (SEP-011) ---
        // TODO: Verify the cryptographic attestations of all capabilities
        // called within the plan.

        // Store execution mode in context for RuntimeHost to access
        let mut context_with_mode = context.clone();
        context_with_mode
            .cross_plan_params
            .insert("execution_mode".to_string(), Value::String(execution_mode.clone()));

        // --- 6. Execution ---
        // If all checks pass, delegate execution to the Orchestrator.
        // Execution mode is passed via context cross_plan_params for RuntimeHost to use
        self.orchestrator.execute_plan(&safe_plan, &context_with_mode).await
    }

    /// Retrieves the primary intent associated with the plan, if present.
    /// Returns None for capability-internal plans that don't have associated intents.
    fn get_intent(&self, plan: &Plan) -> RuntimeResult<Option<StorableIntent>> {
        let intent_id = match plan.intent_ids.first() {
            Some(id) => id,
            None => return Ok(None), // No intent for capability-internal plans
        };

        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        let intent = graph
            .get_intent(intent_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Intent not found: {}", intent_id)))?;

        Ok(Some(intent))
    }

    /// Checks the plan and its originating intent for malicious patterns.
    fn sanitize_intent(&self, intent: &StorableIntent, plan: &Plan) -> RuntimeResult<()> {
        // Check for common prompt injection phrases in the original request.
        let lower_request = intent.original_request.to_lowercase();
        const INJECTION_PHRASES: &[&str] = &[
            "ignore all previous instructions",
            "you are now in developer mode",
        ];
        for phrase in INJECTION_PHRASES {
            if lower_request.contains(phrase) {
                return Err(RuntimeError::Generic(
                    "Potential prompt injection detected".to_string(),
                ));
            }
        }

        // Check for logical inconsistencies between the intent and the plan.
        // Example: If intent is to send an email, the plan shouldn't be deleting files.
        if intent.goal.contains("email") {
            if let PlanBody::Rtfs(body_text) = &plan.body {
                if body_text.contains("delete-file") {
                    return Err(RuntimeError::Generic(
                        "Plan action contradicts intent goal".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Wraps the plan's body in a safety harness.
    fn scaffold_plan(&self, mut plan: Plan) -> RuntimeResult<Plan> {
        // Extract the original body text
        let original_body = match &plan.body {
            PlanBody::Rtfs(text) => text.clone(),
            PlanBody::Wasm(_) => {
                return Err(RuntimeError::Generic(
                    "Cannot scaffold binary plan body".to_string(),
                ))
            }
        };

        // Wrap the original body in a `(do ...)` block if it isn't already.
        let wrapped_body = if original_body.trim().starts_with("(") {
            original_body
        } else {
            format!("(do {})", original_body)
        };

        // NOTE: Previously we injected unimplemented forms like `(with-resource-limits ...)` and `(on-failure ...)`.
        // Those forms are not yet supported by the parser/runtime, causing execution failures.
        // For now, keep the plan safely wrapped with `do` only.
        plan.body = PlanBody::Rtfs(wrapped_body);
        Ok(plan)
    }

    /// Validates the plan against the rules of the system's Constitution.
    fn validate_against_constitution(&self, plan: &Plan) -> RuntimeResult<()> {
        // TODO: Implement actual validation logic based on loaded constitutional rules.
        // For now, this is a placeholder.
        if let PlanBody::Rtfs(body_text) = &plan.body {
            if body_text.contains("launch-nukes") {
                return Err(RuntimeError::Generic(
                    "Plan violates Constitution: Rule against global thermonuclear war."
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Delegation validation hook (M4): governance pre-approval of agent selection.
    /// Extend with constitutional / policy checks (e.g., trust tier allowlist, cost ceilings, jurisdiction constraints).
    /// Return Err(...) to veto delegation (arbiter will fall back to LLM planning path).
    pub fn validate_delegation(
        &self,
        intent: &Intent,
        agent_id: &str,
        score: f64,
    ) -> RuntimeResult<()> {
        // Placeholder policy examples (expand as specs evolve):
        // 1. Reject extremely low scores (defense in depth even if arbiter threshold handles it).
        if score < 0.50 {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "Delegation rejected: score {:.2} below governance floor for agent {}",
                score, agent_id
            )));
        }
        // 2. Enforce simple constraint: if intent goal mentions "EU" ensure agent id does not contain "non_eu" (placeholder heuristic).
        let goal_lower = intent.goal.to_lowercase();
        if goal_lower.contains("eu") && agent_id.contains("non_eu") {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Delegation rejected: agent jurisdiction mismatch (EU constraint)".to_string(),
            ));
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Execution Mode Detection (Criticality-Based Execution)
    // ---------------------------------------------------------------------

    /// Detect execution mode from plan policies or intent constraints
    /// Precedence: Plan policy > Intent constraint > Default (full)
    /// 
    /// Execution modes:
    /// - "full": Execute all actions (default)
    /// - "dry-run": Validate plan without executing critical actions
    /// - "safe-only": Execute only safe actions, pause for critical ones
    /// - "require-approval": Pause and request approval for each critical action
    pub fn detect_execution_mode(
        &self,
        plan: &Plan,
        intent: Option<&StorableIntent>,
    ) -> RuntimeResult<String> {
        // Check plan policies first (highest precedence)
        // Plan policies are HashMap<String, Value>
        if let Some(execution_mode_value) = plan.policies.get("execution_mode") {
            if let Value::String(mode) = execution_mode_value {
                return Ok(mode.clone());
            }
        }

        // Check intent constraints (second precedence)
        // StorableIntent constraints are HashMap<String, String> (RTFS source expressions)
        // For execution-mode, we expect a simple string value like ":dry-run" or "dry-run"
        if let Some(intent) = intent {
            if let Some(constraint_str) = intent.constraints.get("execution-mode") {
                // Parse RTFS keyword or string value
                let mode = constraint_str
                    .trim()
                    .trim_start_matches(':')  // Remove RTFS keyword prefix if present
                    .trim_matches('"')        // Remove quotes if present
                    .to_string();
                if !mode.is_empty() {
                    return Ok(mode);
                }
            }
        }

        // Default: full execution
        Ok("full".to_string())
    }

    /// Validate that execution mode is compatible with plan and intent security requirements
    fn validate_execution_mode(
        &self,
        plan: &Plan,
        intent: Option<&StorableIntent>,
        execution_mode: &str,
    ) -> RuntimeResult<()> {
        // Check if plan has critical capabilities that require special handling
        let has_critical_capabilities = plan
            .capabilities_required
            .iter()
            .any(|cap_id| self.detect_security_level(cap_id) == "critical");

        // If plan has critical capabilities but execution mode is "full",
        // warn but allow (user explicitly requested full execution)
        if has_critical_capabilities && execution_mode == "full" {
            // Log warning but don't block - user may have explicitly set this
            eprintln!(
                "⚠️ Plan contains critical capabilities but execution mode is 'full' - \
                 consider using 'dry-run' or 'require-approval' for safety"
            );
        }

        Ok(())
    }

    /// Detect security level for a capability based on ID patterns or manifest metadata
    /// Returns: "low", "medium", "high", or "critical"
    /// 
    /// This implements pattern-based detection as fallback when capabilities
    /// don't declare security levels in their manifest metadata.
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

    /// Check if a capability requires approval based on execution mode and security level
    pub fn requires_approval(
        &self,
        capability_id: &str,
        execution_mode: &str,
    ) -> bool {
        let security_level = self.detect_security_level(capability_id);

        match execution_mode {
            "require-approval" => {
                // Require approval for medium, high, or critical operations
                security_level == "medium" || security_level == "high" || security_level == "critical"
            }
            "safe-only" => {
                // Require approval for high or critical operations
                security_level == "high" || security_level == "critical"
            }
            "dry-run" => {
                // No approval needed in dry-run (will be simulated)
                false
            }
            "full" => {
                // No approval needed in full execution mode
                false
            }
            _ => {
                // Unknown mode - default to requiring approval for critical operations
                security_level == "critical"
            }
        }
    }

    /// Check if a capability should be simulated in dry-run mode
    pub fn should_simulate_in_dry_run(&self, capability_id: &str, execution_mode: &str) -> bool {
        if execution_mode != "dry-run" {
            return false;
        }

        let security_level = self.detect_security_level(capability_id);
        // Simulate high and critical operations in dry-run
        security_level == "high" || security_level == "critical"
    }

    // ---------------------------------------------------------------------
    // Governance-Enforced Execution Interfaces
    // ---------------------------------------------------------------------

    /// Execute a plan through the governance pipeline.
    /// This is the primary interface for external code to execute plans safely.
    ///
    /// # Security
    /// This method ensures all plan execution goes through the GovernanceKernel,
    /// providing constitutional validation, intent sanitization, and proper audit trails.
    pub async fn execute_plan_governed(
        &self,
        plan: Plan,
        context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // Use the existing validate_and_execute method which handles all governance checks
        self.validate_and_execute(plan, context).await
    }

    /// Execute an entire intent graph through the governance pipeline.
    /// This orchestrates child intents and manages shared context while ensuring governance compliance.
    ///
    /// # Security
    /// This method ensures all plan execution within the intent graph goes through the GovernanceKernel.
    pub async fn execute_intent_graph_governed(
        &self,
        root_intent_id: &str,
        initial_context: &RuntimeContext,
    ) -> RuntimeResult<ExecutionResult> {
        // First validate that the root intent exists and can be executed
        let intent_id = root_intent_id.to_string();
        
        // Get the root intent from the graph
        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;
        
        let root_intent = graph
            .get_intent(&intent_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Intent not found: {}", root_intent_id)))?;
        
        drop(graph);

        // Get all child intents for this root intent
        let children = {
            let graph = self
                .intent_graph
                .lock()
                .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;
            graph.get_child_intents(&intent_id)
        };

        // Execute each child intent through governance
        let mut enhanced_context = initial_context.clone();
        enhanced_context.cross_plan_params.clear();

        let mut child_results = Vec::new();
        for child_intent in children {
            if let Some(child_plan) = self.get_plan_for_intent(&child_intent.intent_id)? {
                // Execute each child plan through governance
                let child_result = self.validate_and_execute(child_plan, &enhanced_context).await?;
                let exported = self.extract_exported_variables(&child_result);
                enhanced_context.cross_plan_params.extend(exported);
                child_results.push((child_intent.intent_id.clone(), child_result));
            }
        }

        // Execute root plan if it exists
        let mut root_result = None;
        if let Some(root_plan) = self.get_plan_for_intent(root_intent_id)? {
            root_result = Some(self.validate_and_execute(root_plan, &enhanced_context).await?);
        }

        // Build result summary
        let mut result_summary = Vec::new();

        for (child_id, result) in &child_results {
            if result.success {
                result_summary.push(format!("{}: {}", child_id, result.value));
            } else {
                result_summary.push(format!("{}: failed", child_id));
            }
        }

        if let Some(ref root) = root_result {
            if root.success {
                result_summary.push(format!("root: {}", root.value));
            } else {
                result_summary.push("root: failed".to_string());
            }
        }

        if result_summary.is_empty() {
            Ok(ExecutionResult {
                success: false,
                value: rtfs::runtime::values::Value::String("No plans executed".to_string()),
                metadata: Default::default(),
            })
        } else {
            Ok(ExecutionResult {
                success: true,
                value: rtfs::runtime::values::Value::String(format!(
                    "Governed orchestration of {} plans: {}",
                    child_results.len(),
                    result_summary.join(", ")
                )),
                metadata: Default::default(),
            })
        }
    }

    /// Get plan for a specific intent (governance-aware lookup)
    fn get_plan_for_intent(&self, intent_id: &str) -> RuntimeResult<Option<Plan>> {
        let graph = self
            .intent_graph
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock IntentGraph".to_string()))?;

        // Get plans associated with this intent from the orchestrator's plan archive
        // Use the governance-accessible method
        let plan_archive = self.orchestrator.get_plan_archive();
        
        let archivable_plans = plan_archive.get_plans_for_intent(&intent_id.to_string());
        
        if let Some(archivable_plan) = archivable_plans.first() {
            Ok(Some(Self::archivable_plan_to_plan(archivable_plan)))
        } else {
            Ok(None)
        }
    }

    /// Helper function to convert ArchivablePlan to Plan (duplicated from orchestrator for governance use)
    fn archivable_plan_to_plan(
        archivable_plan: &super::archivable_types::ArchivablePlan,
    ) -> Plan {
        use rtfs::runtime::values::Value as RtfsValue;
        use rtfs::parser::parse_expression;
        use rtfs::runtime::evaluator::Evaluator;
        use rtfs::runtime::execution_outcome::ExecutionOutcome;
        use rtfs::runtime::module_runtime::ModuleRegistry;
        use rtfs::runtime::pure_host::create_pure_host;
        use rtfs::runtime::security::RuntimeContext;
        use serde_json::Value as JsonValue;

        // Helper function to parse RTFS or JSON strings
        fn deserialize_value(value_str: &str) -> Option<Value> {
            if let Ok(expr) = parse_expression(value_str) {
                let module_registry = ModuleRegistry::new();
                let security_context = RuntimeContext::pure();
                let host = create_pure_host();
                let evaluator =
                    Evaluator::new(std::sync::Arc::new(module_registry), security_context, host);

                match evaluator.evaluate(&expr) {
                    Ok(ExecutionOutcome::Complete(value)) => Some(value),
                    _ => {
                        serde_json::from_str::<JsonValue>(value_str)
                            .ok()
                            .map(convert_json_value)
                    }
                }
            } else {
                serde_json::from_str::<JsonValue>(value_str)
                    .ok()
                    .map(convert_json_value)
            }
        }

        fn convert_json_value(json_val: JsonValue) -> Value {
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
                    let runtime_vec: Vec<Value> = arr.into_iter().map(convert_json_value).collect();
                    Value::Vector(runtime_vec)
                }
                JsonValue::Object(obj) => {
                    let mut runtime_map = std::collections::HashMap::new();
                    for (k, v) in obj {
                        runtime_map.insert(rtfs::ast::MapKey::String(k), convert_json_value(v));
                    }
                    Value::Map(runtime_map)
                }
            }
        }

        // Extract the plan body
        let raw_body = match &archivable_plan.body {
            crate::archivable_types::ArchivablePlanBody::String(s) => s.clone(),
            crate::archivable_types::ArchivablePlanBody::Legacy { steps, .. } => {
                steps.first().cloned().unwrap_or_else(|| "()".to_string())
            }
        };

        // If the body is a (plan ...) form, extract the :body property
        let plan_body = if raw_body.trim().starts_with("(plan") {
            match rtfs::parser::parse(&raw_body) {
                Ok(top_levels) => {
                    if let Some(rtfs::ast::TopLevel::Plan(plan_def)) = top_levels.first() {
                        if let Some(body_prop) =
                            plan_def.properties.iter().find(|p| p.key.0 == "body")
                        {
                            crate::rtfs_bridge::expression_to_rtfs_string(&body_prop.value)
                        } else {
                            raw_body
                        }
                    } else {
                        raw_body
                    }
                }
                Err(_) => raw_body,
            }
        } else {
            raw_body
        };

        // Convert ArchivablePlan back to Plan
        Plan {
            plan_id: archivable_plan.plan_id.clone(),
            name: archivable_plan.name.clone(),
            intent_ids: archivable_plan.intent_ids.clone(),
            language: super::types::PlanLanguage::Rtfs20,
            body: super::types::PlanBody::Rtfs(plan_body),
            status: archivable_plan.status.clone(),
            created_at: archivable_plan.created_at,
            metadata: archivable_plan
                .metadata
                .iter()
                .filter_map(|(k, v)| deserialize_value(v).map(|val| (k.clone(), val)))
                .collect(),
            input_schema: archivable_plan.input_schema.as_ref().and_then(|s| deserialize_value(s)),
            output_schema: archivable_plan.output_schema.as_ref().and_then(|s| deserialize_value(s)),
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

    /// Extract exported variables from execution result (simplified version)
    fn extract_exported_variables(&self, result: &ExecutionResult) -> HashMap<String, Value> {
        let mut exported = HashMap::new();
        if result.success {
            exported.insert("result".to_string(), result.value.clone());
        }
        exported
    }
}
