//! Runtime Host
//!
//! The concrete implementation of the `HostInterface` that connects the RTFS runtime
//! to the CCOS stateful components like the Causal Chain and Capability Marketplace.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::runtime::host_interface::HostInterface;
use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeResult, RuntimeError};
use crate::ccos::causal_chain::CausalChain;
use crate::runtime::capability_marketplace::CapabilityMarketplace;
use crate::runtime::security::RuntimeContext;
use crate::ccos::types::{Action, ActionType, ExecutionResult};
// futures::executor used via fully qualified path below
use crate::ast::MapKey;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
struct HostPlanContext {
    plan_id: String,
    intent_ids: Vec<String>,
    parent_action_id: String,
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
        }
    }

    fn build_context_snapshot(&self, step_name: &str, args: &[Value], capability_id: &str) -> Option<Value> {
        // Policy gate: allow exposing read-only context for this capability?
        // Evaluate dynamic policy using manifest metadata (tags) when available.
        // Step-level override may force exposure or suppression
        if let Ok(ov) = self.step_exposure_override.lock() { if let Some((expose, _)) = ov.last() { if !*expose { return None; } } }

        let allow_exposure = futures::executor::block_on(async {
            if !self.security_context.expose_readonly_context { return false; }
            // Try to fetch manifest to obtain metadata/tags
            if let Some(manifest) = self.capability_marketplace.get_capability(capability_id).await {
                // Tags may be stored in metadata as comma-separated under "tags" or repeated keys like tag:*
                let mut tags: Vec<String> = Vec::new();
                if let Some(tag_list) = manifest.metadata.get("tags") {
                    tags.extend(tag_list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
                }
                for (k,v) in &manifest.metadata {
                    if k.starts_with("tag:") { tags.push(v.clone()); }
                }
                return self.security_context.is_context_exposure_allowed_for(capability_id, Some(&tags));
            }
            // Fallback to exact/prefix policy without tags
            self.security_context.is_context_exposure_allowed_for(capability_id, None)
        });
        if !allow_exposure { return None; }
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
        map.insert(MapKey::String("plan_id".to_string()), Value::String(plan_ctx_owned.plan_id.clone()));
        map.insert(MapKey::String("primary_intent".to_string()), Value::String(plan_ctx_owned.intent_ids.first().cloned().unwrap_or_default()));
        map.insert(MapKey::String("intent_ids".to_string()), Value::Vector(plan_ctx_owned.intent_ids.iter().cloned().map(Value::String).collect()));
        map.insert(MapKey::String("step".to_string()), Value::String(step_name.to_string()));
        map.insert(MapKey::String("inputs_hash".to_string()), Value::String(hash));

        // Apply step override key filtering if present
        if let Ok(ov) = self.step_exposure_override.lock() { if let Some((_, Some(keys))) = ov.last() { let allowed: std::collections::HashSet<&String> = keys.iter().collect(); map.retain(|k, _| match k { MapKey::String(s) => allowed.contains(s), _ => true }); } }
        Some(Value::Map(map))
    }

    /// Sets the context for a new plan execution.
    pub fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>, parent_action_id: String) {
        if let Ok(mut guard) = self.execution_context.lock() {
            *guard = Some(HostPlanContext {
            plan_id,
            intent_ids,
            parent_action_id,
            });
        }
    }

    /// Clears the execution context after a plan has finished.
    pub fn clear_execution_context(&self) {
        if let Ok(mut guard) = self.execution_context.lock() { *guard = None; }
    }

    fn get_causal_chain(&self) -> RuntimeResult<MutexGuard<CausalChain>> {
        self.causal_chain.lock().map_err(|_| RuntimeError::Generic("Failed to lock CausalChain".to_string()))
    }

    fn get_context(&self) -> RuntimeResult<HostPlanContext> {
        let guard = self.execution_context.lock().map_err(|_| RuntimeError::Generic("FATAL: Host lock poisoned".to_string()))?;
        let ctx = guard.clone().ok_or_else(|| RuntimeError::Generic("FATAL: Host method called without a valid execution context".to_string()))?;
        Ok(ctx)
    }
}

impl HostInterface for RuntimeHost {
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // 1. Security Validation
        if !self.security_context.is_capability_allowed(name) {
            return Err(RuntimeError::SecurityViolation {
                operation: "call".to_string(),
                capability: name.to_string(),
                context: format!("{:?}", self.security_context),
            });
        }

        let context = self.get_context()?;
        // New calling convention: provide :args plus optional :context snapshot
        let mut call_map: std::collections::HashMap<MapKey, Value> = std::collections::HashMap::new();
        call_map.insert(MapKey::Keyword(crate::ast::Keyword("args".to_string())), Value::List(args.to_vec()));
        if let Some(snapshot) = self.build_context_snapshot(name, args, name) {
            call_map.insert(MapKey::Keyword(crate::ast::Keyword("context".to_string())), snapshot);
        }
        let capability_args = Value::Map(call_map);

        // 2. Create and log the CapabilityCall action
        let action = Action::new(
            ActionType::CapabilityCall,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(context.parent_action_id.clone()))
        .with_name(name)
        .with_arguments(&args);

        let _action_id = self.get_causal_chain()?.append(&action)?;

        // 3. Execute the capability via the marketplace
        // Bridge async marketplace to sync evaluator using futures::executor::block_on
        let result = futures::executor::block_on(async {
            self.capability_marketplace
                .execute_capability(name, &capability_args)
                .await
        });

        // 4. Log the result to the Causal Chain
        let execution_result = match &result {
            Ok(value) => ExecutionResult { success: true, value: value.clone(), metadata: Default::default() },
            Err(e) => ExecutionResult { success: false, value: Value::Nil, metadata: Default::default() }.with_error(&e.to_string()),
        };

        self.get_causal_chain()?.record_result(action, execution_result)?;

        result
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

    fn notify_step_completed(&self, step_action_id: &str, result: &ExecutionResult) -> RuntimeResult<()> {
        let context = self.get_context()?;
        let action = Action::new(
            ActionType::PlanStepCompleted,
            context.plan_id.clone(),
            context.intent_ids.first().cloned().unwrap_or_default(),
        )
        .with_parent(Some(step_action_id.to_string()))
        .with_result(result.clone());

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

    fn set_execution_context(&self, plan_id: String, intent_ids: Vec<String>, parent_action_id: String) {
        // Call inherent method explicitly to avoid trait-method recursion
        RuntimeHost::set_execution_context(self, plan_id, intent_ids, parent_action_id);
    }

    fn clear_execution_context(&self) {
        // Call inherent method explicitly to avoid trait-method recursion
        RuntimeHost::clear_execution_context(self);
    }

    fn set_step_exposure_override(&self, expose: bool, context_keys: Option<Vec<String>>) {
        if let Ok(mut ov) = self.step_exposure_override.lock() { ov.push((expose, context_keys)); }
    }

    fn clear_step_exposure_override(&self) {
        if let Ok(mut ov) = self.step_exposure_override.lock() { let _ = ov.pop(); }
    }

    fn get_context_value(&self, key: &str) -> Option<Value> {
        let ctx = self.execution_context.lock().ok()?.clone()?;
        match key {
            // Primary identifiers
            "plan-id" => Some(Value::String(ctx.plan_id.clone())),
            "intent-id" => Some(Value::String(ctx.intent_ids.first().cloned().unwrap_or_default())),
            "intent-ids" => Some(Value::Vector(ctx.intent_ids.iter().cloned().map(Value::String).collect())),
            // Parent action ID
            "parent-action-id" => Some(Value::String(ctx.parent_action_id.clone())),
            _ => None,
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