//! SKILL.md Parser.

use autonoetic_types::agent::{
    AgentIdentity, AgentManifest, LlmConfig, ResourceLimits, RuntimeDeclaration,
};
use autonoetic_types::background::BackgroundPolicy;
use autonoetic_types::capability::Capability;
use gray_matter::{engine::YAML, Matter};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct StandardSkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    metadata: Option<StandardMetadataRoot>,
}

#[derive(Debug, Deserialize, Default)]
struct StandardMetadataRoot {
    #[serde(default)]
    autonoetic: Option<AutonoeticMetadata>,
}

#[derive(Debug, Deserialize, Default)]
struct AutonoeticMetadata {
    version: Option<String>,
    runtime: Option<RuntimeDeclaration>,
    agent: Option<AgentIdentity>,
    #[serde(default)]
    capabilities: Option<Vec<Capability>>,
    llm_config: Option<LlmConfig>,
    limits: Option<ResourceLimits>,
    background: Option<BackgroundPolicy>,
    #[serde(default)]
    disclosure: Option<autonoetic_types::disclosure::DisclosurePolicy>,
}

/// Parser for `SKILL.md` files.
pub struct SkillParser;

impl SkillParser {
    /// Parses a `SKILL.md` content string into an `AgentManifest` and the Markdown body.
    pub fn parse(content: &str) -> anyhow::Result<(AgentManifest, String)> {
        let matter = Matter::<YAML>::new();
        let parsed = matter
            .parse(content)
            .map_err(|e| anyhow::anyhow!("gray_matter error: {}", e))?;

        let data: gray_matter::Pod = parsed
            .data
            .ok_or_else(|| anyhow::anyhow!("No YAML frontmatter found in SKILL.md"))?;

        let manifest = match data.deserialize::<AgentManifest>() {
            Ok(v) => v,
            Err(agent_manifest_err) => {
                let standard = data.deserialize::<StandardSkillFrontmatter>().map_err(|standard_err| {
                    anyhow::anyhow!(
                        "Invalid SKILL.md frontmatter. Autonoetic format error: {}. AgentSkills format error: {}",
                        agent_manifest_err,
                        standard_err
                    )
                })?;
                map_standard_frontmatter_to_manifest(standard)
            }
        };

        Ok((manifest, parsed.content))
    }
}

fn map_standard_frontmatter_to_manifest(standard: StandardSkillFrontmatter) -> AgentManifest {
    let meta = standard
        .metadata
        .and_then(|m| m.autonoetic)
        .unwrap_or_default();

    let runtime = meta.runtime.unwrap_or_else(default_runtime);
    let mut agent = meta.agent.unwrap_or_else(|| AgentIdentity {
        id: standard.name.clone(),
        name: standard.name.clone(),
        description: standard.description.clone(),
    });
    if agent.id.trim().is_empty() {
        agent.id = standard.name.clone();
    }
    if agent.name.trim().is_empty() {
        agent.name = standard.name.clone();
    }
    if agent.description.trim().is_empty() {
        agent.description = standard.description.clone();
    }

    AgentManifest {
        version: meta.version.unwrap_or_else(|| "1.0".to_string()),
        runtime,
        agent,
        capabilities: meta.capabilities.unwrap_or_default(),
        llm_config: meta.llm_config,
        limits: meta.limits,
        background: meta.background,
        disclosure: meta.disclosure,
    }
}

fn default_runtime() -> RuntimeDeclaration {
    RuntimeDeclaration {
        engine: "autonoetic".to_string(),
        gateway_version: "0.1.0".to_string(),
        sdk_version: "0.1.0".to_string(),
        runtime_type: "stateful".to_string(),
        sandbox: "bubblewrap".to_string(),
        runtime_lock: "runtime.lock".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_skill() {
        let content = r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "uv.lock"
agent:
  id: "test_agent"
  name: "Test Agent"
  description: "A test agent"
---
# Test Agent Instructions
Here are the instructions.
"#;
        let (manifest, body) = SkillParser::parse(content).unwrap();
        assert_eq!(manifest.version, "1.0");
        assert_eq!(manifest.agent.id, "test_agent");
        assert_eq!(
            body.trim(),
            "# Test Agent Instructions\nHere are the instructions."
        );
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# Just markdown\nNo frontmatter here.";
        assert!(SkillParser::parse(content).is_err());
    }

    #[test]
    fn test_parse_agentskills_standard_with_autonoetic_metadata() {
        let content = r#"---
name: "test-agent"
description: "A standard AgentSkills entry"
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
      id: "test-agent"
      name: "Test Agent"
      description: "A standard AgentSkills entry"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.2
---
# Test Agent Instructions
Use the skill.
"#;
        let (manifest, body) = SkillParser::parse(content).expect("should parse");
        assert_eq!(manifest.version, "1.0");
        assert_eq!(manifest.agent.id, "test-agent");
        assert_eq!(
            manifest.llm_config.as_ref().map(|c| c.provider.as_str()),
            Some("openai")
        );
        assert_eq!(body.trim(), "# Test Agent Instructions\nUse the skill.");
    }

    #[test]
    fn test_parse_background_policy() {
        let content = r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "bg-agent"
  name: "Background Agent"
  description: "Agent with background policy"
capabilities:
  - type: BackgroundReevaluation
    min_interval_secs: 30
    allow_reasoning: false
background:
  enabled: true
  interval_secs: 45
  mode: deterministic
  wake_predicates:
    timer: true
    stale_goals: true
---
# Background Agent
"#;
        let (manifest, _body) = SkillParser::parse(content).expect("should parse");
        let background = manifest.background.expect("background should parse");
        assert!(background.enabled);
        assert_eq!(background.interval_secs, 45);
        assert!(background.wake_predicates.timer);
        assert!(background.wake_predicates.stale_goals);
    }
}
