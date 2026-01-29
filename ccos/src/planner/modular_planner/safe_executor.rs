//! Safe capability executor for planner grounding.
//!
//! This is a thin wrapper around the CapabilityMarketplace that:
//! - Only executes low-risk capabilities (no effects or only network/read effects).
//! - Supports agent-specific constraints for dynamic approval.
//! - Can queue risky capabilities for approval via UnifiedApprovalQueue.
//! - Converts simple param maps into `rtfs::runtime::values::Value`.
//! - Supports data pipeline via `_previous_result` injection.
//! - Returns the execution result for downstream grounding.

use std::sync::Arc;

use crate::approval::types::{ApprovalCategory, RiskAssessment, RiskLevel};
use crate::approval::{storage_file::FileApprovalStorage, UnifiedApprovalQueue};
use crate::capability_marketplace::types::AgentConstraints;
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

/// Configuration for approval behavior
#[derive(Debug, Clone)]
pub struct ApprovalConfig {
    /// If true, queue risky capabilities for approval instead of rejecting
    pub queue_for_approval: bool,
    /// Optional context for approval requests
    pub approval_context: Option<String>,
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            queue_for_approval: false,
            approval_context: None,
        }
    }
}

/// Result of a safety check
#[derive(Debug)]
pub enum SafetyCheckResult {
    /// Capability is safe to execute
    Safe,
    /// Capability is not safe - blocked
    Blocked { reason: String },
    /// Capability requires approval - ID returned
    NeedsApproval { request_id: String },
}

/// Minimal executor that enforces an allowlist of effects.
pub struct SafeCapabilityExecutor {
    pub marketplace: Arc<CapabilityMarketplace>,
    runtime_context: RuntimeContext,
    /// Optional approval queue for queuing risky capabilities
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    /// Optional agent constraints for dynamic approval rules
    agent_constraints: Option<AgentConstraints>,
    /// Configuration for approval behavior
    approval_config: ApprovalConfig,
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
            approval_queue: None,
            agent_constraints: None,
            approval_config: ApprovalConfig::default(),
        }
    }

    /// Add approval queue for queuing risky capabilities
    pub fn with_approval_queue(mut self, queue: UnifiedApprovalQueue<FileApprovalStorage>) -> Self {
        self.approval_queue = Some(queue);
        self
    }

    /// Add agent constraints for dynamic effect checking
    pub fn with_agent_constraints(mut self, constraints: AgentConstraints) -> Self {
        self.agent_constraints = Some(constraints);
        self
    }

    /// Configure approval behavior
    pub fn with_approval_config(mut self, config: ApprovalConfig) -> Self {
        self.approval_config = config;
        self
    }

    /// Enable queuing for approval (convenience method)
    pub fn enable_approval_queuing(mut self, context: Option<String>) -> Self {
        self.approval_config.queue_for_approval = true;
        self.approval_config.approval_context = context;
        self
    }

    /// Check if capability requires approval based on agent constraints.
    /// Returns Some(reason) if approval is needed, None if safe.
    pub fn requires_approval(&self, manifest: &CapabilityManifest) -> Option<String> {
        // No effects means we can't determine safety
        if manifest.effects.is_empty() {
            return Some("No effects declared - cannot determine safety".to_string());
        }

        // Check agent constraints if available (takes precedence over static lists)
        if let Some(ref constraints) = self.agent_constraints {
            for eff in &manifest.effects {
                let norm = eff.trim().to_lowercase();
                let norm = norm.strip_prefix(':').unwrap_or(&norm);

                // Check denied effects
                if constraints
                    .denied_effects
                    .iter()
                    .any(|d| d.to_lowercase() == norm)
                {
                    return Some(format!("Effect '{}' is in agent's denied list", norm));
                }

                // If allowlist is non-empty, check effect is in it
                if !constraints.allowed_effects.is_empty() {
                    let allowed = constraints
                        .allowed_effects
                        .iter()
                        .any(|a| a.to_lowercase() == norm);
                    if !allowed && !norm.is_empty() {
                        return Some(format!("Effect '{}' is not in agent's allowed list", norm));
                    }
                }
            }
            // Agent constraints passed
            return None;
        }

        // Fall back to static denylist/allowlist
        for eff in &manifest.effects {
            let norm = eff.trim().to_lowercase();
            let norm = norm.strip_prefix(':').unwrap_or(&norm);

            if UNSAFE_EFFECTS.contains(&norm) {
                return Some(format!("Effect '{}' is in unsafe effects list", norm));
            }
            if !SAFE_EFFECTS.contains(&norm) && !norm.is_empty() {
                return Some(format!("Effect '{}' is not in safe effects list", norm));
            }
        }

        None // Safe
    }

    /// Queue a capability for effect approval.
    /// Returns the approval request ID.
    pub async fn queue_for_approval(
        &self,
        manifest: &CapabilityManifest,
        intent_description: &str,
    ) -> RuntimeResult<String> {
        let queue = self.approval_queue.as_ref().ok_or_else(|| {
            RuntimeError::Generic(
                "No approval queue configured for SafeCapabilityExecutor".to_string(),
            )
        })?;

        let request_id = queue
            .add_effect_approval(
                manifest.id.clone(),
                manifest.effects.clone(),
                intent_description.to_string(),
                RiskAssessment {
                    level: RiskLevel::Medium,
                    reasons: vec![format!(
                        "Capability {} requires effect-based approval",
                        manifest.id
                    )],
                },
                24, // 24 hours expiry
            )
            .await?;

        log::info!(
            "Queued capability {} for effect approval (request_id: {})",
            manifest.id,
            request_id
        );

        Ok(request_id)
    }

    /// Check if a capability is approved (has a resolved approval).
    pub async fn is_approved(&self, capability_id: &str) -> RuntimeResult<bool> {
        if let Some(ref queue) = self.approval_queue {
            // Check if there's an approved request for this capability
            let pending = queue.list_pending_effects().await?;
            // If there's a pending request, it's not yet approved
            for req in pending {
                if let ApprovalCategory::EffectApproval {
                    capability_id: cid, ..
                } = &req.category
                {
                    if cid == capability_id {
                        return Ok(false);
                    }
                }
            }
        }
        // No pending request means either approved or never queued
        Ok(true)
    }

    /// Returns true if the capability is considered "safe" for opportunistic execution.
    #[allow(dead_code)]
    fn is_safe(&self, manifest: &CapabilityManifest) -> bool {
        self.requires_approval(manifest).is_none()
    }

    /// Execute if the capability is low-risk; otherwise return None or queue for approval.
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

        // Check if approval is needed
        if let Some(reason) = self.requires_approval(&manifest) {
            if self.approval_config.queue_for_approval && self.approval_queue.is_some() {
                // Queue for approval
                let desc = self
                    .approval_config
                    .approval_context
                    .clone()
                    .unwrap_or_else(|| format!("Execute capability {}", capability_id));

                let request_id = self.queue_for_approval(&manifest, &desc).await?;

                log::info!(
                    "Capability {} queued for approval: {} (request_id: {})",
                    capability_id,
                    reason,
                    request_id
                );
                return Ok(None);
            } else {
                eprintln!(
                    "DEBUG: Safe exec blocked for {} ({})",
                    capability_id, reason
                );
                return Ok(None);
            }
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

    /// Execute with full safety check, returning detailed result.
    /// This method provides more information about why execution was blocked.
    pub async fn execute_with_safety_check(
        &self,
        capability_id: &str,
        params: &std::collections::HashMap<String, String>,
        previous_result: Option<&Value>,
        intent_description: &str,
    ) -> RuntimeResult<SafetyCheckResult> {
        // Fetch manifest
        let manifest = match self.marketplace.get_capability(capability_id).await {
            Some(m) => m,
            None => {
                return Ok(SafetyCheckResult::Blocked {
                    reason: "Capability not found in marketplace".to_string(),
                });
            }
        };

        // Check if approval is needed
        if let Some(reason) = self.requires_approval(&manifest) {
            if self.approval_config.queue_for_approval && self.approval_queue.is_some() {
                let request_id = self
                    .queue_for_approval(&manifest, intent_description)
                    .await?;
                return Ok(SafetyCheckResult::NeedsApproval { request_id });
            } else {
                return Ok(SafetyCheckResult::Blocked { reason });
            }
        }

        // Execute safely
        match self
            .execute_if_safe(capability_id, params, previous_result)
            .await?
        {
            Some(_) => Ok(SafetyCheckResult::Safe),
            None => Ok(SafetyCheckResult::Blocked {
                reason: "Authorization failed".to_string(),
            }),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_marketplace::types::{EffectType, HttpCapability, ProviderType};
    use std::collections::HashMap;

    /// Helper to create test manifests
    fn test_manifest(id: &str, effects: Vec<&str>) -> CapabilityManifest {
        CapabilityManifest {
            id: id.to_string(),
            name: "Test".to_string(),
            description: "Test capability".to_string(),
            provider: ProviderType::Http(HttpCapability {
                base_url: "http://test.local".to_string(),
                auth_token: None,
                timeout_ms: 5000,
            }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: effects.into_iter().map(|s| s.to_string()).collect(),
            metadata: HashMap::new(),
            agent_metadata: None,
            domains: vec![],
            categories: vec![],
            effect_type: EffectType::Pure,
        }
    }

    #[test]
    fn test_approval_config_default() {
        let config = ApprovalConfig::default();
        assert!(!config.queue_for_approval);
        assert!(config.approval_context.is_none());
    }

    #[test]
    fn test_requires_approval_with_safe_effects() {
        use crate::capabilities::registry::CapabilityRegistry;
        use tokio::sync::RwLock;

        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let executor = SafeCapabilityExecutor::new(marketplace);

        let manifest = test_manifest("test", vec!["read", "network"]);
        assert!(executor.requires_approval(&manifest).is_none());
    }

    #[test]
    fn test_requires_approval_with_unsafe_effects() {
        use crate::capabilities::registry::CapabilityRegistry;
        use tokio::sync::RwLock;

        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let executor = SafeCapabilityExecutor::new(marketplace);

        let manifest = test_manifest("test", vec!["filesystem"]);
        assert!(executor.requires_approval(&manifest).is_some());
    }

    #[test]
    fn test_requires_approval_with_agent_constraints() {
        use crate::capabilities::registry::CapabilityRegistry;
        use tokio::sync::RwLock;

        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));

        // Agent allows filesystem but denies network
        let constraints = AgentConstraints {
            allowed_effects: vec!["filesystem".to_string()],
            denied_effects: vec!["network".to_string()],
            ..Default::default()
        };

        let executor = SafeCapabilityExecutor::new(marketplace).with_agent_constraints(constraints);

        // Filesystem is allowed by this agent (overrides static list)
        let manifest_fs = test_manifest("test_fs", vec!["filesystem"]);
        assert!(executor.requires_approval(&manifest_fs).is_none());

        // Network is denied by this agent
        let manifest_net = test_manifest("test_net", vec!["network"]);
        assert!(executor.requires_approval(&manifest_net).is_some());
    }
}
