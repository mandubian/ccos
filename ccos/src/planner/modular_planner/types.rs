//! Core types for the modular planner

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A sub-intent produced by decomposition, before resolution to a capability.
///
/// Unlike the old "PlannedStep" with capability hints, SubIntent focuses on
/// describing WHAT needs to be done, not HOW (which tool to use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubIntent {
    /// Human-readable description of what this step accomplishes
    pub description: String,

    /// The semantic type of this intent (helps with resolution)
    pub intent_type: IntentType,

    /// Indices of other SubIntents this one depends on (for ordering)
    pub dependencies: Vec<usize>,

    /// Parameters extracted from the goal that are relevant to this intent
    pub extracted_params: HashMap<String, String>,

    /// Optional hints about the domain (inferred, not hallucinated tool names)
    pub domain_hint: Option<DomainHint>,
}

impl SubIntent {
    pub fn new(description: impl Into<String>, intent_type: IntentType) -> Self {
        Self {
            description: description.into(),
            intent_type,
            dependencies: vec![],
            extracted_params: HashMap::new(),
            domain_hint: None,
        }
    }

    pub fn with_dependencies(mut self, deps: Vec<usize>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extracted_params.insert(key.into(), value.into());
        self
    }

    pub fn with_domain(mut self, domain: DomainHint) -> Self {
        self.domain_hint = Some(domain);
        self
    }
}

/// Semantic classification of what an intent does.
/// This helps the resolution phase find appropriate capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentType {
    /// Requires user interaction (ask for input, confirm action)
    UserInput {
        /// What to ask the user for
        prompt_topic: String,
    },

    /// External API call (fetch data, create resource, etc.)
    ApiCall {
        /// The action verb (list, get, create, update, delete, search)
        action: ApiAction,
    },

    /// Pure data transformation (filter, sort, format, extract)
    DataTransform {
        /// The transformation type
        transform: TransformType,
    },

    /// Output to user (display results, print message)
    Output {
        /// What kind of output
        format: OutputFormat,
    },

    /// Complex intent that may need further decomposition
    Composite,
}

/// API action verbs for resolution matching
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApiAction {
    List,
    Get,
    Create,
    Update,
    Delete,
    Search,
    Execute,
    Other(String),
}

impl ApiAction {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "list" | "fetch" | "retrieve" => ApiAction::List,
            "get" | "read" | "show" | "view" => ApiAction::Get,
            "create" | "add" | "new" | "make" => ApiAction::Create,
            "update" | "edit" | "modify" | "change" => ApiAction::Update,
            "delete" | "remove" | "drop" => ApiAction::Delete,
            "search" | "find" | "query" | "lookup" => ApiAction::Search,
            "execute" | "run" | "invoke" | "call" => ApiAction::Execute,
            other => ApiAction::Other(other.to_string()),
        }
    }

    /// Returns keywords that help match this action to tool names
    pub fn matching_keywords(&self) -> Vec<&'static str> {
        match self {
            ApiAction::List => vec!["list", "fetch", "get_all", "retrieve"],
            ApiAction::Get => vec!["get", "read", "fetch", "retrieve"],
            ApiAction::Create => vec!["create", "add", "new", "insert", "post"],
            ApiAction::Update => vec!["update", "edit", "modify", "patch", "put"],
            ApiAction::Delete => vec!["delete", "remove", "destroy"],
            ApiAction::Search => vec!["search", "find", "query", "lookup"],
            ApiAction::Execute => vec!["execute", "run", "invoke", "call"],
            ApiAction::Other(_) => vec![],
        }
    }
}

/// Data transformation types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformType {
    Filter,
    Sort,
    GroupBy,
    Count,
    Aggregate,
    Format,
    Extract,
    Parse,
    Validate,
    Other(String),
}

/// Output format types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    Display, // Show to user
    Print,   // Console output
    Json,    // Format as JSON
    Table,   // Format as table
    Summary, // Brief summary
    Other(String),
}

/// Domain hints help narrow down which services/APIs to search.
/// Now configured via config/domain_hints.toml instead of hardcoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainHint {
    /// Generic/unknown domain
    Generic,
    /// Named domain (loaded from config)
    Custom(String),
}

impl DomainHint {
    /// Convert to domain string for catalog filtering
    pub fn to_domain_string(&self) -> String {
        match self {
            DomainHint::Generic => "generic".to_string(),
            DomainHint::Custom(s) => s.clone(),
        }
    }

    /// Create from a domain string
    pub fn from_string(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "generic" | "" => DomainHint::Generic,
            other => DomainHint::Custom(other.to_string()),
        }
    }

    /// Infer all possible domains from goal text using config-based keywords
    pub fn infer_all_from_text(text: &str) -> Vec<Self> {
        super::domain_config::infer_all_domains(text)
            .into_iter()
            .map(|s| DomainHint::Custom(s))
            .collect()
    }

    /// Infer domain from goal text (returns first match from config)
    pub fn infer_from_text(text: &str) -> Option<Self> {
        super::domain_config::infer_domain(text).map(DomainHint::Custom)
    }

    /// Get MCP server names that might handle this domain (from config)
    pub fn likely_mcp_servers(&self) -> Vec<String> {
        match self {
            DomainHint::Generic => vec![],
            DomainHint::Custom(domain) => super::domain_config::mcp_servers_for_domain(domain),
        }
    }
}

/// Lightweight summary of a tool for decomposition context.
/// Used when providing LLM with available tools without full schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
    /// Stable identifier (prefer fully qualified capability id)
    pub id: String,

    /// Tool name (e.g., "list_issues")
    pub name: String,

    /// One-line description
    pub description: String,

    /// Domain this tool belongs to
    pub domain: DomainHint,

    /// Primary action this tool performs
    pub action: ApiAction,

    /// Optional input schema for argument validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
}

impl ToolSummary {
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let name_str = name.into();
        let desc_str = description.into();

        // Action type should be determined by:
        // 1. Explicit metadata from tool introspection (preferred)
        // 2. LLM inference from tool description during decomposition
        //
        // Previously used prefix matching (list_ -> List, get_ -> Get) but that was fragile.
        // The caller can set action explicitly with .with_action() if known.
        let action = ApiAction::Other(name_str.clone());

        Self {
            id: name_str.clone(),
            name: name_str,
            description: desc_str,
            domain: DomainHint::Generic,
            action,
            input_schema: None,
        }
    }

    pub fn with_domain(mut self, domain: DomainHint) -> Self {
        self.domain = domain;
        self
    }

    pub fn with_input_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = Some(schema);
        self
    }

    /// Set action type explicitly (preferred over guessing from name patterns)
    pub fn with_action(mut self, action: ApiAction) -> Self {
        self.action = action;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_inference() {
        // Domain inference uses config file - these tests depend on config being loaded
        // With config loaded, keywords like "repository" map to github domain
        if let Some(domain) = DomainHint::infer_from_text("list issues in my repository") {
            assert_eq!(domain, DomainHint::Custom("github".to_string()));
        }
        if let Some(domain) = DomainHint::infer_from_text("send message to slack channel") {
            assert_eq!(domain, DomainHint::Custom("slack".to_string()));
        }
        // "just do something" has no domain keywords, so should return None
        assert_eq!(DomainHint::infer_from_text("just do something"), None);
    }

    #[test]
    fn test_action_from_str() {
        assert_eq!(ApiAction::from_str("list"), ApiAction::List);
        assert_eq!(ApiAction::from_str("fetch"), ApiAction::List);
        assert_eq!(ApiAction::from_str("create"), ApiAction::Create);
        assert_eq!(
            ApiAction::from_str("unknown"),
            ApiAction::Other("unknown".to_string())
        );
    }

    #[test]
    fn test_sub_intent_builder() {
        let intent = SubIntent::new(
            "List open issues",
            IntentType::ApiCall {
                action: ApiAction::List,
            },
        )
        .with_dependencies(vec![0])
        .with_param("owner", "mandubian")
        .with_domain(DomainHint::Custom("github".to_string()));

        assert_eq!(intent.description, "List open issues");
        assert_eq!(intent.dependencies, vec![0]);
        assert_eq!(
            intent.extracted_params.get("owner"),
            Some(&"mandubian".to_string())
        );
        assert_eq!(
            intent.domain_hint,
            Some(DomainHint::Custom("github".to_string()))
        );
    }
}
