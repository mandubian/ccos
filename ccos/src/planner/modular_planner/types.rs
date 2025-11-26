//! Core types for the modular planner

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

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
    Display,    // Show to user
    Print,      // Console output
    Json,       // Format as JSON
    Table,      // Format as table
    Summary,    // Brief summary
    Other(String),
}

/// Domain hints help narrow down which services/APIs to search.
/// These are inferred from goal content, not hallucinated tool names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DomainHint {
    /// GitHub-related operations
    GitHub,
    /// Slack-related operations  
    Slack,
    /// File system operations
    FileSystem,
    /// Database operations
    Database,
    /// Web/HTTP operations
    Web,
    /// Email operations
    Email,
    /// Calendar operations
    Calendar,
    /// Generic/unknown domain
    Generic,
    /// Custom domain with name
    Custom(String),
}

impl DomainHint {
    /// Infer domain from goal text using keyword matching
    pub fn infer_from_text(text: &str) -> Option<Self> {
        let lower = text.to_lowercase();
        
        // GitHub indicators
        let github_keywords = ["github", "repo", "repository", "issue", "issues", 
                               "pull request", "pr", "prs", "commit", "branch", 
                               "fork", "star", "gist", "release", "workflow"];
        if github_keywords.iter().any(|k| lower.contains(k)) {
            return Some(DomainHint::GitHub);
        }
        
        // Slack indicators
        let slack_keywords = ["slack", "channel", "message", "dm", "thread", "emoji"];
        if slack_keywords.iter().any(|k| lower.contains(k)) {
            return Some(DomainHint::Slack);
        }
        
        // File system indicators
        let fs_keywords = ["file", "folder", "directory", "path", "read file", "write file"];
        if fs_keywords.iter().any(|k| lower.contains(k)) {
            return Some(DomainHint::FileSystem);
        }
        
        // Database indicators
        let db_keywords = ["database", "sql", "query", "table", "record", "row"];
        if db_keywords.iter().any(|k| lower.contains(k)) {
            return Some(DomainHint::Database);
        }
        
        // Web indicators
        let web_keywords = ["http", "url", "api", "endpoint", "request", "fetch url"];
        if web_keywords.iter().any(|k| lower.contains(k)) {
            return Some(DomainHint::Web);
        }
        
        None
    }
    
    /// Get MCP server names that might handle this domain
    pub fn likely_mcp_servers(&self) -> Vec<&'static str> {
        match self {
            DomainHint::GitHub => vec!["github", "gh"],
            DomainHint::Slack => vec!["slack"],
            DomainHint::FileSystem => vec!["filesystem", "fs"],
            DomainHint::Database => vec!["postgres", "mysql", "sqlite", "database"],
            DomainHint::Web => vec!["fetch", "http"],
            DomainHint::Email => vec!["email", "gmail", "outlook"],
            DomainHint::Calendar => vec!["calendar", "gcal"],
            DomainHint::Generic | DomainHint::Custom(_) => vec![],
        }
    }
}

/// Lightweight summary of a tool for decomposition context.
/// Used when providing LLM with available tools without full schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
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
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        let name_str = name.into();
        let desc_str = description.into();
        
        // Infer action from name
        let action = if name_str.starts_with("list_") || name_str.starts_with("get_all") {
            ApiAction::List
        } else if name_str.starts_with("get_") || name_str.starts_with("read_") {
            ApiAction::Get
        } else if name_str.starts_with("create_") || name_str.starts_with("add_") {
            ApiAction::Create
        } else if name_str.starts_with("update_") || name_str.starts_with("edit_") {
            ApiAction::Update
        } else if name_str.starts_with("delete_") || name_str.starts_with("remove_") {
            ApiAction::Delete
        } else if name_str.starts_with("search_") || name_str.starts_with("find_") {
            ApiAction::Search
        } else {
            ApiAction::Other(name_str.clone())
        };
        
        Self {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_domain_inference() {
        assert_eq!(
            DomainHint::infer_from_text("list issues in my repository"),
            Some(DomainHint::GitHub)
        );
        assert_eq!(
            DomainHint::infer_from_text("send message to slack channel"),
            Some(DomainHint::Slack)
        );
        assert_eq!(
            DomainHint::infer_from_text("read the file contents"),
            Some(DomainHint::FileSystem)
        );
        assert_eq!(
            DomainHint::infer_from_text("just do something"),
            None
        );
    }
    
    #[test]
    fn test_action_from_str() {
        assert_eq!(ApiAction::from_str("list"), ApiAction::List);
        assert_eq!(ApiAction::from_str("fetch"), ApiAction::List);
        assert_eq!(ApiAction::from_str("create"), ApiAction::Create);
        assert_eq!(ApiAction::from_str("unknown"), ApiAction::Other("unknown".to_string()));
    }
    
    #[test]
    fn test_sub_intent_builder() {
        let intent = SubIntent::new("List open issues", IntentType::ApiCall { action: ApiAction::List })
            .with_dependencies(vec![0])
            .with_param("owner", "mandubian")
            .with_domain(DomainHint::GitHub);
        
        assert_eq!(intent.description, "List open issues");
        assert_eq!(intent.dependencies, vec![0]);
        assert_eq!(intent.extracted_params.get("owner"), Some(&"mandubian".to_string()));
        assert_eq!(intent.domain_hint, Some(DomainHint::GitHub));
    }
}
