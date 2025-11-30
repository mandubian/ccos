//! Approval queue for discovered servers

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;
use rtfs::runtime::error::{RuntimeResult, RuntimeError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub endpoint: String,
    pub description: Option<String>,
    /// Suggested environment variable name for authentication token (e.g., "GITHUB_MCP_TOKEN")
    /// This is just a reference - the actual token is never stored, only read from env vars at runtime
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_env_var: Option<String>,
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
    WebSearch { url: String },
    Manual { user: String },
    LocalOverride { path: String },
}

impl DiscoverySource {
    pub fn name(&self) -> String {
        match self {
            DiscoverySource::McpRegistry { name } => format!("mcp:{}", name),
            DiscoverySource::ApisGuru { api_name } => format!("apis:{}", api_name),
            DiscoverySource::WebSearch { url } => format!("web:{}", url),
            DiscoverySource::Manual { user } => format!("manual:{}", user),
            DiscoverySource::LocalOverride { path } => format!("override:{}", path),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDiscovery {
    pub id: String,
    pub source: DiscoverySource,
    pub server_info: ServerInfo,
    pub domain_match: Vec<String>,
    pub risk_assessment: RiskAssessment,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub requesting_goal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub domain_match: Vec<String>,
    pub risk_assessment: RiskAssessment,
    pub requesting_goal: Option<String>,
    
    pub approved_at: DateTime<Utc>,
    pub approved_by: ApprovalAuthority,
    pub approval_reason: Option<String>,
    
    // Health tracking
    pub last_successful_call: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub total_calls: u64,
    pub total_errors: u64,
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
    pub domain_match: Vec<String>,
    pub risk_assessment: RiskAssessment,
    pub requesting_goal: Option<String>,
    
    pub rejected_at: DateTime<Utc>,
    pub rejected_by: ApprovalAuthority,
    pub rejection_reason: String,
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

pub struct ApprovalQueue {
    base_path: PathBuf,
}

impl ApprovalQueue {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    /// Suggest environment variable name for MCP authentication token based on server name
    /// 
    /// Pattern: {NAMESPACE}_MCP_TOKEN (e.g., "github/github-mcp" -> "GITHUB_MCP_TOKEN")
    /// For GitHub servers, also suggests legacy names: GITHUB_PAT, GITHUB_TOKEN
    pub fn suggest_auth_env_var(server_name: &str) -> String {
        let namespace = if let Some(slash_pos) = server_name.find('/') {
            &server_name[..slash_pos]
        } else {
            server_name
        };

        let normalized = namespace.replace('-', "_").to_uppercase();
        format!("{}_MCP_TOKEN", normalized)
    }

    fn pending_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/pending.json")
    }

    fn approved_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/approved.json")
    }

    fn rejected_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/rejected.json")
    }

    fn timeout_path(&self) -> PathBuf {
        self.base_path.join("capabilities/servers/timeout.json")
    }

    fn ensure_dirs(&self) -> RuntimeResult<()> {
        if let Some(parent) = self.pending_path().parent() {
            fs::create_dir_all(parent).map_err(|e| {
                RuntimeError::Generic(format!("Failed to create directories: {}", e))
            })?;
        }
        Ok(())
    }

    fn load_pending(&self) -> RuntimeResult<ApprovalQueueState> {
        let path = self.pending_path();
        if !path.exists() {
            return Ok(ApprovalQueueState { items: vec![] });
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read pending queue: {}", e))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse pending queue: {}", e))
        })
    }

    fn save_pending(&self, state: &ApprovalQueueState) -> RuntimeResult<()> {
        self.ensure_dirs()?;
        let content = serde_json::to_string_pretty(state).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize pending queue: {}", e))
        })?;

        fs::write(self.pending_path(), content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write pending queue: {}", e))
        })
    }

    fn load_approved(&self) -> RuntimeResult<ApprovedQueueState> {
        let path = self.approved_path();
        if !path.exists() {
            return Ok(ApprovedQueueState { items: vec![] });
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read approved queue: {}", e))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse approved queue: {}", e))
        })
    }

    fn save_approved(&self, state: &ApprovedQueueState) -> RuntimeResult<()> {
        self.ensure_dirs()?;
        let content = serde_json::to_string_pretty(state).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize approved queue: {}", e))
        })?;

        fs::write(self.approved_path(), content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write approved queue: {}", e))
        })
    }

    fn load_rejected(&self) -> RuntimeResult<RejectedQueueState> {
        let path = self.rejected_path();
        if !path.exists() {
            return Ok(RejectedQueueState { items: vec![] });
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read rejected queue: {}", e))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse rejected queue: {}", e))
        })
    }

    fn save_rejected(&self, state: &RejectedQueueState) -> RuntimeResult<()> {
        self.ensure_dirs()?;
        let content = serde_json::to_string_pretty(state).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize rejected queue: {}", e))
        })?;

        fs::write(self.rejected_path(), content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write rejected queue: {}", e))
        })
    }

    fn load_timeout(&self) -> RuntimeResult<TimeoutQueueState> {
        let path = self.timeout_path();
        if !path.exists() {
            return Ok(TimeoutQueueState { items: vec![] });
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read timeout queue: {}", e))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse timeout queue: {}", e))
        })
    }

    fn save_timeout(&self, state: &TimeoutQueueState) -> RuntimeResult<()> {
        self.ensure_dirs()?;
        let content = serde_json::to_string_pretty(state).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize timeout queue: {}", e))
        })?;

        fs::write(self.timeout_path(), content).map_err(|e| {
            RuntimeError::Generic(format!("Failed to write timeout queue: {}", e))
        })
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

    pub fn add(&self, discovery: PendingDiscovery) -> RuntimeResult<()> {
        let mut state = self.load_pending()?;
        state.items.push(discovery);
        self.save_pending(&state)
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

    pub fn approve(&self, id: &str, reason: Option<String>) -> RuntimeResult<()> {
        let mut pending_state = self.load_pending()?;
        if let Some(pos) = pending_state.items.iter().position(|item| item.id == id) {
            let item = pending_state.items.remove(pos);
            self.save_pending(&pending_state)?;
            
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
                last_successful_call: None,
                consecutive_failures: 0,
                total_calls: 0,
                total_errors: 0,
            };
            
            let mut approved_state = self.load_approved()?;
            approved_state.items.push(approved);
            self.save_approved(&approved_state)?;
            
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!("Discovery not found: {}", id)))
        }
    }

    pub fn reject(&self, id: &str, reason: String) -> RuntimeResult<()> {
        let mut pending_state = self.load_pending()?;
        if let Some(pos) = pending_state.items.iter().position(|item| item.id == id) {
            let item = pending_state.items.remove(pos);
            self.save_pending(&pending_state)?;
            
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
            
            let mut rejected_state = self.load_rejected()?;
            rejected_state.items.push(rejected);
            self.save_rejected(&rejected_state)?;
            
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!("Discovery not found: {}", id)))
        }
    }

    pub fn dismiss_server(&self, id: &str, reason: String) -> RuntimeResult<()> {
        let mut approved_state = self.load_approved()?;
        if let Some(pos) = approved_state.items.iter().position(|item| item.id == id || item.server_info.name == id) {
            let item = approved_state.items.remove(pos);
            self.save_approved(&approved_state)?;
            
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
            
            let mut rejected_state = self.load_rejected()?;
            rejected_state.items.push(rejected);
            self.save_rejected(&rejected_state)?;
            
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!("Server not found in approved list: {}", id)))
        }
    }

    pub fn retry_server(&self, id: &str) -> RuntimeResult<()> {
        let mut rejected_state = self.load_rejected()?;
        if let Some(pos) = rejected_state.items.iter().position(|item| item.id == id || item.server_info.name == id) {
            let item = rejected_state.items.remove(pos);
            self.save_rejected(&rejected_state)?;
            
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
                last_successful_call: None,
                consecutive_failures: 0,
                total_calls: 0,
                total_errors: 0,
            };
            
            let mut approved_state = self.load_approved()?;
            approved_state.items.push(approved);
            self.save_approved(&approved_state)?;
            
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!("Server not found in rejected/dismissed list: {}", id)))
        }
    }
}
