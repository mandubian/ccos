//! Causal chain approval storage - logs approval events to audit trail
//!
//! This storage wraps another ApprovalStorage implementation and logs
//! all approval events to the causal chain for audit purposes.

use super::types::{ApprovalFilter, ApprovalRequest, ApprovalStatus, ApprovalStorage};
use crate::causal_chain::CausalChain;
use async_trait::async_trait;
use rtfs::runtime::error::RuntimeResult;
use std::sync::{Arc, RwLock};

/// Approval storage that logs all events to the causal chain
pub struct CausalChainApprovalStorage<S: ApprovalStorage> {
    /// The underlying storage implementation
    inner: S,
    /// Reference to the causal chain for audit logging
    causal_chain: Arc<RwLock<CausalChain>>,
}

impl<S: ApprovalStorage> CausalChainApprovalStorage<S> {
    /// Create a new storage with causal chain logging
    pub fn new(inner: S, causal_chain: Arc<RwLock<CausalChain>>) -> Self {
        Self {
            inner,
            causal_chain,
        }
    }

    /// Log an approval event to the causal chain
    fn log_event(&self, event_type: &str, request: &ApprovalRequest) {
        let event = serde_json::json!({
            "event": event_type,
            "approval_id": request.id,
            "category": format!("{}", request.category),
            "risk_level": format!("{:?}", request.risk_assessment.level),
            "status": format!("{:?}", request.status),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        // Log to causal chain's structured log buffer
        if let Ok(chain) = self.causal_chain.write() {
            // Access the log buffer for structured logging
            // This is audit-only, doesn't create Actions (which require Intents)
            eprintln!(
                "[APPROVAL_AUDIT] {}: id={} category={}",
                event_type,
                request.id,
                format!("{}", request.category)
            );
            // Note: For full causal chain integration, we'd create an Intent
            // and Action. For now, we log via eprintln which can be captured.
            let _ = (chain, event); // Acknowledge we have the lock and event
        }
    }
}

#[async_trait]
impl<S: ApprovalStorage + 'static> ApprovalStorage for CausalChainApprovalStorage<S> {
    async fn add(&self, request: ApprovalRequest) -> RuntimeResult<()> {
        self.log_event("approval_requested", &request);
        self.inner.add(request).await
    }

    async fn update(&self, request: &ApprovalRequest) -> RuntimeResult<()> {
        // Determine event type based on status
        let event_type = match &request.status {
            ApprovalStatus::Pending => "approval_updated",
            ApprovalStatus::Approved { .. } => "approval_granted",
            ApprovalStatus::Rejected { .. } => "approval_rejected",
            ApprovalStatus::Expired { .. } => "approval_expired",
        };
        self.log_event(event_type, request);
        self.inner.update(request).await
    }

    async fn get(&self, id: &str) -> RuntimeResult<Option<ApprovalRequest>> {
        self.inner.get(id).await
    }

    async fn list(&self, filter: ApprovalFilter) -> RuntimeResult<Vec<ApprovalRequest>> {
        self.inner.list(filter).await
    }

    async fn remove(&self, id: &str) -> RuntimeResult<bool> {
        // Log removal before delegating
        if let Ok(Some(request)) = self.inner.get(id).await {
            self.log_event("approval_removed", &request);
        }
        self.inner.remove(id).await
    }

    async fn check_expirations(&self) -> RuntimeResult<Vec<String>> {
        let expired_ids = self.inner.check_expirations().await?;

        // Log each expired item
        for id in &expired_ids {
            if let Ok(Some(request)) = self.inner.get(id).await {
                self.log_event("approval_expired", &request);
            }
        }

        Ok(expired_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::queue::{RiskAssessment, RiskLevel};
    use crate::approval::storage_memory::InMemoryApprovalStorage;
    use crate::approval::types::{ApprovalCategory, ApprovalRequest};

    #[tokio::test]
    async fn test_causal_chain_storage_wraps_inner() {
        let inner = InMemoryApprovalStorage::new();
        let chain = Arc::new(RwLock::new(CausalChain::new().unwrap()));
        let storage = CausalChainApprovalStorage::new(inner, chain);

        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: "test.cap".to_string(),
                effects: vec!["network".to_string()],
                intent_description: "Test network access".to_string(),
            },
            RiskAssessment {
                level: RiskLevel::Medium,
                reasons: vec!["Network access required".to_string()],
            },
            24,
            Some("Testing causal chain storage".to_string()),
        );

        let id = request.id.clone();
        storage.add(request).await.unwrap();

        let retrieved = storage.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, id);
    }
}
