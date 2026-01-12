//! Shared types for MCP discovery
//!
//! This module defines core types used by the unified MCP discovery API.
//! Types are defined here rather than re-exported to provide a single source of truth.

use crate::mcp::rate_limiter::{RateLimitConfig, RetryPolicy};
use rtfs::ast::TypeExpr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export existing types to avoid duplication
pub use crate::capability_marketplace::mcp_discovery::{MCPServerConfig, MCPTool};

/// A discovered MCP tool with its schema
///
/// This represents a tool discovered from an MCP server, including its
/// parsed input/output schemas converted from JSON Schema to RTFS TypeExpr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredMCPTool {
    pub tool_name: String,
    pub description: Option<String>,
    pub input_schema: Option<TypeExpr>,
    pub output_schema: Option<TypeExpr>,
    pub input_schema_json: Option<serde_json::Value>,
}

impl DiscoveredMCPTool {
    /// Convert to MCPTool format for storage/serialization
    pub fn to_mcp_tool(&self) -> MCPTool {
        MCPTool {
            name: self.tool_name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema_json.clone(),
            output_schema: None, // Output schema is typically introspected lazily
            metadata: None,
            annotations: None,
        }
    }
}

/// Result of discovering tools from an MCP server
#[derive(Debug, Clone)]
pub struct MCPDiscoveryResult {
    /// Server configuration used for discovery
    pub server_config: MCPServerConfig,
    /// Discovered tools (parsed with schemas)
    pub tools: Vec<DiscoveredMCPTool>,
    /// Discovered resources (raw JSON)
    pub resources: Vec<serde_json::Value>,
    /// Protocol version from server
    pub protocol_version: String,
}

/// Options for tool discovery
#[derive(Debug, Clone)]
pub struct DiscoveryOptions {
    /// Whether to introspect output schemas (requires calling tools)
    /// Note: This is expensive and should be enabled only when needed
    pub introspect_output_schemas: bool,
    /// Whether to use cache if available
    pub use_cache: bool,
    /// Whether to register discovered tools in marketplace
    pub register_in_marketplace: bool,
    /// Whether to export discovered capabilities to RTFS files
    pub export_to_rtfs: bool,
    /// Directory to export RTFS files to (default: workspace_root/capabilities)
    pub export_directory: Option<String>,
    /// Custom auth headers (overrides server config)
    pub auth_headers: Option<HashMap<String, String>>,
    /// Retry policy for failed requests
    pub retry_policy: RetryPolicy,
    /// Rate limit configuration
    pub rate_limit: RateLimitConfig,
    /// Maximum number of parallel server discoveries (default: 5)
    /// This prevents overwhelming servers and getting rate-limited/banned
    pub max_parallel_discoveries: usize,
    /// Whether to skip output schema introspection by default (lazy loading)
    /// When true, output schemas are only introspected if explicitly requested
    /// Input schemas are always loaded as they're provided by MCP servers
    pub lazy_output_schemas: bool,
    /// Whether to ignore approved capability files and force network discovery
    pub ignore_approved_files: bool,
    /// Whether to force discovery even if export file exists
    pub force_refresh: bool,
    /// Whether to run in non-interactive mode (auto-approve prompts)
    pub non_interactive: bool,
    /// Whether to automatically create an approval request for discovered tools
    pub create_approval_request: bool,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            introspect_output_schemas: false,
            use_cache: false,
            register_in_marketplace: false,
            export_to_rtfs: false,
            export_directory: None,
            auth_headers: None,
            retry_policy: RetryPolicy::default(),
            rate_limit: RateLimitConfig::default(),
            max_parallel_discoveries: 5, // Conservative default to avoid rate limits
            lazy_output_schemas: true,   // Skip expensive introspection by default
            ignore_approved_files: false,
            force_refresh: false,
            non_interactive: false,
            create_approval_request: false,
        }
    }
}

/// A single step in an execution session
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionStep {
    pub step_number: usize,
    pub capability_id: String,
    pub inputs: serde_json::Value,
    pub result: serde_json::Value,
    pub rtfs_code: String,
    pub success: bool,
    pub executed_at: String,
}

/// An execution session that tracks steps toward a goal
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    pub goal: String,
    /// The original user intent that triggered this session (preserved verbatim)
    pub original_goal: Option<String>,
    pub steps: Vec<ExecutionStep>,
    pub context: std::collections::HashMap<String, serde_json::Value>,
    pub created_at: String,
}

impl Session {
    pub fn new(goal: &str) -> Self {
        let id = format!(
            "session_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );

        Self {
            id,
            goal: goal.to_string(),
            original_goal: Some(goal.to_string()),
            steps: Vec::new(),
            context: std::collections::HashMap::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Create a new session with an explicit original goal
    pub fn new_with_original_goal(goal: &str, original_goal: &str) -> Self {
        let mut session = Self::new(goal);
        session.original_goal = Some(original_goal.to_string());
        session
    }

    pub fn add_step(
        &mut self,
        capability_id: &str,
        inputs: serde_json::Value,
        result: serde_json::Value,
        success: bool,
    ) -> ExecutionStep {
        let rtfs_code = Self::inputs_to_rtfs(capability_id, &inputs);
        let step = ExecutionStep {
            step_number: self.steps.len() + 1,
            capability_id: capability_id.to_string(),
            inputs,
            result,
            rtfs_code,
            success,
            executed_at: chrono::Utc::now().to_rfc3339(),
        };
        self.steps.push(step.clone());
        step
    }

    /// Convert JSON inputs to RTFS call syntax
    pub fn inputs_to_rtfs(capability_id: &str, inputs: &serde_json::Value) -> String {
        if inputs.is_null() || (inputs.is_object() && inputs.as_object().unwrap().is_empty()) {
            return format!("(call \"{}\")", capability_id);
        }

        // Convert JSON to RTFS-compatible string using our internal helper
        // that ensures space-separated maps (valid RTFS) instead of comma-separated
        let rtfs_string = Self::json_to_rtfs(inputs);
        format!("(call \"{}\" {})", capability_id, rtfs_string)
    }

    /// Generate the complete RTFS plan from all steps
    pub fn to_rtfs_plan(&self) -> String {
        if self.steps.is_empty() {
            return format!(
                ";; Session: {} - No steps executed\n;; Goal: {}",
                self.id, self.goal
            );
        }

        if self.steps.len() == 1 {
            return format!(
                ";; Goal: {}\n;; Session: {}\n\n{}",
                self.goal, self.id, self.steps[0].rtfs_code
            );
        }

        // Multiple steps - wrap in (do ...)
        let mut lines = vec![
            format!(";; Goal: {}", self.goal),
            format!(";; Session: {}", self.id),
            "".to_string(),
            "(do".to_string(),
        ];

        for step in &self.steps {
            lines.push(format!("  {}", step.rtfs_code));
        }
        lines.push(")".to_string());

        lines.join("\n")
    }

    /// Generate a complete RTFS session file with metadata, causal chain, and replay plan
    pub fn to_rtfs_session(&self) -> String {
        let timestamp = chrono::Utc::now().timestamp();

        let mut lines = vec![
            format!(";; CCOS Session: {}", self.id),
            format!(";; Goal: {}", self.goal),
            format!(";; Created: {}", self.created_at),
            "".to_string(),
        ];

        lines.push(";; === SESSION METADATA ===".to_string());
        lines.push("(def session-meta".to_string());
        lines.push("  {".to_string());
        lines.push(format!("    :session-id \"{}\"", self.id));
        lines.push(format!("    :goal \"{}\"", self.goal.replace("\"", "\\\"")));
        if let Some(ref og) = self.original_goal {
            lines.push(format!(
                "    :original-goal \"{}\"",
                og.replace("\"", "\\\"")
            ));
        }
        lines.push(format!("    :created-at {}", timestamp));
        lines.push(format!("    :step-count {}", self.steps.len()));
        lines.push("  })".to_string());
        lines.push("".to_string());

        // Add causal chain
        lines.push(";; === CAUSAL CHAIN ===".to_string());
        lines.push("(def causal-chain".to_string());
        lines.push("  [".to_string());

        for (i, step) in self.steps.iter().enumerate() {
            lines.push("    {".to_string());
            lines.push(format!("      :step-number {}", step.step_number));
            lines.push(format!("      :capability-id \"{}\"", step.capability_id));
            lines.push(format!(
                "      :inputs {}",
                Self::json_to_rtfs(&step.inputs)
            ));
            lines.push(format!(
                "      :rtfs-code \"{}\"",
                step.rtfs_code.replace("\"", "\\\"")
            ));
            lines.push(format!("      :success {}", step.success));
            lines.push(format!("      :executed-at \"{}\"", step.executed_at));
            if i < self.steps.len() - 1 {
                lines.push("    }".to_string());
            } else {
                lines.push("    }])".to_string());
            }
        }

        if self.steps.is_empty() {
            lines.push("  ])".to_string());
        }

        lines.push("".to_string());

        // Add replay plan as a function (not executed on load)
        lines.push(";; === REPLAY PLAN (call (replay-session) to execute) ===".to_string());
        lines.push("(defn replay-session []".to_string());

        if self.steps.is_empty() {
            lines.push("  nil)".to_string());
        } else if self.steps.len() == 1 {
            lines.push(format!("  {})", self.steps[0].rtfs_code));
        } else {
            lines.push("  (do".to_string());
            for step in &self.steps {
                lines.push(format!("    {}", step.rtfs_code));
            }
            lines.push("  ))".to_string());
        }

        lines.join("\n")
    }

    /// Convert JSON value to RTFS representation
    fn json_to_rtfs(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::Null => "nil".to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(Self::json_to_rtfs).collect();
                format!("[{}]", items.join(" "))
            }
            serde_json::Value::Object(obj) => {
                let pairs: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!(":{} {}", k, Self::json_to_rtfs(v)))
                    .collect();
                format!("{{{}}}", pairs.join(" "))
            }
        }
    }
}
