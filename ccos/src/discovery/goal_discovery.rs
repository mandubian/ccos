use crate::discovery::approval_queue::{ApprovalQueue, PendingDiscovery, RiskAssessment, RiskLevel};
use crate::discovery::registry_search::RegistrySearcher;
use rtfs::runtime::error::RuntimeResult;
use chrono::Utc;
use uuid::Uuid;

pub struct GoalDiscoveryAgent {
    registry_searcher: RegistrySearcher,
    approval_queue: ApprovalQueue,
}

impl GoalDiscoveryAgent {
    pub fn new(approval_queue: ApprovalQueue) -> Self {
        Self {
            registry_searcher: RegistrySearcher::new(),
            approval_queue,
        }
    }

    pub async fn process_goal(&self, goal: &str) -> RuntimeResult<Vec<String>> {
        // Simple keyword extraction: splitting by space for now
        // In real impl, use NLP or LLM
        let keywords: Vec<&str> = goal.split_whitespace().collect();
        // Use the whole goal as query for now as registry search handles it
        let results = self.registry_searcher.search(goal).await?;
        
        let mut queued_ids = Vec::new();
        
        for result in results {
            let id = format!("discovery-{}", Uuid::new_v4());
            
            // Risk assessment logic
            let risk = if result.server_info.endpoint.starts_with("https://") {
                RiskLevel::Medium
            } else {
                RiskLevel::High // No HTTPS or unknown
            };
            
            let discovery = PendingDiscovery {
                id: id.clone(),
                source: result.source,
                server_info: result.server_info,
                domain_match: keywords.iter().map(|s| s.to_string()).collect(),
                risk_assessment: RiskAssessment {
                    level: risk,
                    reasons: vec!["external_registry".to_string()],
                },
                requested_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::hours(24),
                requesting_goal: Some(goal.to_string()),
            };
            
            self.approval_queue.add(discovery)?;
            queued_ids.push(id);
        }
        
        Ok(queued_ids)
    }
}

