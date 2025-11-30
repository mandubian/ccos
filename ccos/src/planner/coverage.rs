use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::planner::menu::CapabilityMenuEntry;
use crate::planner::signals::{
    GoalRequirement, GoalRequirementKind, GoalSignals, RequirementPriority, RequirementReadiness,
};

/// Summary of a plan step used for coverage evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepSummary {
    pub id: String,
    pub capability_id: Option<String>,
    pub capability_class: Option<String>,
    pub provided_inputs: BTreeMap<String, String>,
    pub produced_outputs: Vec<String>,
    pub notes: Option<String>,
}

/// Represents the result of a coverage analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageCheck {
    pub status: CoverageStatus,
    pub unmet_requirements: Vec<RequirementGap>,
    pub missing_capabilities: Vec<String>,
    pub incomplete_capabilities: Vec<String>,
    pub pending_capabilities: Vec<String>,
    pub advisories: Vec<String>,
    pub satisfied_requirements: Vec<String>,
}

impl CoverageCheck {
    pub fn satisfied() -> Self {
        Self {
            status: CoverageStatus::Satisfied,
            unmet_requirements: Vec::new(),
            missing_capabilities: Vec::new(),
            incomplete_capabilities: Vec::new(),
            pending_capabilities: Vec::new(),
            advisories: Vec::new(),
            satisfied_requirements: Vec::new(),
        }
    }

    pub fn with_gap(mut self, gap: RequirementGap) -> Self {
        self.unmet_requirements.push(gap);
        self.status = CoverageStatus::NeedsAttention;
        self
    }

    pub fn record_satisfied(mut self, requirement_id: impl Into<String>) -> Self {
        self.satisfied_requirements.push(requirement_id.into());
        self
    }

    pub fn provision_targets(&self) -> Vec<String> {
        let mut targets = self.missing_capabilities.clone();
        for capability in &self.incomplete_capabilities {
            if !targets.contains(capability) {
                targets.push(capability.clone());
            }
        }
        targets
    }
}

/// Status of coverage evaluation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CoverageStatus {
    Satisfied,
    NeedsAttention,
}

/// Gap produced when a requirement is not covered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementGap {
    pub requirement: GoalRequirement,
    pub explanation: String,
}

/// Trait implemented by coverage analyzers.
pub trait GoalCoverageAnalyzer {
    fn evaluate(
        &self,
        signals: &GoalSignals,
        plan: &[PlanStepSummary],
        menu: &[CapabilityMenuEntry],
    ) -> CoverageCheck;
}

/// Default implementation that evaluates coverage against goal requirements.
pub struct DefaultGoalCoverageAnalyzer;

impl DefaultGoalCoverageAnalyzer {
    fn check_requirement(
        requirement: &GoalRequirement,
        plan: &[PlanStepSummary],
        menu: &[CapabilityMenuEntry],
    ) -> Result<(), RequirementGap> {
        match &requirement.kind {
            GoalRequirementKind::MustCallCapability { capability_id } => {
                // First check if capability is actually available in the menu
                let capability_available = menu.iter().any(|entry| entry.id == *capability_id);
                // Then check if any step in the plan uses it
                let step_exists = plan
                    .iter()
                    .any(|step| step.capability_id.as_deref() == Some(capability_id.as_str()));

                // For a capability to be satisfied, it must both:
                // 1. Be available in the menu (not just referenced in plan)
                // 2. Be invoked by a step in the plan
                if capability_available && step_exists {
                    return Ok(());
                }

                // If capability is not in menu, it's missing even if referenced in plan
                if !capability_available {
                    let missing_capability = RequirementGap {
                        requirement: requirement.clone(),
                        explanation: format!(
                            "Required capability '{}' is not available (referenced in plan but not in menu).",
                            capability_id
                        ),
                    };
                    return Err(missing_capability);
                }

                // Capability is available but not used in plan
                let missing_capability = RequirementGap {
                    requirement: requirement.clone(),
                    explanation: format!(
                        "No step invokes required capability '{}'.",
                        capability_id
                    ),
                };
                Err(missing_capability)
            }
            GoalRequirementKind::MustSatisfyCapabilityClass { class } => {
                if plan.iter().any(|step| {
                    step.capability_class
                        .as_deref()
                        .map(|cls| cls.eq_ignore_ascii_case(class))
                        .unwrap_or(false)
                }) {
                    Ok(())
                } else {
                    Err(RequirementGap {
                        requirement: requirement.clone(),
                        explanation: format!(
                            "Plan is missing a step that satisfies capability class '{}'.",
                            class
                        ),
                    })
                }
            }
            GoalRequirementKind::MustProduceOutput { key } => {
                if plan
                    .iter()
                    .any(|step| step.produced_outputs.iter().any(|out| out == key))
                {
                    Ok(())
                } else {
                    Err(RequirementGap {
                        requirement: requirement.clone(),
                        explanation: format!("No step produces required output '{}'.", key),
                    })
                }
            }
            GoalRequirementKind::MustFilter {
                field,
                expected_value,
            } => {
                let matched = plan.iter().any(|step| {
                    let has_filter_capability = step
                        .capability_class
                        .as_deref()
                        .map(|cls| cls.contains("filter"))
                        .unwrap_or(false)
                        || step
                            .capability_id
                            .as_deref()
                            .map(|id| id.contains("filter"))
                            .unwrap_or(false)
                        // Also check if the step uses an adapter that might be doing filtering
                        || step
                            .capability_id
                            .as_deref()
                            .map(|id| id.contains("adapter") || id.contains("parse"))
                            .unwrap_or(false)
                        // Check if rtfs step contains filter in its expression
                        || (step
                            .capability_id
                            .as_deref()
                            .map(|id| id == "rtfs")
                            .unwrap_or(false)
                            && step
                                .provided_inputs
                                .get("expression")
                                .map(|expr| expr.contains("filter"))
                                .unwrap_or(false));
                    if !has_filter_capability {
                        return false;
                    }
                    if let Some(field) = field {
                        let field_match = step
                            .provided_inputs
                            .keys()
                            .any(|k| k.eq_ignore_ascii_case(field));
                        if !field_match {
                            return false;
                        }
                    }
                    if let Some(expected) = expected_value {
                        // Extract the actual string value from Value enum for comparison
                        let expected_value_str = match expected {
                            rtfs::runtime::values::Value::String(s) => s.clone(),
                            rtfs::runtime::values::Value::Integer(i) => i.to_string(),
                            rtfs::runtime::values::Value::Boolean(b) => b.to_string(),
                            _ => format!("{:?}", expected),
                        };
                        // Check if the expected value appears in inputs or notes
                        // The input values are formatted as "literal('value')" or "var(name)" etc.
                        let input_matches = step.provided_inputs
                            .values()
                            .any(|v| {
                                // Check if the formatted input contains the expected value
                                // (e.g., "literal('RTFS')" contains "RTFS")
                                v.contains(&expected_value_str)
                            });
                        // For rtfs steps, also check the expression directly
                        let expression_matches = step
                            .provided_inputs
                            .get("expression")
                            .map(|expr| expr.contains(&expected_value_str))
                            .unwrap_or(false);
                        let notes_matches = step
                            .notes
                            .as_ref()
                            .map(|n| n.contains(&expected_value_str))
                            .unwrap_or(false);
                        
                        // Debug logging for filter requirement checks
                        eprintln!("🔍 FILTER CHECK: Step '{}' (capability: {:?}), expected: '{}', inputs: {:?}, expression: {:?}, notes: {:?}, input_matches: {}, expression_matches: {}, notes_matches: {}",
                            step.id,
                            step.capability_id,
                            expected_value_str,
                            step.provided_inputs.values().collect::<Vec<_>>(),
                            step.provided_inputs.get("expression"),
                            step.notes,
                            input_matches,
                            expression_matches,
                            notes_matches
                        );
                        
                        input_matches || expression_matches || notes_matches
                    } else {
                        true
                    }
                });
                if matched {
                    Ok(())
                } else {
                    Err(RequirementGap {
                        requirement: requirement.clone(),
                        explanation: format!(
                            "No filtering step satisfies requirement {:?}.",
                            requirement.kind
                        ),
                    })
                }
            }
            GoalRequirementKind::Custom { description } => {
                let matched = plan.iter().any(|step| {
                    step.notes
                        .as_ref()
                        .map(|n| n.contains(description))
                        .unwrap_or(false)
                        || step
                            .capability_class
                            .as_ref()
                            .map(|cls| cls.contains(description))
                            .unwrap_or(false)
                });
                if matched {
                    Ok(())
                } else {
                    Err(RequirementGap {
                        requirement: requirement.clone(),
                        explanation: format!(
                            "Custom requirement '{}' not satisfied by any step.",
                            description
                        ),
                    })
                }
            }
        }
    }

    fn detect_missing_capabilities(
        requirement: &GoalRequirement,
        menu: &[CapabilityMenuEntry],
    ) -> Option<String> {
        if let GoalRequirementKind::MustCallCapability { capability_id } = &requirement.kind {
            if !menu.iter().any(|entry| entry.id == *capability_id) {
                return Some(capability_id.clone());
            }
        }
        None
    }
}

impl GoalCoverageAnalyzer for DefaultGoalCoverageAnalyzer {
    fn evaluate(
        &self,
        signals: &GoalSignals,
        plan: &[PlanStepSummary],
        menu: &[CapabilityMenuEntry],
    ) -> CoverageCheck {
        let mut result = CoverageCheck::satisfied();
        let mut missing_capabilities = Vec::new();
        let mut incomplete_capabilities = Vec::new();
        let mut pending_capabilities = Vec::new();

        for requirement in &signals.requirements {
            match Self::check_requirement(requirement, plan, menu) {
                Ok(()) => {
                    result = result.record_satisfied(requirement.id.clone());
                }
                Err(gap) => {
                    if requirement.priority == RequirementPriority::Must {
                        result = result.with_gap(gap);
                        if let Some(capability_id) = requirement.capability_id() {
                            match requirement.readiness {
                                RequirementReadiness::Incomplete => {
                                    if !incomplete_capabilities.contains(&capability_id.to_string())
                                    {
                                        incomplete_capabilities.push(capability_id.to_string());
                                    }
                                }
                                RequirementReadiness::PendingExternal => {
                                    if !pending_capabilities.contains(&capability_id.to_string()) {
                                        pending_capabilities.push(capability_id.to_string());
                                    }
                                }
                                _ => {
                                    if let Some(missing) =
                                        Self::detect_missing_capabilities(requirement, menu)
                                    {
                                        missing_capabilities.push(missing);
                                    }
                                }
                            }
                        } else if let Some(missing) =
                            Self::detect_missing_capabilities(requirement, menu)
                        {
                            missing_capabilities.push(missing);
                        }
                    } else {
                        // downgrade to advisory for non-must requirements
                        result.advisories.push(gap.explanation);
                    }
                }
            }
        }

        result.missing_capabilities = missing_capabilities;
        result.incomplete_capabilities = incomplete_capabilities;
        result.pending_capabilities = pending_capabilities;
        result
    }
}
