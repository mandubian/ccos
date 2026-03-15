use std::path::{Path, PathBuf};

pub const APPROVED_REUSE_MATH_AGENT_SKILL: &str = r#"---
name: "Math Agent"
description: "Does math"
metadata:
  autonoetic:
    version: "1.0"
    agent:
      id: "math_agent"
      name: "math_agent"
      description: "Does math"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
---
# Instructions
Reply with the sum.
"#;

pub fn install_outbound_reply_agent(agent_dir: &Path, agent_id: &str) -> anyhow::Result<PathBuf> {
    install_agent(
        agent_dir,
        &format!(
            "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"{agent_id}\"\n  description: \"Outbound reply test agent\"\nllm_config:\n  provider: \"openai\"\n  model: \"test-model\"\n  temperature: 0.0\n---\n# Instructions\nReply with the model output.\n",
        ),
    )
}

pub fn install_memory_recall_agent(agent_dir: &Path, agent_id: &str) -> anyhow::Result<PathBuf> {
    install_agent(
        agent_dir,
        &format!(
            "---\nname: \"Memory Agent\"\ndescription: \"Integration test memory agent\"\nmetadata:\n  autonoetic:\n    version: \"1.0\"\n    agent:\n      id: \"{agent_id}\"\n      name: \"memory_agent\"\n      description: \"mock agent\"\n    llm_config:\n      provider: \"openai\"\n      model: \"gpt-4o\"\n    capabilities:\n      - type: \"MemoryWrite\"\n        scopes: [\"*\"]\n      - type: \"MemoryRead\"\n        scopes: [\"*\"]\n---\n# Instructions\nYou are a memory agent.\n",
        ),
    )
}

pub fn install_content_agent(agent_dir: &Path, agent_id: &str) -> anyhow::Result<PathBuf> {
    install_agent(
        agent_dir,
        &format!(
            "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"content_agent\"\n  description: \"Content store test agent\"\nllm_config:\n  provider: \"openai\"\n  model: \"gpt-4o\"\n  temperature: 0.0\ncapabilities:\n  - type: \"ToolInvoke\"\n    allowed: [\"content.read\", \"content.write\", \"content.persist\"]\n  - type: \"MemoryRead\"\n    scopes: [\"*\"]\n  - type: \"MemoryWrite\"\n    scopes: [\"*\"]\n---\n# Instructions\nYou are a content agent that reads and writes content.\n",
        ),
    )
}

pub fn install_generated_skill_learner_agent(
    agent_dir: &Path,
    agent_id: &str,
) -> anyhow::Result<PathBuf> {
    install_agent(
        agent_dir,
        &format!(
            "---\nversion: \"1.0\"\nruntime:\n  engine: \"autonoetic\"\n  gateway_version: \"0.1.0\"\n  sdk_version: \"0.1.0\"\n  type: \"stateful\"\n  sandbox: \"bubblewrap\"\n  runtime_lock: \"runtime.lock\"\nagent:\n  id: \"{agent_id}\"\n  name: \"learner\"\n  description: \"A learning agent\"\nllm_config:\n  provider: \"openai\"\n  model: \"gpt-4o\"\ncapabilities:\n  - type: \"MemoryWrite\"\n    scopes: [\"skills/*\"]\n  - type: \"BackgroundReevaluation\"\n    min_interval_secs: 1\n    allow_reasoning: true\nbackground:\n  enabled: true\n  mode: \"reasoning\"\n  interval_secs: 1\n---\n# Instructions\nYou are a learning agent.\n",
        ),
    )
}

pub fn install_approved_reuse_math_agent(
    agent_dir: &Path,
    agent_id: &str,
) -> anyhow::Result<PathBuf> {
    let skill = APPROVED_REUSE_MATH_AGENT_SKILL.replace("math_agent", agent_id);
    install_agent(agent_dir, &skill)
}

fn install_agent(agent_dir: &Path, skill_md: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(agent_dir)?;
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    Ok(agent_dir.to_path_buf())
}
