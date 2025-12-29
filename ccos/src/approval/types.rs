//! Generic approval system types
//!
//! This module defines generic types for approval requests that can be used
//! across different domains (server discovery, effect approvals, synthesis, LLM prompts).

use chrono::{DateTime, Utc};
use rtfs::runtime::error::RuntimeResult;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

// Re-export existing types that are generic enough
pub use super::queue::{ApprovalAuthority, DiscoverySource, RiskAssessment, RiskLevel, ServerInfo};

/// Category of approval request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ApprovalCategory {
    /// Server discovery approval - includes full server info
    ServerDiscovery {
        /// Source of the discovery
        source: DiscoverySource,
        /// Full server information
        server_info: ServerInfo,
        /// Domain keywords this server matched
        domain_match: Vec<String>,
        /// Goal that requested this server
        requesting_goal: Option<String>,
        /// Health tracking (for approved servers)
        #[serde(default)]
        health: Option<ServerHealthTracking>,
        /// Capability files (for approved servers)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability_files: Option<Vec<String>>,
    },
    /// Effect-based capability execution approval
    EffectApproval {
        capability_id: String,
        effects: Vec<String>,
        intent_description: String,
    },
    /// Synthesized capability approval
    SynthesisApproval {
        capability_id: String,
        generated_code: String,
        is_pure: bool,
    },
    /// LLM prompt approval (for risky prompts)
    LlmPromptApproval {
        prompt: String,
        risk_reasons: Vec<String>,
    },
}

/// Health tracking for approved servers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerHealthTracking {
    pub last_successful_call: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub total_calls: u64,
    pub total_errors: u64,
    pub version: u32,
}

impl ServerHealthTracking {
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

impl fmt::Display for ApprovalCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApprovalCategory::ServerDiscovery { server_info, .. } => {
                write!(f, "ServerDiscovery({})", server_info.name)
            }
            ApprovalCategory::EffectApproval { capability_id, .. } => {
                write!(f, "EffectApproval({})", capability_id)
            }
            ApprovalCategory::SynthesisApproval { capability_id, .. } => {
                write!(f, "SynthesisApproval({})", capability_id)
            }
            ApprovalCategory::LlmPromptApproval { .. } => {
                write!(f, "LlmPromptApproval")
            }
        }
    }
}

/// Status of an approval request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status")]
pub enum ApprovalStatus {
    Pending,
    Approved {
        by: ApprovalAuthority,
        reason: Option<String>,
        at: DateTime<Utc>,
    },
    Rejected {
        by: ApprovalAuthority,
        reason: String,
        at: DateTime<Utc>,
    },
    Expired {
        at: DateTime<Utc>,
    },
}

impl ApprovalStatus {
    pub fn is_pending(&self) -> bool {
        matches!(self, ApprovalStatus::Pending)
    }

    pub fn is_resolved(&self) -> bool {
        !self.is_pending()
    }
}

/// Generic approval request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub category: ApprovalCategory,
    pub risk_assessment: RiskAssessment,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: ApprovalStatus,
    /// Optional context about why this approval was requested
    pub context: Option<String>,
}

impl ApprovalRequest {
    pub fn new(
        category: ApprovalCategory,
        risk_assessment: RiskAssessment,
        expires_in_hours: i64,
        context: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            category,
            risk_assessment,
            requested_at: now,
            expires_at: now + chrono::Duration::hours(expires_in_hours),
            status: ApprovalStatus::Pending,
            context,
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn approve(&mut self, by: ApprovalAuthority, reason: Option<String>) {
        self.status = ApprovalStatus::Approved {
            by,
            reason,
            at: Utc::now(),
        };
    }

    pub fn reject(&mut self, by: ApprovalAuthority, reason: String) {
        self.status = ApprovalStatus::Rejected {
            by,
            reason,
            at: Utc::now(),
        };
    }

    pub fn expire(&mut self) {
        self.status = ApprovalStatus::Expired { at: Utc::now() };
    }
}

/// Filter for querying approval requests
#[derive(Debug, Clone, Default)]
pub struct ApprovalFilter {
    pub category_type: Option<String>, // "ServerDiscovery", "EffectApproval", etc.
    pub status_pending: Option<bool>,
    pub limit: Option<usize>,
}

impl ApprovalFilter {
    pub fn pending() -> Self {
        Self {
            status_pending: Some(true),
            ..Default::default()
        }
    }

    pub fn for_category(category_type: &str) -> Self {
        Self {
            category_type: Some(category_type.to_string()),
            ..Default::default()
        }
    }
}

/// Backend-agnostic storage trait for approval requests
///
/// Implementations can store approvals in files, memory, causal chain, etc.
#[async_trait::async_trait]
pub trait ApprovalStorage: Send + Sync {
    /// Add a new approval request
    async fn add(&self, request: ApprovalRequest) -> RuntimeResult<()>;

    /// Update an existing approval request
    async fn update(&self, request: &ApprovalRequest) -> RuntimeResult<()>;

    /// Get a specific approval request by ID
    async fn get(&self, id: &str) -> RuntimeResult<Option<ApprovalRequest>>;

    /// List approval requests matching a filter
    async fn list(&self, filter: ApprovalFilter) -> RuntimeResult<Vec<ApprovalRequest>>;

    /// Remove an approval request
    async fn remove(&self, id: &str) -> RuntimeResult<bool>;

    /// Check and expire timed-out requests
    async fn check_expirations(&self) -> RuntimeResult<Vec<String>>;
}

/// Trait for consumers that can handle approval requests
#[async_trait::async_trait]
pub trait ApprovalConsumer: Send + Sync {
    /// Called when a new approval request is created
    async fn on_approval_requested(&self, request: &ApprovalRequest);

    /// Called when an approval request is resolved (approved, rejected, or expired)
    async fn on_approval_resolved(&self, request: &ApprovalRequest);
}
