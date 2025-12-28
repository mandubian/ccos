//! Safe capability executor for planner grounding.
//!
//! This is a thin wrapper around the CapabilityMarketplace that:
//! - Only executes low-risk capabilities (no effects or only network/read effects).
//! - Converts simple param maps into `rtfs::runtime::values::Value`.
//! - Supports data pipeline via `_previous_result` injection.
//! - Returns the execution result for downstream grounding.

use std::sync::Arc;

use crate::capability_marketplace::CapabilityManifest;
use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::security::{RuntimeContext, SecurityAuthorizer};
use rtfs::runtime::values::Value;

/// Allowlist of effects that are considered safe for opportunistic execution.
/// Normalized to lowercase without leading colons.
const SAFE_EFFECTS: &[&str] = &["network", "compute", "read", "output", "pure", "llm"];

/// Denylist of effects that block safe execution.
/// Normalized to lowercase without leading colons.
const UNSAFE_EFFECTS: &[&str] = &["filesystem", "system", "write", "delete"];

/// Minimal executor that enforces an allowlist of effects.
pub struct SafeCapabilityExecutor {
    pub marketplace: Arc<CapabilityMarketplace>,
    runtime_context: RuntimeContext,
}

impl SafeCapabilityExecutor {
    /// Create with a controlled RuntimeContext that allows network/compute/read only.
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        // Use controlled context with effects-based permissions
        let mut ctx = RuntimeContext::controlled(vec![]);
        ctx.allow_effect("network");
        ctx.allow_effect("compute");
        ctx.allow_effect("read");
        ctx.allow_effect("llm");
        ctx.deny_effect("filesystem");
        ctx.deny_effect("system");
        ctx.deny_effect("write");
        ctx.deny_effect("delete");
        SafeCapabilityExecutor {
            marketplace,
            runtime_context: ctx,
        }
    }

    /// Returns true if the capability is considered "safe" for opportunistic execution.
    fn is_safe(&self, manifest: &CapabilityManifest) -> bool {
        // If effects list is empty, we can't determine safety
        if manifest.effects.is_empty() {
            log::debug!(
                "DEBUG: Safe exec blocked for {} (no effects metadata - cannot determine safety)",
                manifest.id
            );
            // Capabilities should declare their effects explicitly
            return false;
        }

        // Check effects against allowlist
        for eff in &manifest.effects {
            // Normalize: trim, lowercase, strip leading colon
            let norm = eff.trim().to_lowercase();
            let norm = norm.strip_prefix(':').unwrap_or(&norm);

            if UNSAFE_EFFECTS.contains(&norm) {
                log::debug!(
                    "DEBUG: Safe exec blocked for {} (effect {} not allowed)",
                    manifest.id,
                    norm
                );
                return false;
            }
            if !SAFE_EFFECTS.contains(&norm) && !norm.is_empty() {
                log::debug!(
                    "DEBUG: Safe exec blocked for {} (effect {} not in allowlist: {:?})",
                    manifest.id,
                    norm,
                    SAFE_EFFECTS
                );
                return false;
            }
        }

        true
    }

    /// Execute if the capability is low-risk; otherwise return None.
    ///
    /// # Arguments
    /// - `capability_id`: The capability to execute
    /// - `params`: String parameters from the intent
    /// - `previous_result`: Optional result from a previous step in the pipeline
    pub async fn execute_if_safe(
        &self,
        capability_id: &str,
        params: &std::collections::HashMap<String, String>,
        previous_result: Option<&Value>,
    ) -> RuntimeResult<Option<Value>> {
        // Fetch manifest
        let manifest = match self.marketplace.get_capability(capability_id).await {
            Some(m) => m,
            None => {
                eprintln!(
                    "DEBUG: Safe exec skipped for {} (manifest not registered in marketplace)",
                    capability_id
                );
                return Ok(None);
            }
        };

        // Quick effect gate using manifest (prefer explicit effects over defaults)
        if !manifest.effects.is_empty() {
            for eff in &manifest.effects {
                let norm = eff.trim().to_lowercase();
                if UNSAFE_EFFECTS.contains(&norm.as_str()) {
                    eprintln!(
                        "DEBUG: Safe exec blocked for {} (effect {} not allowed)",
                        capability_id, norm
                    );
                    return Ok(None);
                }
                if !SAFE_EFFECTS.contains(&norm.as_str()) && !norm.is_empty() {
                    eprintln!(
                        "DEBUG: Safe exec blocked for {} (effect {} not in allowlist: {:?})",
                        capability_id, norm, SAFE_EFFECTS
                    );
                    return Ok(None);
                }
            }
        } else {
            // No effects declared - require explicit metadata for safety
            // Previously used pattern matching (list_, get_, search_) which was fragile
            // Now we require capabilities to declare effects explicitly
            eprintln!(
                "DEBUG: Safe exec blocked for {} (no effects metadata - cannot determine safety)",
                capability_id
            );
            return Ok(None);
        }

        // Authorize via RuntimeContext (effects/cap allowlist)
        let args: Vec<Value> = params.values().cloned().map(Value::String).collect();
        let mut ctx = self.runtime_context.clone();
        ctx.allowed_capabilities.insert(capability_id.to_string());
        if let Err(e) = SecurityAuthorizer::authorize_capability(&ctx, capability_id, &args) {
            log::info!(
                "Safe exec blocked for {} (authorization failed: {})",
                capability_id,
                e
            );
            return Ok(None);
        }

        // Build Value::Map from params, injecting _previous_result if available
        let mut map = std::collections::HashMap::new();

        // Phase 2 Trace: Parameter Normalization
        // Check input schema for required keys - but do NOT add domain-specific aliases here
        // Any normalization should be driven by the tool's schema, not hardcoded rules
        let normalized_params = params.clone();

        for (k, v) in &normalized_params {
            let k = k.clone();

            let rtfs_val = if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(v) {
                if let Ok(rv) = crate::utils::value_conversion::json_to_rtfs_value(&json_val) {
                    rv
                } else {
                    Value::String(v.clone())
                }
            } else {
                Value::String(v.clone())
            };

            // Insert both string and keyword keys for compatibility
            map.insert(MapKey::String(k.clone()), rtfs_val.clone());
            map.insert(MapKey::Keyword(Keyword(k.clone())), rtfs_val);
        }

        // Inject _previous_result for data pipeline support
        if let Some(prev) = previous_result {
            eprintln!(
                "DEBUG: SafeExec injecting prev result type: {:?}",
                prev.type_name()
            );

            map.insert(MapKey::String("_previous_result".to_string()), prev.clone());
            map.insert(
                MapKey::Keyword(Keyword("_previous_result".to_string())),
                prev.clone(),
            );

            // NOTE: Removed blind "data" aliasing heuristic.
            // Previously we auto-aliased _previous_result to :data, but this caused
            // parameter pollution (e.g., list_issues receiving get_me results as :data).
            // Capabilities that need upstream data should:
            // 1. Explicitly reference _previous_result in their schema/adapter
            // 2. Have an adapter that maps _previous_result to the correct param name
            // See: issue with Pig Latin demo polluting list_issues with user profile

            log::debug!(
                "Safe exec injecting _previous_result into {} params",
                capability_id
            );
        }

        let input = Value::Map(map);

        let result = self
            .marketplace
            .execute_capability(capability_id, &input)
            .await?;
        if let Ok(json) = crate::utils::value_conversion::rtfs_value_to_json(&result) {
            if let Ok(s) = serde_json::to_string(&json) {
                log::debug!(
                    "Safe exec result for {} (truncated): {}",
                    capability_id,
                    s.chars().take(400).collect::<String>()
                );
            }
        }
        Ok(Some(result))
    }
}
