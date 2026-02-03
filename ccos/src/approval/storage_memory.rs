//! In-memory approval storage implementation
//!
//! Simple in-memory storage for testing and ephemeral use cases.

use super::types::{ApprovalFilter, ApprovalRequest, ApprovalStorage};
use async_trait::async_trait;
use chrono::Utc;
use rtfs::runtime::error::RuntimeResult;
use std::collections::HashMap;
use std::sync::RwLock;

/// In-memory approval storage for testing
pub struct InMemoryApprovalStorage {
    requests: RwLock<HashMap<String, ApprovalRequest>>,
}

impl InMemoryApprovalStorage {
    pub fn new() -> Self {
        Self {
            requests: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryApprovalStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ApprovalStorage for InMemoryApprovalStorage {
    async fn add(&self, request: ApprovalRequest) -> RuntimeResult<()> {
        let mut requests = self.requests.write().unwrap();
        requests.insert(request.id.clone(), request);
        Ok(())
    }

    async fn update(&self, request: &ApprovalRequest) -> RuntimeResult<()> {
        let mut requests = self.requests.write().unwrap();
        requests.insert(request.id.clone(), request.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> RuntimeResult<Option<ApprovalRequest>> {
        let requests = self.requests.read().unwrap();
        Ok(requests.get(id).cloned())
    }

    async fn list(&self, filter: ApprovalFilter) -> RuntimeResult<Vec<ApprovalRequest>> {
        let requests = self.requests.read().unwrap();
        let mut results: Vec<ApprovalRequest> = requests
            .values()
            .filter(|r| {
                // Filter by pending status
                if let Some(pending) = filter.status_pending {
                    if pending != r.status.is_pending() {
                        return false;
                    }
                }
                // Filter by category type
                if let Some(ref cat_type) = filter.category_type {
                    let actual_type = match &r.category {
                        super::types::ApprovalCategory::ServerDiscovery { .. } => "ServerDiscovery",
                        super::types::ApprovalCategory::EffectApproval { .. } => "EffectApproval",
                        super::types::ApprovalCategory::SynthesisApproval { .. } => {
                            "SynthesisApproval"
                        }
                        super::types::ApprovalCategory::LlmPromptApproval { .. } => {
                            "LlmPromptApproval"
                        }
                        super::types::ApprovalCategory::SecretRequired { .. } => "SecretRequired",
                        super::types::ApprovalCategory::BudgetExtension { .. } => "BudgetExtension",
                        super::types::ApprovalCategory::ChatPolicyException { .. } => {
                            "ChatPolicyException"
                        }
                        super::types::ApprovalCategory::ChatPublicDeclassification { .. } => {
                            "ChatPublicDeclassification"
                        }
                        super::types::ApprovalCategory::SecretWrite { .. } => "SecretWrite",
                        super::types::ApprovalCategory::HumanActionRequest { .. } => "HumanActionRequest",
                    };
                    if actual_type != cat_type {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();

        // Sort by requested_at descending (newest first)
        results.sort_by(|a, b| b.requested_at.cmp(&a.requested_at));

        // Apply limit
        if let Some(limit) = filter.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    async fn remove(&self, id: &str) -> RuntimeResult<bool> {
        let mut requests = self.requests.write().unwrap();
        Ok(requests.remove(id).is_some())
    }

    async fn check_expirations(&self) -> RuntimeResult<Vec<String>> {
        let now = Utc::now();
        let mut expired_ids = Vec::new();

        {
            let mut requests = self.requests.write().unwrap();
            for request in requests.values_mut() {
                if request.status.is_pending() && request.expires_at < now {
                    request.expire();
                    expired_ids.push(request.id.clone());
                }
            }
        }

        Ok(expired_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::queue::{RiskAssessment, RiskLevel};
    use crate::approval::types::{ApprovalCategory, ApprovalRequest};

    #[tokio::test]
    async fn test_add_and_get() {
        let storage = InMemoryApprovalStorage::new();

        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: "test.cap".to_string(),
                effects: vec!["read".to_string()],
                intent_description: "Test intent".to_string(),
            },
            RiskAssessment {
                level: RiskLevel::Low,
                reasons: vec![],
            },
            24,
            None,
        );

        let id = request.id.clone();
        storage.add(request).await.unwrap();

        let retrieved = storage.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, id);
    }

    #[tokio::test]
    async fn test_list_pending() {
        let storage = InMemoryApprovalStorage::new();

        let request = ApprovalRequest::new(
            ApprovalCategory::EffectApproval {
                capability_id: "test.cap".to_string(),
                effects: vec!["read".to_string()],
                intent_description: "Test".to_string(),
            },
            RiskAssessment {
                level: RiskLevel::Low,
                reasons: vec![],
            },
            24,
            None,
        );

        storage.add(request).await.unwrap();

        let pending = storage.list(ApprovalFilter::pending()).await.unwrap();
        assert_eq!(pending.len(), 1);
    }
}
