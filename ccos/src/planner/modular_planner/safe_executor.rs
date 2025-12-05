//! Safe capability executor for planner grounding.
//!
//! This is a thin wrapper around the CapabilityMarketplace that:
//! - Only executes low-risk capabilities (no effects or only network/read effects).
//! - Converts simple param maps into `rtfs::runtime::values::Value`.
//! - Supports data pipeline via `_previous_result` injection.
//! - Returns the execution result for downstream grounding.

use std::sync::Arc;

use crate::capability_marketplace::CapabilityMarketplace;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use rtfs::ast::MapKey;
use rtfs::runtime::security::{RuntimeContext, SecurityAuthorizer};

/// Allowlist of effects that are considered safe for opportunistic execution.
const SAFE_EFFECTS: &[&str] = &[":network", ":compute", ":read"];

/// Denylist of effects that block safe execution.
const UNSAFE_EFFECTS: &[&str] = &[":filesystem", ":system", ":write", ":delete"];

/// Minimal executor that enforces an allowlist of effects.
pub struct SafeCapabilityExecutor {
    marketplace: Arc<CapabilityMarketplace>,
    runtime_context: RuntimeContext,
}

impl SafeCapabilityExecutor {
    /// Create with a controlled RuntimeContext that allows network/compute/read only.
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        let mut ctx = RuntimeContext::controlled(vec![]);
        ctx.allow_effect(":network");
        ctx.allow_effect(":compute");
        ctx.allow_effect(":read");
        // Disallow filesystem/system by default via effect denies
        ctx.deny_effect(":filesystem");
        ctx.deny_effect(":system");
        ctx.deny_effect(":write");
        ctx.deny_effect(":delete");
        Self {
            marketplace,
            runtime_context: ctx,
        }
    }

    /// Check if a capability is safe to execute (read-only, network allowed).
    pub async fn is_safe(&self, capability_id: &str) -> bool {
        let manifest = match self.marketplace.get_capability(capability_id).await {
            Some(m) => m,
            None => return false,
        };

        // If no effects declared, check if it's a known safe pattern
        if manifest.effects.is_empty() {
            // MCP list/search/get operations are typically safe
            let safe_patterns = ["list_", "search_", "get_", ".list", ".search", ".get"];
            return safe_patterns.iter().any(|p| capability_id.contains(p));
        }

        // Check effects against allowlist
        for eff in &manifest.effects {
            let norm = eff.trim().to_lowercase();
            if UNSAFE_EFFECTS.contains(&norm.as_str()) {
                return false;
            }
            if !SAFE_EFFECTS.contains(&norm.as_str()) && !norm.is_empty() {
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
                log::debug!(
                    "Safe exec skipped for {} (manifest not registered in marketplace)",
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
                    log::debug!(
                        "Safe exec blocked for {} (effect {} not allowed)",
                        capability_id, norm
                    );
                    return Ok(None);
                }
                if !SAFE_EFFECTS.contains(&norm.as_str()) && !norm.is_empty() {
                    log::debug!(
                        "Safe exec blocked for {} (effect {} not in allowlist)",
                        capability_id, norm
                    );
                    return Ok(None);
                }
            }
        } else {
            // No effects declared - check safe patterns for MCP capabilities
            let safe_patterns = ["list_", "search_", "get_", ".list", ".search", ".get"];
            let is_safe_pattern = safe_patterns.iter().any(|p| capability_id.contains(p));
            if !is_safe_pattern {
                log::debug!(
                    "Safe exec blocked for {} (no effects declared and not a safe pattern)",
                    capability_id
                );
                return Ok(None);
            }
        }

        // Authorize via RuntimeContext (effects/cap allowlist)
        let args: Vec<Value> = params
            .values()
            .cloned()
            .map(Value::String)
            .collect();
        let mut ctx = self.runtime_context.clone();
        ctx.allowed_capabilities.insert(capability_id.to_string());
        if let Err(e) = SecurityAuthorizer::authorize_capability(&ctx, capability_id, &args) {
            log::debug!(
                "Safe exec blocked for {} (authorization failed: {})",
                capability_id, e
            );
            return Ok(None);
        }

        // Build Value::Map from params, injecting _previous_result if available
        let mut map = std::collections::HashMap::new();
        for (k, v) in params {
            map.insert(MapKey::String(k.clone()), Value::String(v.clone()));
        }
        
        // Inject _previous_result for data pipeline support
        if let Some(prev) = previous_result {
            map.insert(MapKey::String("_previous_result".to_string()), prev.clone());
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
                log::info!(
                    "Safe exec result for {} (truncated): {}",
                    capability_id,
                    s.chars().take(400).collect::<String>()
                );
            }
        }
        Ok(Some(result))
    }
}

