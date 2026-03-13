//! Agent Repository - unified agent loading and identity management.

use crate::runtime::parser::SkillParser;
use autonoetic_types::agent::{AgentManifest, AgentMeta};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A fully loaded agent with its manifest and instructions.
#[derive(Debug, Clone)]
pub struct LoadedAgent {
    pub dir: PathBuf,
    pub manifest: AgentManifest,
    pub instructions: String,
}

impl LoadedAgent {
    /// Returns the agent's ID from the manifest.
    pub fn id(&self) -> &str {
        &self.manifest.agent.id
    }

    /// Returns the directory name.
    pub fn dir_name(&self) -> String {
        self.dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// Repository for discovering and loading agents.
/// Provides unified agent loading across gateway, scheduler, router, and CLI.
pub struct AgentRepository {
    agents_dir: PathBuf,
    cache: RwLock<Vec<AgentMeta>>,
}

impl AgentRepository {
    /// Create a new repository scanning the given agents directory.
    pub fn new(agents_dir: PathBuf) -> Self {
        Self {
            agents_dir,
            cache: RwLock::new(Vec::new()),
        }
    }

    /// Create from a config's agents directory.
    pub fn from_config(config: &autonoetic_types::config::GatewayConfig) -> Self {
        Self::new(config.agents_dir.clone())
    }

    /// Refresh the agent cache by scanning the directory.
    pub async fn refresh(&self) -> anyhow::Result<Vec<AgentMeta>> {
        let agents = scan_agents(&self.agents_dir)?;
        *self.cache.write().await = agents.clone();
        Ok(agents)
    }

    /// Get cached agents (or scan if empty).
    pub async fn list(&self) -> anyhow::Result<Vec<AgentMeta>> {
        let cache = self.cache.read().await;
        if !cache.is_empty() {
            return Ok(cache.clone());
        }
        drop(cache);
        self.refresh().await
    }

    /// Load a specific agent by ID.
    /// Returns an error if the agent doesn't exist or identity mismatch.
    pub async fn get(&self, agent_id: &str) -> anyhow::Result<LoadedAgent> {
        let meta = self
            .list()
            .await?
            .into_iter()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", agent_id))?;

        self.load_from_meta(&meta)
    }

    /// Load a specific agent by ID synchronously (scans directory directly).
    /// Returns an error if the agent doesn't exist or identity mismatch.
    pub fn get_sync(&self, agent_id: &str) -> anyhow::Result<LoadedAgent> {
        let agents = scan_agents(&self.agents_dir)?;
        let meta = agents
            .into_iter()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' not found", agent_id))?;

        self.load_from_meta(&meta)
    }

    /// Load all agents synchronously in a single directory scan.
    /// Returns a vector of LoadedAgent, or an error if any agent fails to load.
    pub fn list_loaded_sync(&self) -> anyhow::Result<Vec<LoadedAgent>> {
        let agents = scan_agents(&self.agents_dir)?;
        let mut loaded = Vec::new();
        let mut errors = Vec::new();
        for meta in agents {
            match self.load_from_meta(&meta) {
                Ok(loaded_agent) => loaded.push(loaded_agent),
                Err(e) => errors.push((meta.id.clone(), e)),
            }
        }

        if !errors.is_empty() {
            let error_details: Vec<String> = errors
                .iter()
                .map(|(id, e)| format!("  - {}: {}", id, e))
                .collect();
            anyhow::bail!(
                "Failed to load {} agent(s):\n{}",
                errors.len(),
                error_details.join("\n")
            );
        }

        Ok(loaded)
    }

    /// Load an agent from an AgentMeta, enforcing identity rules.
    pub fn load_from_meta(&self, meta: &AgentMeta) -> anyhow::Result<LoadedAgent> {
        let skill_path = meta.dir.join("SKILL.md");
        let skill_content = std::fs::read_to_string(&skill_path)?;
        let (manifest, instructions) = SkillParser::parse(&skill_content)?;

        // Enforce identity: directory name must match manifest agent ID
        let dir_name = meta
            .dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        if dir_name != manifest.agent.id {
            anyhow::bail!(
                "Agent identity mismatch: directory name '{}' does not match manifest agent.id '{}'. \
                Either rename the directory to match the agent ID, or fix the agent.id in SKILL.md.",
                dir_name,
                manifest.agent.id
            );
        }

        // Validate execution_mode: Script requires script_entry
        use autonoetic_types::agent::ExecutionMode;
        if matches!(manifest.execution_mode, ExecutionMode::Script) {
            if manifest.script_entry.is_none() {
                anyhow::bail!(
                    "Agent '{}' has execution_mode=script but is missing script_entry. \
                    Add 'script_entry: scripts/main.py' to the agent manifest.",
                    manifest.agent.id
                );
            }
        }

        Ok(LoadedAgent {
            dir: meta.dir.clone(),
            manifest,
            instructions,
        })
    }

    /// Try to load an agent, returning None if not found.
    /// Returns an error only for identity mismatch or other actual errors.
    /// Useful for scenarios where missing agents are acceptable.
    pub async fn try_get(&self, agent_id: &str) -> anyhow::Result<Option<LoadedAgent>> {
        let agents = self.list().await?;

        // First check if agent exists in directory
        let exists = agents.iter().any(|a| a.id == agent_id);
        if !exists {
            return Ok(None);
        }

        // Agent exists, try to load it (this will enforce identity)
        match self.get(agent_id).await {
            Ok(loaded) => Ok(Some(loaded)),
            Err(e) => {
                // If it's a "not found" error (shouldn't happen given we checked exists), return None
                if e.to_string().contains("not found") {
                    Ok(None)
                } else {
                    // Re-throw identity mismatch or other errors
                    Err(e)
                }
            }
        }
    }

    /// Get the agents directory path.
    pub fn agents_dir(&self) -> &Path {
        &self.agents_dir
    }
}

/// Scan a directory for agent subdirectories.
///
/// Each subdirectory containing a `SKILL.md` file is treated as an agent.
pub fn scan_agents(dir: &Path) -> anyhow::Result<Vec<AgentMeta>> {
    let mut agents = Vec::new();

    if !dir.exists() {
        tracing::warn!("Agents directory {} does not exist", dir.display());
        return Ok(agents);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if skill_md.exists() {
                let id = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                agents.push(AgentMeta { id, dir: path });
            }
        }
    }

    tracing::info!("Discovered {} agent(s)", agents.len());
    Ok(agents)
}

/// Create a cached agent repository wrapper.
pub fn cached(agents_dir: PathBuf) -> Arc<AgentRepository> {
    Arc::new(AgentRepository::new(agents_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_agent(temp_dir: &Path, agent_id: &str) -> anyhow::Result<PathBuf> {
        let agent_dir = temp_dir.join(agent_id);
        std::fs::create_dir_all(agent_dir.join("state"))?;
        std::fs::create_dir_all(agent_dir.join("skills"))?;

        let skill_md = format!(
            r#"---
name: "{agent_id}"
description: "Test agent"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "{agent_id}"
      name: "{agent_id}"
      description: "Test agent"
    capabilities: []
---
# {agent_id}
Test instructions.
"#
        );
        std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
        Ok(agent_dir)
    }

    #[test]
    fn test_agent_repository_loads_agent() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

        create_test_agent(&agents_dir, "test-agent").expect("agent should create");

        let repo = AgentRepository::new(agents_dir);
        let loaded = repo.get_sync("test-agent").expect("should load agent");

        assert_eq!(loaded.id(), "test-agent");
        assert!(loaded.instructions.contains("Test instructions"));
    }

    #[test]
    fn test_agent_repository_identity_mismatch() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

        // Create agent with directory name "dir-agent" but manifest says "different-id"
        let agent_dir = agents_dir.join("dir-agent");
        std::fs::create_dir_all(agent_dir.join("state")).expect("agent dir should create");

        let skill_md = r#"---
name: "different-id"
description: "Test agent"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "different-id"
      name: "different-id"
      description: "Test agent"
    capabilities: []
---
# different-id
Test instructions.
"#;
        std::fs::write(agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");

        let repo = AgentRepository::new(agents_dir);
        let result = repo.get_sync("dir-agent");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("identity mismatch"));
    }

    #[test]
    fn test_agent_repository_script_mode_requires_script_entry() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

        let agent_dir = agents_dir.join("script-agent");
        std::fs::create_dir_all(agent_dir.join("state")).expect("agent dir should create");

        let skill_md = r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "script-agent"
  name: "Script Agent"
  description: "A script-only agent"
execution_mode: script
# Missing script_entry!
capabilities: []
---
# Script Agent
"#;
        std::fs::write(agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");

        let repo = AgentRepository::new(agents_dir);
        let result = repo.get_sync("script-agent");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("execution_mode=script but is missing script_entry"));
    }

    #[tokio::test]
    async fn test_agent_repository_list() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

        create_test_agent(&agents_dir, "agent-a").expect("agent-a should create");
        create_test_agent(&agents_dir, "agent-b").expect("agent-b should create");

        let repo = AgentRepository::new(agents_dir);
        let agents = repo.list().await.expect("should list agents");

        assert_eq!(agents.len(), 2);
        let ids: Vec<_> = agents.iter().map(|a| a.id.clone()).collect();
        assert!(ids.contains(&"agent-a".to_string()));
        assert!(ids.contains(&"agent-b".to_string()));
    }

    #[test]
    fn test_list_loaded_sync_fails_on_identity_mismatch() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        std::fs::create_dir_all(&agents_dir).expect("agents dir should create");

        create_test_agent(&agents_dir, "good-agent").expect("good agent should create");

        let bad_agent_dir = agents_dir.join("bad-dir");
        std::fs::create_dir_all(bad_agent_dir.join("state")).expect("bad agent dir should create");
        std::fs::create_dir_all(bad_agent_dir.join("skills")).expect("skills dir should create");

        let skill_md = r#"---
name: "bad-dir"
description: "Test agent"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "different-id"
      name: "Test Agent"
      description: "Test agent"
    capabilities: []
---
# different-id
Test instructions.
"#;
        std::fs::write(bad_agent_dir.join("SKILL.md"), skill_md).expect("skill.md should write");

        let repo = AgentRepository::new(agents_dir);
        let result = repo.list_loaded_sync();

        assert!(
            result.is_err(),
            "list_loaded_sync should fail on identity mismatch"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("identity mismatch"),
            "Error should mention identity mismatch: {}",
            err
        );
    }

}
