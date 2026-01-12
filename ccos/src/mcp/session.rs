//! MCP Interactive Session Management
//!
//! Tracks execution steps and state for interactive sessions.

use crate::utils::value_conversion::rtfs_value_to_json;
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A single step in an execution session
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub goal: String,
    /// The original user intent that triggered this session (preserved verbatim)
    pub original_goal: Option<String>,
    pub steps: Vec<ExecutionStep>,
    pub context: HashMap<String, serde_json::Value>,
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
            context: HashMap::new(),
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

/// Session store - thread-safe storage for active sessions
pub type SessionStore = Arc<RwLock<HashMap<String, Session>>>;

pub fn create_session_store() -> SessionStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Generate a URL-safe slug from a goal string
pub fn slugify_goal(goal: &str) -> String {
    goal.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .take(5) // Limit to 5 words
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(50) // Limit total length
        .collect()
}

/// Save a session to an RTFS file in the sessions directory
pub fn save_session(
    session: &Session,
    sessions_dir: Option<&std::path::Path>,
    filename: Option<&str>,
) -> std::io::Result<std::path::PathBuf> {
    let dir = match sessions_dir {
        Some(d) => d.to_path_buf(),
        None => crate::utils::fs::get_configured_sessions_path(),
    };

    // Create sessions directory if it doesn't exist
    std::fs::create_dir_all(&dir)?;

    // Generate filename: use override if provided, otherwise slugify goal
    let actual_filename = match filename {
        Some(f) => {
            if f.ends_with(".rtfs") {
                f.to_string()
            } else {
                format!("{}.rtfs", f)
            }
        }
        None => {
            let goal_slug = slugify_goal(&session.goal);
            format!("{}_{}.rtfs", session.id, goal_slug)
        }
    };

    let filepath = dir.join(&actual_filename);

    // Generate and write RTFS content
    let content = session.to_rtfs_session();
    std::fs::write(&filepath, content)?;

    eprintln!("[ccos-mcp] Session saved to: {}", filepath.display());
    Ok(filepath)
}

/// Find a session on disk by its ID
pub async fn find_session_on_disk(session_id: &str) -> Option<Session> {
    let sessions_dir = crate::utils::fs::get_configured_sessions_path();
    if !sessions_dir.exists() {
        return None;
    }

    let Ok(entries) = std::fs::read_dir(sessions_dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "rtfs") {
            let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if filename.starts_with(session_id) {
                if let Ok(content) = std::fs::read_to_string(path) {
                    return parse_session_from_rtfs(&content);
                }
            }
        }
    }
    None
}

/// Parse a Session struct from RTFS content
pub fn parse_session_from_rtfs(content: &str) -> Option<Session> {
    // Parse using rtfs::parser::parse which returns Vec<TopLevel>
    let top_levels = match rtfs::parser::parse(content) {
        Ok(tls) => tls,
        Err(e) => {
            eprintln!("[ccos-mcp] Failed to parse session RTFS: {:?}", e);
            return None;
        }
    };

    let mut session_id = None;
    let mut goal = None;
    let mut original_goal = None;
    let mut steps = Vec::new();
    let mut created_at = chrono::Utc::now().to_rfc3339();

    for tl in top_levels {
        if let rtfs::ast::TopLevel::Expression(expr) = tl {
            if let rtfs::ast::Expression::Def(def_expr) = expr {
                if def_expr.symbol.0 == "session-meta" {
                    if let rtfs::ast::Expression::Map(map) = &*def_expr.value {
                        for (key, val) in map {
                            let key_str = key.to_string();
                            match key_str.as_str() {
                                ":session-id" => {
                                    if let rtfs::ast::Expression::Literal(
                                        rtfs::ast::Literal::String(s),
                                    ) = val
                                    {
                                        session_id = Some(s.clone());
                                    }
                                }
                                ":goal" => {
                                    if let rtfs::ast::Expression::Literal(
                                        rtfs::ast::Literal::String(s),
                                    ) = val
                                    {
                                        goal = Some(s.clone());
                                    }
                                }
                                ":original-goal" => {
                                    if let rtfs::ast::Expression::Literal(
                                        rtfs::ast::Literal::String(s),
                                    ) = val
                                    {
                                        original_goal = Some(s.clone());
                                    }
                                }
                                ":created-at" => {
                                    if let rtfs::ast::Expression::Literal(
                                        rtfs::ast::Literal::Integer(ts),
                                    ) = val
                                    {
                                        if let Some(dt) = chrono::DateTime::from_timestamp(*ts, 0) {
                                            created_at = dt.to_rfc3339();
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                } else if def_expr.symbol.0 == "causal-chain" {
                    if let rtfs::ast::Expression::Vector(vec) = &*def_expr.value {
                        for step_expr in vec {
                            if let rtfs::ast::Expression::Map(step_map) = step_expr {
                                let mut step_number = 0;
                                let mut capability_id = String::new();
                                let mut inputs = json!({});
                                let mut success = true;
                                let mut executed_at = String::new();
                                let mut rtfs_code_opt = None;

                                for (key, val) in step_map {
                                    let key_str = key.to_string();
                                    match key_str.as_str() {
                                        ":step-number" => {
                                            if let rtfs::ast::Expression::Literal(
                                                rtfs::ast::Literal::Integer(n),
                                            ) = val
                                            {
                                                step_number = *n as usize;
                                            }
                                        }
                                        ":capability-id" => {
                                            if let rtfs::ast::Expression::Literal(
                                                rtfs::ast::Literal::String(s),
                                            ) = val
                                            {
                                                capability_id = s.clone();
                                            }
                                        }
                                        ":inputs" => {
                                            let rtfs_val = Value::from(val.clone());
                                            if let Ok(j) = rtfs_value_to_json(&rtfs_val) {
                                                inputs = j;
                                            }
                                        }
                                        ":success" => {
                                            if let rtfs::ast::Expression::Literal(
                                                rtfs::ast::Literal::Boolean(b),
                                            ) = val
                                            {
                                                success = *b;
                                            }
                                        }
                                        ":executed-at" => {
                                            if let rtfs::ast::Expression::Literal(
                                                rtfs::ast::Literal::String(s),
                                            ) = val
                                            {
                                                executed_at = s.clone();
                                            }
                                        }
                                        ":rtfs-code" => {
                                            if let rtfs::ast::Expression::Literal(
                                                rtfs::ast::Literal::String(s),
                                            ) = val
                                            {
                                                rtfs_code_opt = Some(s.clone());
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                let rtfs_code = rtfs_code_opt.unwrap_or_else(|| {
                                    Session::inputs_to_rtfs(&capability_id, &inputs)
                                });

                                steps.push(ExecutionStep {
                                    step_number,
                                    capability_id,
                                    inputs,
                                    result: json!({}),
                                    rtfs_code,
                                    success,
                                    executed_at,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    if let (Some(id), Some(g)) = (session_id, goal) {
        Some(Session {
            id,
            goal: g,
            original_goal,
            steps,
            context: HashMap::new(),
            created_at,
        })
    } else {
        None
    }
}
