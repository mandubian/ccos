//! Agent Identity
//!
//! Defines persistent agent identities with credentials, constraints, and owned capabilities.
//! Supports disk persistence via JSONL for durability across restarts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Constraints on what an agent can do autonomously.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentConstraints {
    /// Maximum autonomy level (0-4, mirrors SelfProgrammingConfig trust levels)
    pub max_autonomy_level: u8,
    /// Domains requiring human approval (e.g., "finance", "pii", "external_apis")
    pub require_approval_domains: Vec<String>,
    /// Maximum concurrent tasks this agent can handle
    pub max_concurrent_tasks: usize,
}

impl Default for AgentConstraints {
    fn default() -> Self {
        Self {
            max_autonomy_level: 2, // Trusted but not full autonomy
            require_approval_domains: vec![],
            max_concurrent_tasks: 5,
        }
    }
}

/// Persistent agent identity with capabilities and constraints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentIdentity {
    /// Unique identifier for this agent
    pub agent_id: String,
    /// Human-readable name
    pub name: String,
    /// Optional description of agent's purpose/specialization
    pub description: Option<String>,
    /// Capabilities this agent has created/owns
    pub capabilities_owned: Vec<String>,
    /// Current autonomy level (must be <= constraints.max_autonomy_level)
    pub autonomy_level: u8,
    /// Constraints on agent behavior
    pub constraints: AgentConstraints,
    /// Unix timestamp when agent was created
    pub created_at: u64,
    /// Unix timestamp of last activity
    pub last_active_at: u64,
    /// Arbitrary metadata for extensibility
    pub metadata: HashMap<String, String>,
}

impl AgentIdentity {
    /// Create a new agent identity with current timestamp.
    pub fn new(agent_id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            agent_id: agent_id.into(),
            name: name.into(),
            description: None,
            capabilities_owned: vec![],
            autonomy_level: 0,
            constraints: AgentConstraints::default(),
            created_at: now,
            last_active_at: now,
            metadata: HashMap::new(),
        }
    }

    /// Builder: set description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Builder: set autonomy level
    pub fn with_autonomy_level(mut self, level: u8) -> Self {
        self.autonomy_level = level.min(self.constraints.max_autonomy_level);
        self
    }

    /// Builder: set constraints
    pub fn with_constraints(mut self, constraints: AgentConstraints) -> Self {
        self.constraints = constraints;
        // Ensure autonomy_level respects new constraints
        self.autonomy_level = self.autonomy_level.min(self.constraints.max_autonomy_level);
        self
    }

    /// Add a capability to this agent's owned list.
    pub fn add_capability(&mut self, capability_id: String) {
        if !self.capabilities_owned.contains(&capability_id) {
            self.capabilities_owned.push(capability_id);
        }
    }

    /// Update last_active_at to current time.
    pub fn touch(&mut self) {
        self.last_active_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

/// Registry managing multiple agent identities with optional disk persistence.
#[derive(Debug)]
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<String, AgentIdentity>>>,
    storage_path: Option<PathBuf>,
}

impl AgentRegistry {
    /// Create a new in-memory registry (no persistence).
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            storage_path: None,
        }
    }

    /// Create a registry with JSONL persistence at the given path.
    pub fn with_persistence(path: impl Into<PathBuf>) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            storage_path: Some(path.into()),
        }
    }

    /// Load agents from disk (if persistence enabled).
    pub fn load(&self) -> Result<(), AgentRegistryError> {
        let Some(path) = &self.storage_path else {
            return Ok(());
        };

        if !path.exists() {
            return Ok(());
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut agents = self
            .agents
            .write()
            .map_err(|_| AgentRegistryError::LockError)?;

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let identity: AgentIdentity = serde_json::from_str(&line)?;
            agents.insert(identity.agent_id.clone(), identity);
        }

        Ok(())
    }

    /// Persist all agents to disk (if persistence enabled).
    pub fn flush(&self) -> Result<(), AgentRegistryError> {
        let Some(path) = &self.storage_path else {
            return Ok(());
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let agents = self
            .agents
            .read()
            .map_err(|_| AgentRegistryError::LockError)?;
        let mut file = File::create(path)?;

        for identity in agents.values() {
            let line = serde_json::to_string(identity)?;
            writeln!(file, "{}", line)?;
        }

        Ok(())
    }

    /// Register a new agent identity.
    pub fn register(&self, identity: AgentIdentity) -> Result<(), AgentRegistryError> {
        let mut agents = self
            .agents
            .write()
            .map_err(|_| AgentRegistryError::LockError)?;
        agents.insert(identity.agent_id.clone(), identity);
        drop(agents);

        // Auto-persist if enabled
        if self.storage_path.is_some() {
            self.flush()?;
        }
        Ok(())
    }

    /// Get an agent by ID.
    pub fn get(&self, agent_id: &str) -> Option<AgentIdentity> {
        let agents = self.agents.read().ok()?;
        agents.get(agent_id).cloned()
    }

    /// Update an existing agent.
    pub fn update(&self, identity: AgentIdentity) -> Result<(), AgentRegistryError> {
        let mut agents = self
            .agents
            .write()
            .map_err(|_| AgentRegistryError::LockError)?;
        if !agents.contains_key(&identity.agent_id) {
            return Err(AgentRegistryError::NotFound(identity.agent_id.clone()));
        }
        agents.insert(identity.agent_id.clone(), identity);
        drop(agents);

        if self.storage_path.is_some() {
            self.flush()?;
        }
        Ok(())
    }

    /// List all registered agents.
    pub fn list(&self) -> Vec<AgentIdentity> {
        let agents = self.agents.read().ok();
        agents
            .map(|a| a.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Remove an agent by ID.
    pub fn remove(&self, agent_id: &str) -> Result<Option<AgentIdentity>, AgentRegistryError> {
        let mut agents = self
            .agents
            .write()
            .map_err(|_| AgentRegistryError::LockError)?;
        let removed = agents.remove(agent_id);
        drop(agents);

        if self.storage_path.is_some() && removed.is_some() {
            self.flush()?;
        }
        Ok(removed)
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from agent registry operations.
#[derive(Debug, thiserror::Error)]
pub enum AgentRegistryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Lock error")]
    LockError,
    #[error("Agent not found: {0}")]
    NotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_creation() {
        let agent = AgentIdentity::new("agent-1", "Test Agent")
            .with_description("A test agent")
            .with_autonomy_level(2);

        assert_eq!(agent.agent_id, "agent-1");
        assert_eq!(agent.name, "Test Agent");
        assert_eq!(agent.autonomy_level, 2);
        assert!(agent.created_at > 0);
    }

    #[test]
    fn test_autonomy_respects_constraints() {
        let constraints = AgentConstraints {
            max_autonomy_level: 1,
            ..Default::default()
        };
        let agent = AgentIdentity::new("a", "A")
            .with_constraints(constraints)
            .with_autonomy_level(5); // Should be clamped to 1

        assert_eq!(agent.autonomy_level, 1);
    }

    #[test]
    fn test_registry_basic_operations() {
        let registry = AgentRegistry::new();
        let agent = AgentIdentity::new("agent-1", "Agent One");

        registry.register(agent.clone()).unwrap();

        let fetched = registry.get("agent-1").unwrap();
        assert_eq!(fetched.name, "Agent One");

        let all = registry.list();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_add_capability() {
        let mut agent = AgentIdentity::new("a", "A");
        agent.add_capability("my.capability".to_string());
        agent.add_capability("my.capability".to_string()); // duplicate

        assert_eq!(agent.capabilities_owned.len(), 1);
    }
}
