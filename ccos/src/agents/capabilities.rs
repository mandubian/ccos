//! Agent Capabilities
//!
//! RTFS-callable capabilities for agent operations.
//! Provides: agent.create, agent.recall, agent.learn, agent.list

use crate::agents::identity::{AgentIdentity, AgentRegistry, AgentRegistryError};
use crate::agents::memory::{AgentMemory, LearnedPattern};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::working_memory::backend_inmemory::InMemoryJsonlBackend;
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
    registry: Arc<AgentRegistry>,
    memories: Arc<Mutex<HashMap<String, AgentMemory>>>,
    working_memory: Arc<Mutex<WorkingMemory>>,
}

impl AgentCapabilityState {
    pub fn new(registry: Arc<AgentRegistry>, working_memory: Arc<Mutex<WorkingMemory>>) -> Self {
        Self {
            registry,
            memories: Arc::new(Mutex::new(HashMap::new())),
            working_memory,
        }
    }

    fn get_or_create_memory(&self, agent_id: &str) -> AgentMemory {
        let mut memories = self.memories.lock().unwrap();
        if let Some(memory) = memories.get(agent_id) {
            // Clone just the parameters, create new wrapper
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

/// Register agent capabilities with the marketplace.
pub async fn register_agent_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    registry: Arc<AgentRegistry>,
    working_memory: Arc<Mutex<WorkingMemory>>,
) -> Result<(), RuntimeError> {
    let state = Arc::new(AgentCapabilityState::new(registry, working_memory));

    // agent.create - Create a new agent identity
    marketplace
        .register_native_capability(
            "agent.create".to_string(),
            "Create Agent".to_string(),
            "Create a new persistent agent identity".to_string(),
            Arc::new({
                let state = state.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let state = state.clone();
                    async move {
                        let input: CreateAgentInput = parse_input(&args_clone)?;

                        let mut identity = AgentIdentity::new(&input.agent_id, &input.name);
                        if let Some(desc) = input.description {
                            identity = identity.with_description(desc);
                        }
                        if let Some(level) = input.autonomy_level {
                            identity = identity.with_autonomy_level(level);
                        }

                        match state.registry.register(identity) {
                            Ok(_) => {
                                let output = CreateAgentOutput {
                                    agent_id: input.agent_id,
                                    created: true,
                                    message: "Agent created successfully".to_string(),
                                };
                                to_value(&output)
                            }
                            Err(e) => {
                                let output = CreateAgentOutput {
                                    agent_id: input.agent_id,
                                    created: false,
                                    message: format!("Failed to create agent: {}", e),
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
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let state = state.clone();
                    async move {
                        let input: RecallInput = parse_input(&args_clone)?;

                        // Verify agent exists
                        if state.registry.get(&input.agent_id).is_none() {
                            return Err(RuntimeError::Generic(format!(
                                "Agent not found: {}",
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
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let args_clone = args.clone();
                    let state = state.clone();
                    async move {
                        let input: LearnInput = parse_input(&args_clone)?;

                        // Verify agent exists
                        if state.registry.get(&input.agent_id).is_none() {
                            return Err(RuntimeError::Generic(format!(
                                "Agent not found: {}",
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

                        // Store in memory (note: in production, would persist)
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

    // agent.list - List all registered agents
    marketplace
        .register_native_capability(
            "agent.list".to_string(),
            "List Agents".to_string(),
            "List all registered agent identities".to_string(),
            Arc::new({
                let state = state.clone();
                move |args: &Value| -> BoxFuture<'static, RuntimeResult<Value>> {
                    let state = state.clone();
                    async move {
                        let agents = state.registry.list();
                        let output = ListAgentsOutput {
                            agents: agents
                                .into_iter()
                                .map(|a| AgentSummary {
                                    agent_id: a.agent_id,
                                    name: a.name,
                                    autonomy_level: a.autonomy_level,
                                    capabilities_count: a.capabilities_owned.len(),
                                    created_at: a.created_at,
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

    eprintln!("🤖 Registered 4 agent capabilities");
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
