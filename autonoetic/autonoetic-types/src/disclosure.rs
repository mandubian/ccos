//! Disclosure Policy Types
//!
//! Defines how sensitive information should be handled in assistant replies.

use serde::{Deserialize, Serialize};

/// The disclosure classification of a piece of information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisclosureClass {
    /// Information that can be safely disclosed to the user verbatim.
    Public,
    /// Information that is for internal context only.
    Internal,
    /// Information that should be summarized or paraphrased, not repeated verbatim.
    Confidential,
    /// Highly sensitive information that should never be echoed verbatim.
    Secret,
}

impl Default for DisclosureClass {
    fn default() -> Self {
        Self::Public
    }
}

/// A disclosure rule mapping a source (tool/path) to a classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisclosureRule {
    /// The source pattern (e.g., `memory.read`, `state/secrets/*`, `sandbox.exec`)
    pub source: String,
    /// The path or argument pattern if applicable (e.g. `state/secrets/*`). If not provided,
    /// applies to all calls to `source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_pattern: Option<String>,
    /// The class to assign to information matching this rule.
    pub class: DisclosureClass,
}

/// The disclosure policy configuration for an agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisclosurePolicy {
    /// Ordered list of disclosure rules. The first matching rule applies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<DisclosureRule>,
    /// The default classification if no rules match. Defaults to `Public`.
    #[serde(default)]
    pub default_class: DisclosureClass,
}
