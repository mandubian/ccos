//! Agent Operations Capabilities
//!
//! RTFS-callable capabilities for agent operations.
//! Provides: agent.create, agent.recall, agent.learn, agent.list
//!
//! This module replaces the legacy `src/agents/capabilities.rs` and implements
//! the unified artifact model where agents are stored in the CapabilityMarketplace.

use crate::capability_marketplace::types::{
    AgentConstraints, CapabilityKind, CapabilityManifest, CapabilityQuery, LocalCapability,
    ProviderType,
};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::working_memory::agent_memory::{AgentMemory, LearnedPattern};
use crate::working_memory::facade::WorkingMemory;
use futures::future::BoxFuture;
use futures::FutureExt;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Input for agent.create
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateAgentInput {
    pub agent_id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub autonomy_level: Option<u8>,
}

/// Output for agent.create
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAgentOutput {
    pub agent_id: String,
    pub created: bool,
    pub message: String,
}

/// Input for agent.recall
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecallInput {
    pub agent_id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Output for agent.recall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallOutput {
    pub entries: Vec<RecallEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallEntry {
    pub id: String,
    pub title: String,
    pub content: String,
    pub timestamp: u64,
}

/// Input for agent.learn
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LearnInput {
    pub agent_id: String,
    pub pattern_id: String,
    pub description: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub error_category: Option<String>,
    #[serde(default)]
    pub suggested_action: Option<String>,
    #[serde(default)]
    pub related_capabilities: Vec<String>,
}

/// Output for agent.learn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnOutput {
    pub stored: bool,
    pub pattern_id: String,
}

/// Input for agent.list
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListAgentsInput {}

/// Output for agent.list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAgentsOutput {
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub name: String,
    pub autonomy_level: u8,
    pub capabilities_count: usize,
    pub created_at: u64,
}

/// Shared state for agent capabilities
pub struct AgentCapabilityState {
    memories: Arc<Mutex<HashMap<String, AgentMemory>>>,
    working_memory: Arc<Mutex<WorkingMemory>>,
}

impl AgentCapabilityState {
    pub fn new(working_memory: Arc<Mutex<WorkingMemory>>) -> Self {
        Self {
            memories: Arc::new(Mutex::new(HashMap::new())),
            working_memory,
        }
    }

    fn get_or_create_memory(&self, agent_id: &str) -> AgentMemory {
        let mut memories = self.memories.lock().unwrap();
        if let Some(_memory) = memories.get(agent_id) {
            // Return a new wrapper with same backend
            AgentMemory::new(agent_id, self.working_memory.clone())
        } else {
            let memory = AgentMemory::new(agent_id, self.working_memory.clone());
            memories.insert(
                agent_id.to_string(),
                AgentMemory::new(agent_id, self.working_memory.clone()),
            );
            memory
        }
    }
}

/// Register agent operational capabilities with the marketplace.
pub async fn register_agent_ops_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    working_memory: Arc<Mutex<WorkingMemory>>,
) -> Result<(), RuntimeError> {
    let state = Arc::new(AgentCapabilityState::new(working_memory));

    // agent.create - Create a new agent (registers a :kind :agent capability)
    marketplace
        .register_native_capability(
            "agent.create".to_string(),
            "Create Agent".to_string(),
            "Register a new goal-directed agent artifact".to_string(),
            Arc::new({
                let marketplace = marketplace.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let marketplace = marketplace.clone();
                    async move {
                        let input: CreateAgentInput = parse_input(&args_clone)?;

                        // Create a manifest for the agent
                        let mut manifest = CapabilityManifest::new_agent(
                            input.agent_id.clone(),
                            input.name.clone(),
                            input
                                .description
                                .unwrap_or_else(|| "Dynamic Agent".to_string()),
                            ProviderType::Local(LocalCapability {
                                handler: Arc::new(|_| Ok(Value::Nil)),
                            }), // Default no-op local provider
                            "1.0.0".to_string(),
                            true, // planning
                            true, // stateful
                            true, // interactive
                        );

                        // Set autonomy level in agent metadata
                        if let Some(ref mut meta) = manifest.agent_metadata {
                            meta.autonomy_level = input.autonomy_level.unwrap_or(0);
                            meta.constraints = AgentConstraints::new(2);
                        }

                        // Register in marketplace
                        match marketplace.register_capability_manifest(manifest).await {
                            Ok(_) => {
                                let output = CreateAgentOutput {
                                    agent_id: input.agent_id,
                                    created: true,
                                    message: "Agent registered in marketplace successfully"
                                        .to_string(),
                                };
                                to_value(&output)
                            }
                            Err(e) => {
                                let output = CreateAgentOutput {
                                    agent_id: input.agent_id,
                                    created: false,
                                    message: format!("Failed to register agent: {}", e),
                                };
                                to_value(&output)
                            }
                        }
                    }
                    .boxed()
                }
            }),
            "medium".to_string(),
        )
        .await?;

    // agent.recall - Recall entries from agent memory
    marketplace
        .register_native_capability(
            "agent.recall".to_string(),
            "Recall Memory".to_string(),
            "Recall relevant entries from an agent's working memory".to_string(),
            Arc::new({
                let state = state.clone();
                let marketplace = marketplace.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let state = state.clone();
                    let marketplace = marketplace.clone();
                    async move {
                        let input: RecallInput = parse_input(&args_clone)?;

                        // Verify agent exists in marketplace
                        if marketplace.get_capability(&input.agent_id).await.is_none() {
                            return Err(RuntimeError::Generic(format!(
                                "Agent capability not found: {}",
                                input.agent_id
                            )));
                        }

                        let memory = state.get_or_create_memory(&input.agent_id);
                        let tags: Vec<&str> = input.tags.iter().map(|s| s.as_str()).collect();

                        match memory.recall_relevant(&tags, input.limit) {
                            Ok(entries) => {
                                let output = RecallOutput {
                                    entries: entries
                                        .into_iter()
                                        .map(|e| RecallEntry {
                                            id: e.id,
                                            title: e.title,
                                            content: e.content,
                                            timestamp: e.timestamp_s,
                                        })
                                        .collect(),
                                };
                                to_value(&output)
                            }
                            Err(e) => Err(RuntimeError::Generic(format!("Recall failed: {}", e))),
                        }
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    // agent.learn - Store a learned pattern
    marketplace
        .register_native_capability(
            "agent.learn".to_string(),
            "Learn Pattern".to_string(),
            "Store a learned pattern in an agent's memory".to_string(),
            Arc::new({
                let state = state.clone();
                let marketplace = marketplace.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let state = state.clone();
                    let marketplace = marketplace.clone();
                    async move {
                        let input: LearnInput = parse_input(&args_clone)?;

                        // Verify agent exists in marketplace
                        if marketplace.get_capability(&input.agent_id).await.is_none() {
                            return Err(RuntimeError::Generic(format!(
                                "Agent capability not found: {}",
                                input.agent_id
                            )));
                        }

                        let mut pattern =
                            LearnedPattern::new(&input.pattern_id, &input.description);

                        if let Some(conf) = input.confidence {
                            pattern = pattern.with_confidence(conf);
                        }
                        if let Some(cat) = input.error_category {
                            pattern = pattern.with_error_category(cat);
                        }
                        if let Some(action) = input.suggested_action {
                            pattern = pattern.with_suggested_action(action);
                        }
                        for cap in input.related_capabilities {
                            pattern.add_related_capability(cap);
                        }

                        let mut memories = state
                            .memories
                            .lock()
                            .map_err(|_| RuntimeError::Generic("Lock error".to_string()))?;

                        let memory = memories.entry(input.agent_id.clone()).or_insert_with(|| {
                            AgentMemory::new(&input.agent_id, state.working_memory.clone())
                        });

                        memory.store_learned_pattern(pattern);

                        let output = LearnOutput {
                            stored: true,
                            pattern_id: input.pattern_id,
                        };
                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    // agent.list - List all agents from the marketplace
    marketplace
        .register_native_capability(
            "agent.list".to_string(),
            "List Agents".to_string(),
            "List all registered agent artifacts in the marketplace".to_string(),
            Arc::new({
                let marketplace = marketplace.clone();
                move |_args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let marketplace = marketplace.clone();
                    async move {
                        let query = CapabilityQuery::new()
                            .with_kind(CapabilityKind::Agent)
                            .with_limit(100);

                        let agents = marketplace.list_capabilities_with_query(&query).await;

                        let output = ListAgentsOutput {
                            agents: agents
                                .into_iter()
                                .map(|a| {
                                    let autonomy = a
                                        .agent_metadata
                                        .as_ref()
                                        .map(|m| m.autonomy_level)
                                        .unwrap_or(0);
                                    AgentSummary {
                                        agent_id: a.id,
                                        name: a.name,
                                        autonomy_level: autonomy,
                                        capabilities_count: 0, // Marketplace-based agents might not "own" fixed lists
                                        created_at: 0, // Could pull from provenance if available
                                    }
                                })
                                .collect(),
                        };
                        to_value(&output)
                    }
                    .boxed()
                }
            }),
            "low".to_string(),
        )
        .await?;

    eprintln!("🤖 Registered 4 unified agent capabilities");
    Ok(())
}

/// Parse input from Value to typed struct
fn parse_input<T: for<'de> Deserialize<'de>>(args: &Value) -> RuntimeResult<T> {
    let json = crate::utils::value_conversion::rtfs_value_to_json(args)?;
    serde_json::from_value(json)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse input: {}", e)))
}

/// Convert output struct to Value
fn to_value<T: Serialize>(output: &T) -> RuntimeResult<Value> {
    let json = serde_json::to_value(output)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize output: {}", e)))?;
    crate::utils::value_conversion::json_to_rtfs_value(&json)
}
