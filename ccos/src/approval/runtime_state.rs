use crate::capability_marketplace::types::ApprovalStatus;
use rtfs::runtime::RuntimeError;
use rtfs::runtime::RuntimeResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Persistent store for the runtime approval state of capabilities.
/// This acts as a projection of the approval workflow, optimized for fast lookups during execution.
pub const DEFAULT_APPROVAL_STORE_PATH: &str = ".ccos/capability_approvals.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeApprovalStore {
    // Map capability ID to approval status
    pub approvals: HashMap<String, ApprovalStatus>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl RuntimeApprovalStore {
    pub fn new() -> Self {
        Self {
            approvals: HashMap::new(),
            path: PathBuf::new(),
        }
    }

    pub async fn load(path: &Path) -> RuntimeResult<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)
                .await
                .map_err(|e| RuntimeError::Generic(e.to_string()))?;
            let approvals = if content.trim().is_empty() {
                HashMap::new()
            } else {
                serde_json::from_str(&content).map_err(|e| {
                    RuntimeError::Generic(format!(
                        "Failed to parse approval store at {}: {}",
                        path.display(),
                        e
                    ))
                })?
            };
            Ok(Self {
                approvals,
                path: path.to_owned(),
            })
        } else {
            Ok(Self {
                approvals: HashMap::new(),
                path: path.to_owned(),
            })
        }
    }

    pub async fn save(&self) -> RuntimeResult<()> {
        let content = serde_json::to_string_pretty(&self.approvals)
            .map_err(|e| RuntimeError::Generic(e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| RuntimeError::Generic(e.to_string()))?;
        }
        fs::write(&self.path, content)
            .await
            .map_err(|e| RuntimeError::Generic(e.to_string()))?;
        Ok(())
    }

    pub fn get_status(&self, id: &str) -> Option<ApprovalStatus> {
        self.approvals.get(id).cloned()
    }

    pub fn set_status(&mut self, id: String, status: ApprovalStatus) {
        self.approvals.insert(id, status);
    }
}
