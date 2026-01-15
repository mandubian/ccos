//! Approval queue for discovered servers

use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use chrono::{DateTime, Utc};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub endpoint: String,
    pub description: Option<String>,
    /// Suggested environment variable name for authentication token (e.g., "GITHUB_MCP_TOKEN")
    /// This is just a reference - the actual token is never stored, only read from env vars at runtime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_env_var: Option<String>,
    /// Path to RTFS capabilities file if tools have been introspected
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities_path: Option<String>,
    /// Alternative endpoints/URLs for the same server (e.g., multiple remotes from MCP registry)
    /// These can be tried during introspection if the primary endpoint fails
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub alternative_endpoints: Vec<String>,
    /// List of capability files associated with this server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "entry")]
pub enum DiscoverySource {
    McpRegistry { name: String },
    ApisGuru { api_name: String },
    NpmRegistry { package: String },
    WebSearch { url: String },
    Manual { user: String },
    OpenApi { url: String },
    HtmlDocs { url: String },
    Mcp { endpoint: String },
    LocalOverride { path: String },
    LocalConfig,
}

impl DiscoverySource {
    pub fn name(&self) -> String {
        match self {
            DiscoverySource::McpRegistry { name } => format!("mcp_registry:{}", name),
            DiscoverySource::ApisGuru { api_name } => format!("apis:{}", api_name),
            DiscoverySource::NpmRegistry { package } => format!("npm:{}", package),
            DiscoverySource::WebSearch { url } => format!("web:{}", url),
            DiscoverySource::Manual { user } => format!("manual:{}", user),
            DiscoverySource::OpenApi { url } => format!("openapi:{}", url),
            DiscoverySource::HtmlDocs { url } => format!("htmldocs:{}", url),
            DiscoverySource::Mcp { endpoint } => format!("mcp:{}", endpoint),
            DiscoverySource::LocalOverride { path } => format!("override:{}", path),
            DiscoverySource::LocalConfig => "local_config".to_string(),
        }
    }
}

pub trait HasId {
    fn id(&self) -> &str;
}

pub trait HasName {
    fn name(&self) -> &str;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDiscovery {
    pub id: String,
    pub source: DiscoverySource,
    pub server_info: ServerInfo,
    pub domain_match: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_assessment: Option<RiskAssessment>,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub requesting_goal: Option<String>,
    /// List of capability files (mirrored from server_info for root-level access)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_files: Option<Vec<String>>,
}

impl HasId for PendingDiscovery {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasName for PendingDiscovery {
    fn name(&self) -> &str {
        &self.server_info.name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalAuthority {
    User(String),
    Constitution { rule_id: String },
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedDiscovery {
    pub id: String,
    pub source: DiscoverySource,
    pub server_info: ServerInfo,
    pub domain_match: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_assessment: Option<RiskAssessment>,
    pub requesting_goal: Option<String>,

    pub approved_at: DateTime<Utc>,
    pub approved_by: ApprovalAuthority,
    pub approval_reason: Option<String>,

    // Capability files (RTFS files for non-MCP servers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_files: Option<Vec<String>>,

    // Version tracking (for future use - enables rollback, audit trail, gradual migration)
    // Default to 1 for existing entries, increment on updates
    #[serde(default = "default_version")]
    pub version: u32,

    // Health tracking
    pub last_successful_call: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub total_calls: u64,
    pub total_errors: u64,
}

impl HasId for ApprovedDiscovery {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasName for ApprovedDiscovery {
    fn name(&self) -> &str {
        &self.server_info.name
    }
}

fn default_version() -> u32 {
    1
}

impl ApprovedDiscovery {
    pub fn error_rate(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.total_errors as f64 / self.total_calls as f64
        }
    }

    pub fn should_dismiss(&self) -> bool {
        self.consecutive_failures > 5 || (self.total_calls > 100 && self.error_rate() > 0.5)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedDiscovery {
    pub id: String,
    pub source: DiscoverySource,
    pub server_info: ServerInfo,
    pub domain_match: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_assessment: Option<RiskAssessment>,
    pub requesting_goal: Option<String>,

    pub rejected_at: DateTime<Utc>,
    pub rejected_by: ApprovalAuthority,
    pub rejection_reason: String,
}

impl HasId for RejectedDiscovery {
    fn id(&self) -> &str {
        &self.id
    }
}

impl HasName for RejectedDiscovery {
    fn name(&self) -> &str {
        &self.server_info.name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalQueueState {
    pub items: Vec<PendingDiscovery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedQueueState {
    pub items: Vec<ApprovedDiscovery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedQueueState {
    pub items: Vec<RejectedDiscovery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutQueueState {
    pub items: Vec<PendingDiscovery>,
}

#[deprecated(
    since = "0.2.0",
    note = "Use `UnifiedApprovalQueue` with `FileApprovalStorage` instead. This legacy queue will be removed in a future release."
)]
pub struct ApprovalQueue {
    base_path: PathBuf,
}

impl ApprovalQueue {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Suggest environment variable name for authentication token based on server name
    ///
    /// Detects server type from name pattern:
    /// - Web APIs (names starting with "web/"): {NAMESPACE}_API_KEY (e.g., "web/api/openweathermap" -> "OPENWEATHER_API_KEY")
    /// - MCP servers (names containing "/mcp" or from MCP registry): {NAMESPACE}_MCP_TOKEN (e.g., "github/github-mcp" -> "GITHUB_MCP_TOKEN")
    /// - APIs.guru: {API_NAME}_API_KEY (e.g., "apis.guru/openweathermap" -> "OPENWEATHERMAP_API_KEY")
    pub fn suggest_auth_env_var(server_name: &str) -> String {
        // Extract the relevant part of the name for token generation
        let (namespace, is_web_api) = if server_name.starts_with("web/") {
            // Web API: "web/api/openweathermap" -> extract "openweathermap"
            let parts: Vec<&str> = server_name.split('/').collect();
            if parts.len() >= 3 {
                (parts[2], true)
            } else if parts.len() == 2 {
                (parts[1], true)
            } else {
                (server_name, true)
            }
        } else if server_name.contains("/mcp") || server_name.ends_with("-mcp") {
            // MCP server: "github/github-mcp" -> extract "github"
            if let Some(slash_pos) = server_name.find('/') {
                (&server_name[..slash_pos], false)
            } else {
                (server_name, false)
            }
        } else if server_name.starts_with("apis.guru/") {
            // APIs.guru: "apis.guru/openweathermap" -> extract "openweathermap"
            let parts: Vec<&str> = server_name.split('/').collect();
            if parts.len() >= 2 {
                (parts[1], true)
            } else {
                (server_name, true)
            }
        } else {
            // Default: extract namespace (part before first slash) and assume MCP
            if let Some(slash_pos) = server_name.find('/') {
                (&server_name[..slash_pos], false)
            } else {
                (server_name, false)
            }
        };

        let normalized = namespace.replace('-', "_").to_uppercase();

        if is_web_api {
            format!("{}_API_KEY", normalized)
        } else {
            format!("{}_MCP_TOKEN", normalized)
        }
    }

    fn pending_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/pending")
    }

    pub fn server_dir(&self) -> PathBuf {
        self.base_path.join("servers")
    }

    fn approved_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/approved")
    }

    fn rejected_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/rejected")
    }

    fn timeout_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/timeout")
    }

    fn ensure_dirs(&self) -> RuntimeResult<()> {
        let paths = [
            self.pending_path(),
            self.approved_path(),
            self.rejected_path(),
            self.timeout_path(),
        ];

        for path in &paths {
            if !path.exists() {
                fs::create_dir_all(path).map_err(|e| {
                    RuntimeError::Generic(format!(
                        "Failed to create directory {}: {}",
                        path.display(),
                        e
                    ))
                })?;
            }
        }
        Ok(())
    }

    pub fn load_item_from_file<T: for<'a> Deserialize<'a>>(
        path: &Path,
    ) -> RuntimeResult<Option<T>> {
        if path.extension().map_or(false, |ext| ext == "json") {
            let content = fs::read_to_string(path).map_err(|e| {
                RuntimeError::Generic(format!("Failed to read file {}: {}", path.display(), e))
            })?;

            match serde_json::from_str::<T>(&content) {
                Ok(item) => Ok(Some(item)),
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to deserialize JSON from {}: {}",
                        path.display(),
                        e
                    );
                    Ok(None)
                }
            }
        } else if path.extension().map_or(false, |ext| ext == "rtfs") {
            let content = fs::read_to_string(path).map_err(|e| {
                RuntimeError::Generic(format!("Failed to read file {}: {}", path.display(), e))
            })?;

            // Parse RTFS
            let ast = rtfs::parser::parse(&content).map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse RTFS {}: {}", path.display(), e))
            })?;

            if let Some(toplevel) = ast.first() {
                if let rtfs::ast::TopLevel::Expression(rtfs::ast::Expression::List(l)) = toplevel {
                    if l.first().map_or(
                        false,
                        |e| matches!(e, rtfs::ast::Expression::Symbol(s) if s.0 == "server"),
                    ) {
                        // Convert list (server :k v ...) to map {:k v ...}
                        let mut map = serde_json::Map::new();
                        let mut iter = l.iter().skip(1);
                        while let Some(k_expr) = iter.next() {
                            if let Some(v_expr) = iter.next() {
                                // Convert Key Expr to String
                                let key = match k_expr {
                                    rtfs::ast::Expression::Literal(
                                        rtfs::ast::Literal::Keyword(k),
                                    ) => Some(k.0.clone()),
                                    rtfs::ast::Expression::Literal(rtfs::ast::Literal::String(
                                        s,
                                    )) => Some(s.clone()),
                                    _ => None,
                                };

                                // Convert Value Expr to JSON
                                if let Some(k) = key {
                                    if let Ok(json_val) = Self::ast_expr_to_json(v_expr) {
                                        map.insert(k, json_val);
                                    }
                                }
                            }
                        }

                        let final_json = serde_json::Value::Object(map);
                        match serde_json::from_value::<T>(final_json) {
                            Ok(item) => return Ok(Some(item)),
                            Err(e) => {
                                eprintln!(
                                    "Warning: Failed to deserialize RTFS object in {}: {}",
                                    path.display(),
                                    e
                                );
                                return Ok(None);
                            }
                        }
                    }
                }
            }

            // If strictly invalid structure, log error or just ignore
            eprintln!(
                "Warning: Invalid or unrecognized RTFS content in {}",
                path.display()
            );
            Ok(None)
        } else {
            Ok(None)
        }
    }

    // Helper to convert AST Expression to JSON
    fn ast_expr_to_json(expr: &rtfs::ast::Expression) -> RuntimeResult<serde_json::Value> {
        match expr {
            rtfs::ast::Expression::Literal(lit) => match lit {
                rtfs::ast::Literal::Integer(i) => Ok(serde_json::Value::Number((*i).into())),
                rtfs::ast::Literal::Float(f) => serde_json::Number::from_f64(*f)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| RuntimeError::Generic("Invalid float".to_string())),
                rtfs::ast::Literal::String(s) => Ok(serde_json::Value::String(s.clone())),
                rtfs::ast::Literal::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
                rtfs::ast::Literal::Nil => Ok(serde_json::Value::Null),
                rtfs::ast::Literal::Keyword(k) => Ok(serde_json::Value::String(k.0.clone())),
                _ => Ok(serde_json::Value::String(format!("{}", lit))),
            },
            rtfs::ast::Expression::List(l) | rtfs::ast::Expression::Vector(l) => {
                let mut arr = Vec::new();
                for item in l {
                    arr.push(Self::ast_expr_to_json(item)?);
                }
                Ok(serde_json::Value::Array(arr))
            }
            rtfs::ast::Expression::Map(m) => {
                let mut map = serde_json::Map::new();
                for (k, v) in m {
                    map.insert(k.to_string(), Self::ast_expr_to_json(v)?);
                }
                Ok(serde_json::Value::Object(map))
            }
            _ => Err(RuntimeError::Generic(
                "Unsupported expression type for JSON conversion".to_string(),
            )),
        }
    }

    pub fn load_from_dir<T: for<'a> Deserialize<'a> + HasId>(
        &self,
        dir: &Path,
    ) -> RuntimeResult<Vec<T>> {
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut items = Vec::new();
        let entries = fs::read_dir(dir).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read directory {}: {}", dir.display(), e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                RuntimeError::Generic(format!("Failed to read directory entry: {}", e))
            })?;
            let path = entry.path();

            if path.is_file() {
                if let Ok(Some(item)) = Self::load_item_from_file::<T>(&path) {
                    items.push(item);
                }
            } else if path.is_dir() {
                // Check for server.rtfs first, then server.json
                let server_rtfs = path.join("server.rtfs");
                if server_rtfs.exists() {
                    if let Ok(Some(item)) = Self::load_item_from_file::<T>(&server_rtfs) {
                        items.push(item);
                        continue;
                    }
                }

                let server_json = path.join("server.json");
                if server_json.exists() {
                    if let Ok(Some(item)) = Self::load_item_from_file::<T>(&server_json) {
                        items.push(item);
                    }
                }
            }
        }

        Ok(items)
    }

    pub fn save_to_dir<T: Serialize + HasId + HasName>(
        &self,
        dir: &Path,
        item: &T,
    ) -> RuntimeResult<()> {
        self.ensure_dirs()?;

        let safe_name = crate::utils::fs::sanitize_filename(item.name());
        let server_dir = dir.join(&safe_name);

        if !server_dir.exists() {
            fs::create_dir_all(&server_dir).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to create server directory {}: {}",
                    server_dir.display(),
                    e
                ))
            })?;
        }

        // Save as RTFS
        let file_path = server_dir.join("server.rtfs");
        let item_json = serde_json::to_value(item).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize item to JSON: {}", e))
        })?;

        let rtfs_val = json_to_rtfs_value(&item_json)?;

        // Convert to (server ...) format if it's a map
        let rtfs_content = if let rtfs::runtime::values::Value::Map(m) = rtfs_val {
            let mut parts = Vec::new();
            // Start with name comment or similar if desired, but for data preservation we just dump fields
            parts.push("(server".to_string());

            // Sort keys for deterministic output
            let mut entries: Vec<_> = m.into_iter().collect();
            entries.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));

            for (k, v) in entries {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => kw.0,
                    rtfs::ast::MapKey::String(s) => format!(":{}", s.replace(" ", "_")), // Force keyword style
                    _ => format!(":{}", k),
                };

                // Helper to format value nicely
                let val_str = format!("{}", v);
                parts.push(format!("  {} {}", key_str, val_str));
            }
            parts.push(")".to_string());
            parts.join("\n")
        } else {
            // Fallback
            format!("{}", rtfs_val)
        };

        // Add header
        let content = format!(";; Server Manifest: {}\n{}\n", item.name(), rtfs_content);

        fs::write(&file_path, content).map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to write file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        // Remove legacy server.json if it exists
        let legacy_path = server_dir.join("server.json");
        if legacy_path.exists() {
            let _ = fs::remove_file(legacy_path);
        }

        Ok(())
    }

    fn remove_from_dir<T: HasId + HasName>(&self, dir: &Path, item: &T) -> RuntimeResult<()> {
        let safe_name = crate::utils::fs::sanitize_filename(item.name());
        let server_dir = dir.join(&safe_name);

        if server_dir.exists() {
            fs::remove_dir_all(&server_dir).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to remove directory {}: {}",
                    server_dir.display(),
                    e
                ))
            })?;
        }

        Ok(())
    }

    fn migrate_legacy_file<T: for<'a> Deserialize<'a> + Serialize + HasId + HasName + Clone>(
        &self,
        legacy_path: &Path,
        target_dir: &Path,
        wrapper_fn: fn(Vec<T>) -> T, // Dummy wrapper not needed, we need to extract items
    ) -> RuntimeResult<()> {
        if legacy_path.exists() && legacy_path.is_file() {
            println!(
                "Migrating legacy file: {} -> {}",
                legacy_path.display(),
                target_dir.display()
            );

            let content = fs::read_to_string(legacy_path).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to read legacy file {}: {}",
                    legacy_path.display(),
                    e
                ))
            })?;

            // We need to parse the wrapper struct (e.g. ApprovalQueueState) to get items
            // This is tricky with generics. Instead, let's just parse as Value and extract items
            let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                RuntimeError::Generic(format!("Failed to parse legacy JSON: {}", e))
            })?;

            if let Some(items_array) = json.get("items").and_then(|v| v.as_array()) {
                for item_val in items_array {
                    let item: T = serde_json::from_value(item_val.clone()).map_err(|e| {
                        RuntimeError::Generic(format!(
                            "Failed to parse item from legacy JSON: {}",
                            e
                        ))
                    })?;

                    self.save_to_dir(target_dir, &item)?;
                }
            }

            // Rename legacy file to .bak
            let bak_path = legacy_path.with_extension("json.bak");
            fs::rename(legacy_path, &bak_path).map_err(|e| {
                RuntimeError::Generic(format!("Failed to rename legacy file: {}", e))
            })?;
        }
        Ok(())
    }

    fn load_pending(&self) -> RuntimeResult<ApprovalQueueState> {
        // Migration check
        let legacy_path = self.base_path.join("capabilities/servers/pending.json");
        self.migrate_legacy_file::<PendingDiscovery>(
            &legacy_path,
            &self.pending_path(),
            |_| unimplemented!(),
        )?;

        let items = self.load_from_dir(&self.pending_path())?;
        Ok(ApprovalQueueState { items })
    }

    fn save_pending(&self, state: &ApprovalQueueState) -> RuntimeResult<()> {
        // In the new model, we save items individually.
        // This method receives the full state, so we should save all items.
        // However, this is inefficient if we just added one.
        // But for now, to keep the API compatible, we'll iterate and save.
        // Optimally, we should change the API to save_pending_item(item).

        for item in &state.items {
            self.save_to_dir(&self.pending_path(), item)?;
        }
        Ok(())
    }

    fn load_approved(&self) -> RuntimeResult<ApprovedQueueState> {
        let legacy_path = self.base_path.join("capabilities/servers/approved.json");
        self.migrate_legacy_file::<ApprovedDiscovery>(
            &legacy_path,
            &self.approved_path(),
            |_| unimplemented!(),
        )?;

        let items = self.load_from_dir(&self.approved_path())?;
        Ok(ApprovedQueueState { items })
    }

    fn save_approved(&self, state: &ApprovedQueueState) -> RuntimeResult<()> {
        for item in &state.items {
            self.save_to_dir(&self.approved_path(), item)?;
        }
        Ok(())
    }

    fn load_rejected(&self) -> RuntimeResult<RejectedQueueState> {
        let legacy_path = self.base_path.join("capabilities/servers/rejected.json");
        self.migrate_legacy_file::<RejectedDiscovery>(
            &legacy_path,
            &self.rejected_path(),
            |_| unimplemented!(),
        )?;

        let items = self.load_from_dir(&self.rejected_path())?;
        Ok(RejectedQueueState { items })
    }

    fn save_rejected(&self, state: &RejectedQueueState) -> RuntimeResult<()> {
        for item in &state.items {
            self.save_to_dir(&self.rejected_path(), item)?;
        }
        Ok(())
    }

    fn load_timeout(&self) -> RuntimeResult<TimeoutQueueState> {
        let legacy_path = self.base_path.join("capabilities/servers/timeout.json");
        self.migrate_legacy_file::<PendingDiscovery>(
            &legacy_path,
            &self.timeout_path(),
            |_| unimplemented!(),
        )?;

        let items = self.load_from_dir(&self.timeout_path())?;
        Ok(TimeoutQueueState { items })
    }

    fn save_timeout(&self, state: &TimeoutQueueState) -> RuntimeResult<()> {
        for item in &state.items {
            self.save_to_dir(&self.timeout_path(), item)?;
        }
        Ok(())
    }

    pub fn check_timeouts(&self) -> RuntimeResult<Vec<String>> {
        let mut pending_state = self.load_pending()?;
        let now = Utc::now();
        let mut timed_out_ids = Vec::new();
        let mut remaining_items = Vec::new();
        let mut timed_out_items = Vec::new();

        for item in pending_state.items {
            if item.expires_at < now {
                timed_out_ids.push(item.id.clone());
                timed_out_items.push(item);
            } else {
                remaining_items.push(item);
            }
        }

        if !timed_out_items.is_empty() {
            pending_state.items = remaining_items;
            self.save_pending(&pending_state)?;

            let mut timeout_state = self.load_timeout()?;
            timeout_state.items.extend(timed_out_items);
            self.save_timeout(&timeout_state)?;
        }

        Ok(timed_out_ids)
    }

    /// Add a discovery to pending queue
    /// Returns the ID of the entry (existing ID if duplicate, new ID if new)
    pub fn add(&self, discovery: PendingDiscovery) -> RuntimeResult<String> {
        let mut state = self.load_pending()?;

        // Check if server is already approved - if so, move it from approved to pending
        let mut approved_state = self.load_approved()?;
        if let Some(approved_pos) = approved_state.items.iter().position(|item| {
            item.server_info.name == discovery.server_info.name
                || (!discovery.server_info.endpoint.is_empty()
                    && item.server_info.endpoint == discovery.server_info.endpoint)
        }) {
            let approved_item = approved_state.items.remove(approved_pos);
            self.save_approved(&approved_state)?;

            // Move capability files from approved/ to pending/
            let server_id = approved_item
                .server_info
                .name
                .to_lowercase()
                .replace(" ", "_")
                .replace("/", "_");

            let approved_dir =
                std::path::Path::new("capabilities/servers/approved").join(&server_id);
            let pending_dir = std::path::Path::new("capabilities/servers/pending").join(&server_id);

            if approved_dir.exists() {
                // Create pending directory if needed
                if let Some(parent) = pending_dir.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                // Remove existing pending dir if it exists
                if pending_dir.exists() {
                    let _ = std::fs::remove_dir_all(&pending_dir);
                }

                // Move from approved to pending
                if let Err(e) = std::fs::rename(&approved_dir, &pending_dir) {
                    // Log but don't fail - capabilities can be re-introspected
                    eprintln!(
                        "Warning: Failed to move capabilities from approved to pending: {}",
                        e
                    );
                }
            }

            // Update discovery with capabilities_path pointing to pending location
            // IMPORTANT: Preserve the approved item's ID to maintain continuity
            let mut updated_discovery = discovery.clone();
            updated_discovery.id = approved_item.id.clone(); // Keep the original ID
            if let Some(ref old_path) = approved_item.server_info.capabilities_path {
                // Update path from approved to pending
                let new_path = old_path.replace("/approved/", "/pending/");
                updated_discovery.server_info.capabilities_path = Some(new_path);
            } else {
                // Set default path
                let default_path = format!(
                    "capabilities/servers/pending/{}/capabilities.rtfs",
                    server_id
                );
                if std::path::Path::new(&default_path).exists() {
                    updated_discovery.server_info.capabilities_path = Some(default_path);
                }
            }

            // Preserve other metadata from approved item (timestamps, etc.)
            // But update with new discovery information (source, domain_match, etc.)
            // The requested_at and expires_at are reset for the new pending entry

            // Add to pending with preserved ID
            let id = updated_discovery.id.clone();
            state.items.push(updated_discovery);
            self.save_pending(&state)?;
            return Ok(id);
        }

        // Check for duplicates in pending: same server name or endpoint
        let is_duplicate = state.items.iter().any(|item| {
            item.server_info.name == discovery.server_info.name
                || (!discovery.server_info.endpoint.is_empty()
                    && item.server_info.endpoint == discovery.server_info.endpoint)
        });

        if is_duplicate {
            // Update existing entry instead of adding duplicate
            if let Some(existing_pos) = state.items.iter().position(|item| {
                item.server_info.name == discovery.server_info.name
                    || (!discovery.server_info.endpoint.is_empty()
                        && item.server_info.endpoint == discovery.server_info.endpoint)
            }) {
                // Merge: keep existing ID and timestamps, update other fields
                let existing = &mut state.items[existing_pos];
                let existing_id = existing.id.clone();

                // Update fields that might have changed
                existing.source = discovery.source.clone();
                existing.domain_match = discovery.domain_match.clone();
                existing.risk_assessment = discovery.risk_assessment.clone();
                existing.requesting_goal = discovery.requesting_goal.clone();

                // Update server info (merge capabilities_path if new one exists)
                existing.server_info.description = discovery.server_info.description.clone();
                existing.server_info.auth_env_var = discovery.server_info.auth_env_var.clone();
                // Merge alternative_endpoints (combine unique endpoints)
                let mut all_endpoints = existing.server_info.alternative_endpoints.clone();
                all_endpoints.extend(discovery.server_info.alternative_endpoints.clone());
                all_endpoints.sort();
                all_endpoints.dedup();
                existing.server_info.alternative_endpoints = all_endpoints;

                if discovery.server_info.capabilities_path.is_some() {
                    existing.server_info.capabilities_path =
                        discovery.server_info.capabilities_path.clone();
                }

                // Extend expiration if new one is later
                if discovery.expires_at > existing.expires_at {
                    existing.expires_at = discovery.expires_at;
                }

                self.save_pending(&state)?;
                return Ok(existing_id);
            }
        }

        // New server, add it
        let id = discovery.id.clone();
        state.items.push(discovery);
        self.save_pending(&state)?;
        Ok(id)
    }

    pub fn remove_pending(&self, id: &str) -> RuntimeResult<Option<PendingDiscovery>> {
        let mut state = self.load_pending()?;
        if let Some(pos) = state.items.iter().position(|item| item.id == id) {
            let removed = state.items.remove(pos);
            self.remove_from_dir(&self.pending_path(), &removed)?;
            // No need to save_pending() full state as we removed the file
            Ok(Some(removed))
        } else {
            Ok(None)
        }
    }

    pub fn list_pending(&self) -> RuntimeResult<Vec<PendingDiscovery>> {
        self.check_timeouts()?;
        Ok(self.load_pending()?.items)
    }

    pub fn list_timeouts(&self) -> RuntimeResult<Vec<PendingDiscovery>> {
        Ok(self.load_timeout()?.items)
    }

    pub fn get_pending(&self, id: &str) -> RuntimeResult<Option<PendingDiscovery>> {
        let state = self.load_pending()?;
        Ok(state.items.into_iter().find(|item| item.id == id))
    }

    /// Update a pending entry in place (without removing/re-adding, preserving directory contents)
    pub fn update_pending(&self, updated: &PendingDiscovery) -> RuntimeResult<()> {
        self.ensure_dirs()?;
        self.save_to_dir(&self.pending_path(), updated)
    }

    /// Check if a pending item would conflict with an existing approved server
    /// Returns the existing approved server if there's a conflict
    pub fn check_approval_conflict(
        &self,
        pending_id: &str,
    ) -> RuntimeResult<Option<ApprovedDiscovery>> {
        let pending = self.get_pending(pending_id)?;
        if let Some(pending_item) = pending {
            let approved_state = self.load_approved()?;
            let conflict = approved_state.items.into_iter().find(|existing| {
                existing.server_info.name == pending_item.server_info.name
                    || (!pending_item.server_info.endpoint.is_empty()
                        && existing.server_info.endpoint == pending_item.server_info.endpoint)
            });
            Ok(conflict)
        } else {
            Ok(None)
        }
    }

    pub fn list_approved(&self) -> RuntimeResult<Vec<ApprovedDiscovery>> {
        Ok(self.load_approved()?.items)
    }

    pub fn get_approved(&self, id: &str) -> RuntimeResult<Option<ApprovedDiscovery>> {
        let state = self.load_approved()?;
        Ok(state.items.into_iter().find(|item| item.id == id))
    }

    pub fn list_rejected(&self) -> RuntimeResult<Vec<RejectedDiscovery>> {
        Ok(self.load_rejected()?.items)
    }

    /// Update capability_files for an approved server
    pub fn update_capability_files(
        &self,
        server_id: &str,
        files: Vec<String>,
    ) -> RuntimeResult<()> {
        let mut approved_state = self.load_approved()?;
        if let Some(item) = approved_state
            .items
            .iter_mut()
            .find(|item| item.id == server_id)
        {
            item.capability_files = if files.is_empty() { None } else { Some(files) };
            self.save_approved(&approved_state)?;
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Approved server not found: {}",
                server_id
            )))
        }
    }

    /// Add new capability files to an approved server (merge with existing)
    pub fn add_capability_files_to_approved(
        &self,
        server_endpoint: &str,
        new_files: Vec<String>,
    ) -> RuntimeResult<()> {
        let mut approved_state = self.load_approved()?;
        if let Some(item) = approved_state
            .items
            .iter_mut()
            .find(|item| item.server_info.endpoint == server_endpoint)
        {
            let mut existing_files = item.capability_files.clone().unwrap_or_default();
            existing_files.extend(new_files);
            item.capability_files = if existing_files.is_empty() {
                None
            } else {
                Some(existing_files)
            };
            self.save_approved(&approved_state)?;
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Approved server not found for endpoint: {}",
                server_endpoint
            )))
        }
    }

    pub fn approve(&self, id: &str, reason: Option<String>) -> RuntimeResult<()> {
        let mut pending_state = self.load_pending()?;
        if let Some(pos) = pending_state.items.iter().position(|item| item.id == id) {
            let mut item = pending_state.items.remove(pos);

            // IMPORTANT: Move capability files BEFORE removing the pending directory
            // Otherwise remove_from_dir will delete the entire directory including capabilities.rtfs

            let pending_dir = self.pending_path();
            let approved_dir = self.approved_path();
            std::fs::create_dir_all(&approved_dir).map_err(|e| {
                RuntimeError::Generic(format!("Failed to create approved directory: {}", e))
            })?;

            let server_id = crate::utils::fs::sanitize_filename(&item.server_info.name);
            let pending_server_dir = pending_dir.join(&server_id);
            let approved_server_dir = approved_dir.join(&server_id);

            let mut capability_files = Vec::new();

            // Move capability files if they exist
            // Check both the directory and the capabilities_path field
            let should_move =
                if let Some(ref capabilities_path) = item.server_info.capabilities_path {
                    // If capabilities_path is set, check if that file exists (absolute or relative)
                    let path = std::path::Path::new(capabilities_path);
                    let path = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        self.base_path.join(path)
                    };
                    path.exists()
                } else {
                    // Otherwise check if the server directory exists
                    pending_server_dir.exists()
                };

            if should_move {
                // Determine the source directory
                // capabilities_path is like "capabilities/servers/pending/github_github-mcp/capabilities.rtfs"
                // We want to move the entire directory "capabilities/servers/pending/github_github-mcp"
                let source_dir: std::path::PathBuf =
                    if let Some(ref capabilities_path) = item.server_info.capabilities_path {
                        // Extract directory from capabilities_path (absolute or relative)
                        let path = std::path::Path::new(capabilities_path);
                        let absolute = if path.is_absolute() {
                            path.to_path_buf()
                        } else {
                            self.base_path.join(path)
                        };
                        absolute
                            .parent()
                            .ok_or_else(|| {
                                RuntimeError::Generic(
                                    "Invalid capabilities_path: no parent directory".to_string(),
                                )
                            })?
                            .to_path_buf()
                    } else {
                        pending_server_dir.clone()
                    };

                if source_dir.exists() {
                    if approved_server_dir.exists() {
                        // Remove existing approved directory to replace it
                        std::fs::remove_dir_all(&approved_server_dir).map_err(|e| {
                            RuntimeError::Generic(format!(
                                "Failed to remove existing approved directory: {}",
                                e
                            ))
                        })?;
                    }

                    // Move the entire directory (BEFORE removing from pending)
                    std::fs::rename(&source_dir, &approved_server_dir).map_err(|e| {
                        RuntimeError::Generic(format!(
                            "Failed to move capability directory from {} to {}: {}",
                            source_dir.display(),
                            approved_server_dir.display(),
                            e
                        ))
                    })?;

                    // Collect all RTFS file paths from the approved directory
                    if approved_server_dir.exists() {
                        if let Ok(entries) = std::fs::read_dir(&approved_server_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_file()
                                    && path.extension().map_or(false, |ext| ext == "rtfs")
                                {
                                    if let Ok(rel_path) =
                                        path.strip_prefix("capabilities/servers/approved")
                                    {
                                        capability_files
                                            .push(rel_path.to_string_lossy().to_string());
                                    }
                                } else if path.is_dir() {
                                    // Recursively find RTFS files in subdirectories
                                    if let Ok(sub_entries) = std::fs::read_dir(&path) {
                                        for sub_entry in sub_entries.flatten() {
                                            let sub_path = sub_entry.path();
                                            if sub_path.is_file()
                                                && sub_path
                                                    .extension()
                                                    .map_or(false, |ext| ext == "rtfs")
                                            {
                                                if let Ok(rel_path) = sub_path
                                                    .strip_prefix("capabilities/servers/approved")
                                                {
                                                    capability_files.push(
                                                        rel_path.to_string_lossy().to_string(),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Update capabilities_path in server_info to point to approved location
                    if let Some(ref old_path) = item.server_info.capabilities_path {
                        // Replace "pending" with "approved" in the path
                        let new_path = old_path.replace("/pending/", "/approved/");
                        item.server_info.capabilities_path = Some(new_path);
                    } else {
                        // If no capabilities_path was set, set it now based on the moved location
                        let default_path = format!(
                            "capabilities/servers/approved/{}/capabilities.rtfs",
                            server_id
                        );
                        if std::path::Path::new(&default_path).exists() {
                            item.server_info.capabilities_path = Some(default_path);
                        }
                    }
                }
            }

            // NOW remove from pending (after moving capabilities)
            // Since we moved the directory, remove_from_dir will just try to remove a non-existent directory, which is fine
            let _ = self.remove_from_dir(&self.pending_path(), &item);

            let approved = ApprovedDiscovery {
                id: item.id,
                source: item.source,
                server_info: item.server_info,
                domain_match: item.domain_match,
                risk_assessment: item.risk_assessment,
                requesting_goal: item.requesting_goal,
                approved_at: Utc::now(),
                approved_by: ApprovalAuthority::User("cli_user".to_string()), // Default for CLI
                approval_reason: reason,
                capability_files: if capability_files.is_empty() {
                    None
                } else {
                    Some(capability_files)
                },
                version: 1, // Initial version
                last_successful_call: None,
                consecutive_failures: 0,
                total_calls: 0,
                total_errors: 0,
            };

            // Check if server already exists in approved (by name or endpoint)
            // We need to load approved state again because it might have changed
            // But actually we just want to save the new one.
            // If it exists, save_to_dir will overwrite the server.json file
            // But we should check to preserve stats if possible.

            let mut approved_state = self.load_approved()?;
            let existing_pos = approved_state.items.iter().position(|existing| {
                existing.server_info.name == approved.server_info.name
                    || (!approved.server_info.endpoint.is_empty()
                        && existing.server_info.endpoint == approved.server_info.endpoint)
            });

            let final_approved = if let Some(pos) = existing_pos {
                // Update existing entry - merge capabilities, keep stats
                let mut existing = approved_state.items[pos].clone();

                // Increment version on update
                existing.version += 1;

                // Update server info
                existing.server_info = approved.server_info;
                existing.source = approved.source;
                existing.domain_match = approved.domain_match;
                existing.risk_assessment = approved.risk_assessment;
                existing.requesting_goal = approved.requesting_goal;
                existing.approved_at = approved.approved_at; // Update approval time
                existing.approval_reason = approved.approval_reason;

                // Merge capability files (add new ones, keep existing)
                if let Some(new_files) = approved.capability_files {
                    let mut all_files = existing.capability_files.clone().unwrap_or_default();
                    for file in new_files {
                        if !all_files.contains(&file) {
                            all_files.push(file);
                        }
                    }
                    existing.capability_files = Some(all_files);
                }

                // Keep usage stats from existing entry
                existing
            } else {
                // New server
                approved
            };

            self.save_to_dir(&self.approved_path(), &final_approved)?;

            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Discovery not found: {}",
                id
            )))
        }
    }

    pub fn reject(&self, id: &str, reason: String) -> RuntimeResult<()> {
        let mut pending_state = self.load_pending()?;
        if let Some(pos) = pending_state.items.iter().position(|item| item.id == id) {
            let item = pending_state.items.remove(pos);
            self.remove_from_dir(&self.pending_path(), &item)?;

            let rejected = RejectedDiscovery {
                id: item.id,
                source: item.source,
                server_info: item.server_info,
                domain_match: item.domain_match,
                risk_assessment: item.risk_assessment,
                requesting_goal: item.requesting_goal,
                rejected_at: Utc::now(),
                rejected_by: ApprovalAuthority::User("cli_user".to_string()), // Default for CLI
                rejection_reason: reason,
            };

            self.save_to_dir(&self.rejected_path(), &rejected)?;

            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Discovery not found: {}",
                id
            )))
        }
    }

    pub fn dismiss_server(&self, id: &str, reason: String) -> RuntimeResult<()> {
        let mut approved_state = self.load_approved()?;
        if let Some(pos) = approved_state
            .items
            .iter()
            .position(|item| item.id == id || item.server_info.name == id)
        {
            let item = approved_state.items.remove(pos);
            self.remove_from_dir(&self.approved_path(), &item)?;

            let rejected = RejectedDiscovery {
                id: item.id,
                source: item.source,
                server_info: item.server_info,
                domain_match: item.domain_match,
                risk_assessment: item.risk_assessment,
                requesting_goal: item.requesting_goal,
                rejected_at: Utc::now(),
                rejected_by: ApprovalAuthority::User("cli_user".to_string()),
                rejection_reason: format!("Dismissed: {}", reason),
            };

            self.save_to_dir(&self.rejected_path(), &rejected)?;

            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Server not found in approved list: {}",
                id
            )))
        }
    }

    pub fn retry_server(&self, id: &str) -> RuntimeResult<()> {
        let mut rejected_state = self.load_rejected()?;
        if let Some(pos) = rejected_state
            .items
            .iter()
            .position(|item| item.id == id || item.server_info.name == id)
        {
            let item = rejected_state.items.remove(pos);
            self.remove_from_dir(&self.rejected_path(), &item)?;

            let approved = ApprovedDiscovery {
                id: item.id,
                source: item.source,
                server_info: item.server_info,
                domain_match: item.domain_match,
                risk_assessment: item.risk_assessment,
                requesting_goal: item.requesting_goal,
                approved_at: Utc::now(),
                approved_by: ApprovalAuthority::User("cli_user".to_string()),
                approval_reason: Some("Manually retried".to_string()),
                capability_files: None, // Retried servers don't have capability files yet
                version: 1,             // Initial version for retried servers
                last_successful_call: None,
                consecutive_failures: 0,
                total_calls: 0,
                total_errors: 0,
            };

            self.save_to_dir(&self.approved_path(), &approved)?;

            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Server not found in rejected/dismissed list: {}",
                id
            )))
        }
    }
}
