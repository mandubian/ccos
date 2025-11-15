use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use futures::future::LocalBoxFuture;
use rtfs::runtime::error::RuntimeResult;

use crate::capability_marketplace::types::CapabilityManifest;
use crate::planner::coverage::CoverageCheck;
use crate::planner::signals::{GoalSignals, RequirementReadiness};
use crate::synthesis::registration_flow::TestResult;

#[derive(Debug, Clone)]
pub struct PendingCapabilityRequest {
    pub capability_id: String,
    pub request_id: Option<String>,
    pub suggested_human_action: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedCapabilityInfo {
    pub manifest: CapabilityManifest,
    pub resolution_method: Option<String>,
    pub provider_info: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CapabilityProvisionAction {
    Discovered {
        capability: ResolvedCapabilityInfo,
    },
    Synthesized {
        capability: ResolvedCapabilityInfo,
        tests_run: Vec<TestResult>,
    },
    PendingExternal {
        capability_id: String,
        request_id: Option<String>,
        suggested_human_action: Option<String>,
    },
    Failed {
        capability_id: String,
        reason: String,
        recoverable: bool,
    },
    Skipped,
}

pub type CapabilityProvisionFn = Arc<
    dyn Fn(
        String,
        &GoalSignals,
    ) -> LocalBoxFuture<'static, RuntimeResult<CapabilityProvisionAction>>,
>;

/// Outcome after attempting to provision capabilities for unmet requirements.
#[derive(Debug, Clone)]
pub enum RequirementResolutionOutcome {
    /// No resolver available or no action needed.
    NoAction,
    /// Resolver provided new capability manifests (via discovery or synthesis).
    CapabilitiesDiscovered {
        capabilities: Vec<ResolvedCapabilityInfo>,
        pending_requests: Vec<PendingCapabilityRequest>,
    },
    /// Synthesized new capabilities (typically via LLM flow).
    Synthesized {
        capabilities: Vec<ResolvedCapabilityInfo>,
        tests_run: Vec<TestResult>,
        pending_requests: Vec<PendingCapabilityRequest>,
    },
    /// Awaiting external implementation or governance response.
    AwaitingExternal {
        capability_requests: Vec<PendingCapabilityRequest>,
    },
    /// Resolution failed; include user-facing reasons and whether retryable.
    Failed { reason: String, recoverable: bool },
}

/// Strategy object that can ensure capabilities exist for uncovered requirements.
#[derive(Clone)]
pub struct RequirementResolver {
    provisioner: Option<CapabilityProvisionFn>,
    attempted: Arc<Mutex<HashSet<String>>>,
}

impl RequirementResolver {
    pub fn new(provisioner: Option<CapabilityProvisionFn>) -> Self {
        Self {
            provisioner,
            attempted: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Attempt to provision capabilities for the missing requirement surfaces in `coverage`.
    pub async fn ensure_capabilities(
        &self,
        coverage: &CoverageCheck,
        signals: &GoalSignals,
    ) -> RuntimeResult<RequirementResolutionOutcome> {
        let provisioner = match &self.provisioner {
            Some(provisioner) => provisioner.clone(),
            None => return Ok(RequirementResolutionOutcome::NoAction),
        };

        // Collect provision targets from coverage
        let mut provision_targets = coverage.provision_targets();

        // Also check signals for capabilities with Identified readiness that aren't in coverage
        // (e.g., unknown capabilities added during plan validation)
        // These are capabilities that were identified as needed but haven't been resolved yet
        for requirement in &signals.requirements {
            if let Some(capability_id) = requirement.capability_id() {
                let capability_id_str = capability_id.to_string();
                // If this capability is Identified and not already in provision targets or other lists
                if requirement.readiness == RequirementReadiness::Identified
                    && !provision_targets.contains(&capability_id_str)
                    && !coverage
                        .incomplete_capabilities
                        .contains(&capability_id_str)
                    && !coverage.pending_capabilities.contains(&capability_id_str)
                {
                    // Even if it's in missing_capabilities, we still want to try to provision it
                    // (it might have been added after the coverage check)
                    provision_targets.push(capability_id_str);
                }
            }
        }

        if provision_targets.is_empty() {
            return Ok(RequirementResolutionOutcome::NoAction);
        }

        let capability_ids = self.filter_unattempted(&provision_targets);
        if capability_ids.is_empty() {
            return Ok(RequirementResolutionOutcome::NoAction);
        }

        let mut discovered_capabilities = Vec::new();
        let mut synthesized_capabilities = Vec::new();
        let mut synthesized_tests: Vec<TestResult> = Vec::new();
        let mut pending_requests: Vec<PendingCapabilityRequest> = Vec::new();
        let mut failures = Vec::new();
        let mut has_recoverable_failure = false;
        let mut has_fatal_failure = false;

        for capability_id in capability_ids {
            match (provisioner)(capability_id.clone(), signals).await {
                Ok(CapabilityProvisionAction::Discovered { capability }) => {
                    discovered_capabilities.push(capability);
                }
                Ok(CapabilityProvisionAction::Synthesized {
                    capability,
                    tests_run,
                }) => {
                    synthesized_capabilities.push(capability);
                    synthesized_tests.extend(tests_run);
                }
                Ok(CapabilityProvisionAction::PendingExternal {
                    capability_id: pending,
                    request_id,
                    suggested_human_action,
                }) => {
                    pending_requests.push(PendingCapabilityRequest {
                        capability_id: pending,
                        request_id,
                        suggested_human_action,
                    });
                }
                Ok(CapabilityProvisionAction::Failed {
                    capability_id: failed_id,
                    reason,
                    recoverable,
                }) => {
                    failures.push(format!("{}: {}", failed_id, reason));
                    if recoverable {
                        has_recoverable_failure = true;
                    } else {
                        has_fatal_failure = true;
                    }
                }
                Ok(CapabilityProvisionAction::Skipped) => {}
                Err(err) => {
                    failures.push(format!("{}: {}", capability_id, err));
                    has_fatal_failure = true;
                }
            }
        }

        if !synthesized_capabilities.is_empty() {
            return Ok(RequirementResolutionOutcome::Synthesized {
                capabilities: synthesized_capabilities,
                tests_run: synthesized_tests,
                pending_requests: pending_requests.clone(),
            });
        }

        if !discovered_capabilities.is_empty() {
            return Ok(RequirementResolutionOutcome::CapabilitiesDiscovered {
                capabilities: discovered_capabilities,
                pending_requests: pending_requests.clone(),
            });
        }

        if !pending_requests.is_empty() {
            return Ok(RequirementResolutionOutcome::AwaitingExternal {
                capability_requests: pending_requests,
            });
        }

        if !failures.is_empty() {
            let reason = failures.join("; ");
            let recoverable = has_recoverable_failure && !has_fatal_failure;
            return Ok(RequirementResolutionOutcome::Failed {
                reason,
                recoverable,
            });
        }

        Ok(RequirementResolutionOutcome::NoAction)
    }

    fn filter_unattempted(&self, candidates: &[String]) -> Vec<String> {
        let mut guard = self
            .attempted
            .lock()
            .expect("requirement attempts mutex poisoned");
        candidates
            .iter()
            .filter(|cap| guard.insert(cap.to_string()))
            .cloned()
            .collect()
    }
}
