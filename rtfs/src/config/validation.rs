//! Configuration validation utilities
//!
//! Provides a best-effort pass producing non-fatal warnings & suggestions so
//! users can keep TOML concise while still surfacing misconfigurations early.

use crate::config::types::{AgentConfig, PolicyConfig};

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationMessage {
    pub level: ValidationLevel,
    pub code: &'static str,
    pub message: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValidationReport {
    pub messages: Vec<ValidationMessage>,
}

impl ValidationReport {
    pub fn push(
        &mut self,
        level: ValidationLevel,
        code: &'static str,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) {
        self.messages.push(ValidationMessage {
            level,
            code,
            message: message.into(),
            suggestion,
        });
    }
    pub fn has_errors(&self) -> bool {
        self.messages
            .iter()
            .any(|m| matches!(m.level, ValidationLevel::Error))
    }
}

/// Validate an `AgentConfig` returning a report of findings.
/// This never fails hard; caller decides how to surface messages.
pub fn validate_config(cfg: &AgentConfig) -> ValidationReport {
    let mut report = ValidationReport::default();

    // Governance policies: ensure at least one policy defined
    if cfg.governance.policies.is_empty() {
        report.push(
            ValidationLevel::Warning,
            "governance.no_policies",
            "No governance policies defined (all operations effectively uncontrolled)",
            Some("Add [governance.policies.default] or shorthand [governance.policies] default='allow'".to_string()),
        );
    } else {
        for (name, pol) in &cfg.governance.policies {
            validate_policy(name, pol, &mut report);
        }
    }

    // LLM profiles default name check
    if let Some(llm) = &cfg.llm_profiles {
        if let Some(default_name) = &llm.default {
            // Expanded names use ':' between set and spec; help user if they used '.'
            if default_name.contains('.') && !default_name.contains(':') {
                let colon = default_name.replace('.', ":");
                report.push(
                    ValidationLevel::Warning,
                    "llm_profiles.default.separator",
                    format!(
                        "Default profile '{}' uses '.'; expanded synthetic names use ':'",
                        default_name
                    ),
                    Some(format!("Use '{}' instead", colon)),
                );
            }
        }
        if llm.profiles.is_empty()
            && llm
                .model_sets
                .as_ref()
                .map(|v| v.is_empty())
                .unwrap_or(true)
        {
            report.push(
                ValidationLevel::Warning,
                "llm_profiles.empty",
                "llm_profiles present but both 'profiles' and 'model_sets' are empty",
                Some("Remove the section or add at least one profile".to_string()),
            );
        }
    }

    // Network egress sanity
    if cfg.network.enabled && cfg.network.egress.via == "none" {
        report.push(
            ValidationLevel::Info,
            "network.enabled_none_via",
            "Network enabled but egress.via='none' (no outbound)",
            Some("Set [network.egress] via='direct' or 'proxy'".to_string()),
        );
    }

    // Delegation threshold hints
    if let Some(th) = cfg.delegation.threshold {
        if th < 0.2 {
            report.push(
                ValidationLevel::Warning,
                "delegation.threshold.low",
                format!(
                    "Delegation threshold {:.2} is very low; almost all agents will pass",
                    th
                ),
                Some("Consider >= 0.5 for moderate trust".to_string()),
            );
        }
        if th > 0.95 {
            report.push(
                ValidationLevel::Warning,
                "delegation.threshold.high",
                format!(
                    "Delegation threshold {:.2} is very high; delegation may never occur",
                    th
                ),
                Some("Consider <= 0.85 unless intentionally restrictive".to_string()),
            );
        }
    }

    report
}

fn validate_policy(name: &str, pol: &PolicyConfig, report: &mut ValidationReport) {
    if pol.requires_approvals > 5 {
        report.push(
            ValidationLevel::Warning,
            "governance.policy.approvals.high",
            format!(
                "Policy '{}' requires {} approvals (may block throughput)",
                name, pol.requires_approvals
            ),
            Some("Reduce to <=2 for faster iteration".to_string()),
        );
    }
    if pol.budgets.max_cost_usd == 0.0 && pol.budgets.token_budget == 0.0 {
        report.push(
            ValidationLevel::Info,
            "governance.policy.budgets.zero",
            format!(
                "Policy '{}' has zero budgets (may block cost-tracked ops)",
                name
            ),
            Some("Increase max_cost_usd or token_budget if blocking execution".to_string()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::{
        AgentConfig, BudgetConfig, GovernanceConfig, KeyConfig, PolicyConfig,
    };
    use std::collections::HashMap;

    #[test]
    fn test_validation_default_separator_warning() {
        let mut pols = HashMap::new();
        pols.insert(
            "default".to_string(),
            PolicyConfig {
                risk_tier: "low".to_string(),
                requires_approvals: 0,
                budgets: BudgetConfig {
                    max_cost_usd: 1.0,
                    token_budget: 100.0,
                },
            },
        );
        let cfg = AgentConfig {
            llm_profiles: Some(crate::config::types::LlmProfilesConfig {
                default: Some("openrouter_free.balanced".to_string()),
                profiles: vec![],
                model_sets: None,
            }),
            governance: GovernanceConfig {
                policies: pols,
                keys: KeyConfig {
                    verify: "k".to_string(),
                },
            },
            ..AgentConfig::default()
        };
        let report = validate_config(&cfg);
        assert!(report
            .messages
            .iter()
            .any(|m| m.code == "llm_profiles.default.separator"));
    }

    #[test]
    fn test_validation_no_policies_warning() {
        let cfg = AgentConfig::default();
        let report = validate_config(&cfg);
        assert!(report
            .messages
            .iter()
            .any(|m| m.code == "governance.no_policies"));
    }
}
