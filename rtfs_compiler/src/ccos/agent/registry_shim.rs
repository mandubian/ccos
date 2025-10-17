//! AgentRegistry shim that delegates to CapabilityMarketplace
//! This provides backward compatibility during the agent unification migration.

use super::registry::{AgentDescriptor, AgentRegistry, IntentDraft, ScoredAgent};
use crate::ccos::capability_marketplace::{CapabilityMarketplace, types::{CapabilityQuery, CapabilityKind}};
use std::sync::Arc;

/// Shim implementation that delegates AgentRegistry calls to CapabilityMarketplace
pub struct AgentRegistryShim {
    marketplace: Arc<CapabilityMarketplace>,
}

impl AgentRegistryShim {
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self { marketplace }
    }

    /// Convert a CapabilityManifest to an AgentDescriptor for backward compatibility
    fn manifest_to_descriptor(manifest: &crate::ccos::capability_marketplace::types::CapabilityManifest) -> AgentDescriptor {
        // Extract skills from metadata or use empty vector
        let skills = manifest.metadata.get("skills")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(Vec::new);
        let supported_constraints = manifest.metadata.get("constraints")
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(Vec::new);
        
        AgentDescriptor {
            agent_id: manifest.id.clone(),
            execution_mode: super::registry::AgentExecutionMode::RTFS {
                plan: manifest.description.clone(),
            },
            skills,
            supported_constraints,
            trust_tier: super::registry::TrustTier::T1Trusted, // Default to trusted
            cost: super::registry::CostModel::default(),
            latency: super::registry::LatencyStats::default(),
            success: super::registry::SuccessStats::default(),
            provenance: Some("marketplace".to_string()),
        }
    }
}

impl AgentRegistry for AgentRegistryShim {
    fn register(&mut self, agent: AgentDescriptor) {
        // Convert AgentDescriptor to CapabilityManifest and register with marketplace
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("skills".to_string(), agent.skills.join(","));
        metadata.insert("constraints".to_string(), agent.supported_constraints.join(","));
        
        let manifest = crate::ccos::capability_marketplace::types::CapabilityManifest {
            id: agent.agent_id.clone(),
            name: agent.agent_id.clone(),
            description: format!("Agent: {}", agent.agent_id),
            version: "1.0.0".to_string(),
            provider: crate::ccos::capability_marketplace::types::ProviderType::Local(
                crate::ccos::capability_marketplace::types::LocalCapability {
                    handler: std::sync::Arc::new(|_args| Ok(crate::runtime::values::Value::String("agent_executed".to_string()))),
                }
            ),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata,
            agent_metadata: Some(crate::ccos::capability_marketplace::types::AgentMetadata {
                kind: CapabilityKind::Agent,
                planning: true,
                stateful: true,
                interactive: false,
                config: std::collections::HashMap::new(),
            }),
        };

        // Use block_in_place for async call in sync context
        let marketplace = Arc::clone(&self.marketplace);
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let _ = marketplace.register_capability_manifest(manifest).await;
            });
        });
    }

    fn list(&self) -> Vec<AgentDescriptor> {
        // Query marketplace for agent capabilities and convert to descriptors
        let marketplace = Arc::clone(&self.marketplace);
        
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let query = CapabilityQuery::new()
                    .with_kind(CapabilityKind::Agent)
                    .with_limit(100);
                
                let manifests = marketplace.list_capabilities_with_query(&query).await;
                manifests.into_iter()
                    .map(|manifest| Self::manifest_to_descriptor(&manifest))
                    .collect()
            })
        })
    }

    fn find_candidates(&self, draft: &IntentDraft, max: usize) -> Vec<ScoredAgent> {
        // Query marketplace for agent capabilities and score them
        let marketplace = Arc::clone(&self.marketplace);
        
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
            let query = CapabilityQuery::new()
                .with_kind(CapabilityKind::Agent)
                .with_limit(max * 2); // Get more candidates for scoring
            
            let manifests = marketplace.list_capabilities_with_query(&query).await;
            let descriptors: Vec<AgentDescriptor> = manifests.into_iter()
                .map(|manifest| Self::manifest_to_descriptor(&manifest))
                .collect();
            
            // Score agents using the same logic as InMemoryAgentRegistry
            let mut scored: Vec<ScoredAgent> = descriptors
                .into_iter()
                .map(|descriptor| {
                    let (score, rationale, skill_hits) = super::registry::InMemoryAgentRegistry::score_agent(&descriptor, draft);
                    ScoredAgent {
                        descriptor,
                        score,
                        rationale,
                        skill_hits,
                    }
                })
                .collect();
            
            scored.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            scored.truncate(max);
            scored
            })
        })
    }

    fn record_feedback(&mut self, agent_id: &str, success: bool) {
        // For now, we don't have a direct way to record feedback in the marketplace
        // This could be extended to store feedback in capability metadata or a separate store
        eprintln!("Warning: AgentRegistryShim::record_feedback called for {} (success: {}). Feedback recording not yet implemented in marketplace.", agent_id, success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::capabilities::registry::CapabilityRegistry;
    use tokio::sync::RwLock;
    use crate::ccos::agent::{
        AgentDescriptor, AgentExecutionMode, TrustTier, CostModel, LatencyStats, SuccessStats
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn test_agent_registry_shim() {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        
        let mut shim = AgentRegistryShim::new(Arc::clone(&marketplace));
        
        // Test listing agents (should be empty initially)
        let agents = shim.list();
        assert!(agents.is_empty());
        
        // Test registering an agent
        let agent = AgentDescriptor {
            agent_id: "test-agent".to_string(),
            execution_mode: AgentExecutionMode::RTFS {
                plan: "test plan".to_string(),
            },
            skills: vec!["testing".to_string()],
            supported_constraints: vec!["constraint1".to_string()],
            trust_tier: TrustTier::T1Trusted,
            cost: CostModel::default(),
            latency: LatencyStats::default(),
            success: SuccessStats::default(),
            provenance: Some("test".to_string()),
        };
        
        shim.register(agent);
        
        // Give async operations time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        // Test listing agents (should now contain our agent)
        let agents = shim.list();
        assert!(!agents.is_empty());
        assert!(agents.iter().any(|a| a.agent_id == "test-agent"));
    }
}
