use std::path::Path;
use std::sync::Arc;
use tracing::info;

use autonoetic_gateway::llm::Message;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub fn init_agent_scaffold(
    config_path: &Path,
    agent_id: &str,
    template: Option<&str>,
) -> anyhow::Result<()> {
    anyhow::ensure!(!agent_id.trim().is_empty(), "agent_id must not be empty");

    let config = autonoetic_gateway::config::load_config(config_path)?;
    std::fs::create_dir_all(&config.agents_dir)?;

    let agent_dir = config.agents_dir.join(agent_id);
    anyhow::ensure!(
        !agent_dir.exists(),
        "Agent '{}' already exists at {}",
        agent_id,
        agent_dir.display()
    );
    std::fs::create_dir_all(&agent_dir)?;
    std::fs::create_dir_all(agent_dir.join("state"))?;
    std::fs::create_dir_all(agent_dir.join("history"))?;
    std::fs::create_dir_all(agent_dir.join("skills"))?;
    std::fs::create_dir_all(agent_dir.join("scripts"))?;

    let skill_md = render_skill_template(agent_id, template);
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    std::fs::write(
        agent_dir.join("runtime.lock"),
        default_runtime_lock_contents(),
    )?;

    println!(
        "Initialized agent '{}' in {}",
        agent_id,
        agent_dir.display()
    );
    Ok(())
}

pub fn render_skill_template(agent_id: &str, template: Option<&str>) -> String {
    let (name_suffix, description, body) = match template.unwrap_or("generic") {
        "researcher" => (
            "Researcher",
            "Research-focused autonomous agent.",
            "You are a researcher agent. Build evidence-based outputs and cite sources.",
        ),
        "coder" => (
            "Coder",
            "Software engineering autonomous agent.",
            "You are a coding agent. Produce tested, minimal, and auditable changes.",
        ),
        "auditor" => (
            "Auditor",
            "Audit and review autonomous agent.",
            "You are an auditor agent. Prioritize correctness, risks, and reproducibility.",
        ),
        _ => (
            "Agent",
            "General-purpose autonomous agent.",
            "You are an autonomous agent. Plan clearly and execute safely.",
        ),
    };
    format!(
        r#"---
name: "{agent_id}"
description: "{description}"
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
      name: "{agent_id} {name_suffix}"
      description: "{description}"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.2
---
# {agent_id}

{body}
"#
    )
}

pub fn default_runtime_lock_contents() -> &'static str {
    r#"gateway:
  artifact: "marketplace://gateway/autonoetic-gateway"
  version: "0.1.0"
  sha256: "replace-me"
sdk:
  version: "0.1.0"
sandbox:
  backend: "bubblewrap"
dependencies: []
artifacts: []
"#
}

pub async fn handle_agent_list(config_path: &Path) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let repo = autonoetic_gateway::AgentRepository::from_config(&config);
    let agents = repo.list().await?;
    if agents.is_empty() {
        println!("No agents found in {}", config.agents_dir.display());
    } else {
        println!("{:<30} {}", "AGENT ID", "DIRECTORY");
        for a in &agents {
            println!("{:<30} {}", a.id, a.dir.display());
        }
    }
    Ok(())
}

pub async fn handle_agent_run(
    config_path: &Path,
    agent_id: &str,
    message: Option<&str>,
    interactive: bool,
    headless: bool,
) -> anyhow::Result<()> {
    info!(
        "Running Agent {} (interactive: {}, headless: {})",
        agent_id, interactive, headless
    );
    if let Some(msg) = message {
        info!("Kickoff message: {}", msg);
    }
    run_agent_with_runtime(config_path, agent_id, message, interactive, headless).await
}

pub async fn run_agent_with_runtime(
    config_path: &Path,
    agent_id: &str,
    kickoff_message: Option<&str>,
    interactive: bool,
    headless: bool,
) -> anyhow::Result<()> {
    let (manifest, instructions, agent_dir) = load_agent_runtime_context(config_path, agent_id)?;
    let llm_config = manifest
        .llm_config
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Agent '{}' is missing llm_config", agent_id))?;
    let driver = autonoetic_gateway::llm::build_driver(llm_config, reqwest::Client::new())?;
    run_agent_with_runtime_with_driver(
        manifest,
        instructions,
        agent_dir,
        kickoff_message,
        interactive,
        headless,
        driver,
    )
    .await
}

pub fn load_agent_runtime_context(
    config_path: &Path,
    agent_id: &str,
) -> anyhow::Result<(autonoetic_types::agent::AgentManifest, String, std::path::PathBuf)> {
    let config = autonoetic_gateway::config::load_config(config_path)?;
    let repo = autonoetic_gateway::AgentRepository::from_config(&config);
    let loaded = repo.get_sync(agent_id)?;
    Ok((loaded.manifest, loaded.instructions, loaded.dir))
}

pub async fn run_agent_with_runtime_with_driver(
    manifest: autonoetic_types::agent::AgentManifest,
    instructions: String,
    agent_dir: std::path::PathBuf,
    kickoff_message: Option<&str>,
    interactive: bool,
    headless: bool,
    driver: Arc<dyn autonoetic_gateway::llm::LlmDriver>,
) -> anyhow::Result<()> {
    if headless {
        tracing::info!("Headless mode enabled.");
    }

    let mut runtime = autonoetic_gateway::runtime::lifecycle::AgentExecutor::new(
        manifest,
        instructions,
        driver,
        agent_dir,
        autonoetic_gateway::runtime::tools::default_registry(),
    );
    if let Some(message) = kickoff_message {
        runtime = runtime.with_initial_user_message(message.to_string());
    }
    if interactive {
        return run_interactive_session(&mut runtime, kickoff_message).await;
    }

    let mut history = vec![
        Message::system(runtime.instructions.clone()),
        Message::user(runtime.initial_user_message.clone()),
    ];
    match runtime.execute_with_history(&mut history).await {
        Ok(Some(reply)) => {
            println!("{}", reply);
            runtime.close_session("headless_complete")?;
        }
        Ok(None) => {
            println!("[No assistant text returned]");
            runtime.close_session("headless_complete_empty")?;
        }
        Err(e) => {
            let _ = runtime.close_session("headless_error");
            return Err(e);
        }
    }
    Ok(())
}

pub async fn run_interactive_session(
    runtime: &mut autonoetic_gateway::runtime::lifecycle::AgentExecutor,
    kickoff_message: Option<&str>,
) -> anyhow::Result<()> {
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut history = vec![Message::system(runtime.instructions.clone())];

    stdout
        .write_all(b"Interactive mode enabled. Type /exit to quit.\n")
        .await?;
    stdout.flush().await?;

    if let Some(message) = kickoff_message {
        history.push(Message::user(message.to_string()));
        match runtime.execute_with_history(&mut history).await {
            Ok(Some(reply)) => {
                stdout.write_all(reply.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = runtime.close_session("interactive_error");
                return Err(e);
            }
        };
    }

    loop {
        stdout.write_all(b"> ").await?;
        stdout.flush().await?;

        let Some(line) = lines.next_line().await? else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "/exit" || trimmed == "/quit" {
            break;
        }

        history.push(Message::user(trimmed.to_string()));
        match runtime.execute_with_history(&mut history).await {
            Ok(Some(reply)) => {
                stdout.write_all(reply.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok(None) => {}
            Err(e) => {
                let _ = runtime.close_session("interactive_error");
                return Err(e);
            }
        };
    }
    runtime.close_session("interactive_exit")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use autonoetic_gateway::llm::{
        CompletionRequest, CompletionResponse, LlmDriver, StopReason, TokenUsage, ToolCall,
    };
    use tempfile::tempdir;

    struct DenySandboxExecDriver;

    #[async_trait::async_trait]
    impl LlmDriver for DenySandboxExecDriver {
        async fn complete(
            &self,
            request: &CompletionRequest,
        ) -> anyhow::Result<CompletionResponse> {
            if !request.tools.iter().any(|t| t.name == "sandbox.exec") {
                anyhow::bail!("sandbox.exec not exposed to model");
            }
            Ok(CompletionResponse {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "sandbox.exec".to_string(),
                    arguments: serde_json::json!({
                        "command": "echo blocked"
                    })
                    .to_string(),
                }],
                stop_reason: StopReason::ToolUse,
                usage: TokenUsage::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_agent_run_path_enforces_sandbox_shell_policy() {
        let temp = tempdir().expect("tempdir should create");
        let agents_dir = temp.path().join("agents");
        let agent_dir = agents_dir.join("agent_demo");
        std::fs::create_dir_all(&agent_dir).expect("agent dir should create");

        let skill = r#"---
version: "1.0"
runtime:
  engine: "autonoetic"
  gateway_version: "0.1.0"
  sdk_version: "0.1.0"
  type: "stateful"
  sandbox: "bubblewrap"
  runtime_lock: "runtime.lock"
agent:
  id: "agent_demo"
  name: "Agent Demo"
  description: "Demo agent"
capabilities:
  - type: "ShellExec"
    patterns:
      - "python3 scripts/*"
---
# Agent Demo
Use tools when needed.
"#;
        std::fs::write(agent_dir.join("SKILL.md"), skill).expect("skill should write");

        let config_path = temp.path().join("config.yaml");
        let config_yaml = format!(
            "agents_dir: \"{}\"\nport: 4000\nofp_port: 4200\ntls: false\n",
            agents_dir.display()
        );
        std::fs::write(&config_path, config_yaml).expect("config should write");

        let (manifest, instructions, loaded_agent_dir) =
            load_agent_runtime_context(&config_path, "agent_demo").expect("context should load");
        let err = run_agent_with_runtime_with_driver(
            manifest,
            instructions,
            loaded_agent_dir,
            Some("start"),
            false,
            true,
            Arc::new(DenySandboxExecDriver),
        )
        .await
        .expect_err("policy denial should fail runtime");

        assert!(
            err.to_string()
                .contains("sandbox command denied by ShellExec policy"),
            "error should indicate shell policy denial"
        );
    }

    #[test]
    fn test_init_agent_scaffold_creates_skill_and_runtime_lock() {
        let temp = tempdir().expect("tempdir should create");
        let config_path = temp.path().join("config.yaml");
        let agents_dir = temp.path().join("agents");
        let config_yaml = format!(
            "agents_dir: \"{}\"\nport: 4000\nofp_port: 4200\ntls: false\n",
            agents_dir.display()
        );
        std::fs::write(&config_path, config_yaml).expect("config should write");

        init_agent_scaffold(&config_path, "agent_bootstrap", Some("coder"))
            .expect("scaffold should succeed");

        let agent_dir = agents_dir.join("agent_bootstrap");
        let skill =
            std::fs::read_to_string(agent_dir.join("SKILL.md")).expect("SKILL.md should exist");
        let lock = std::fs::read_to_string(agent_dir.join("runtime.lock"))
            .expect("runtime.lock should exist");

        assert!(skill.contains("id: \"agent_bootstrap\""));
        assert!(skill.contains("description: \"Software engineering autonomous agent.\""));
        assert!(lock.contains("dependencies: []"));
    }
}
