use std::path::Path;
use std::sync::Arc;
use tracing::info;

use autonoetic_gateway::llm::Message;
use autonoetic_types::config::{GatewayConfig, LlmPreset};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// LLM configuration for template rendering
#[derive(Debug, Clone, Default)]
pub struct LlmTemplateConfig {
    pub provider: String,
    pub model: String,
    pub temperature: f64,
}

/// Resolve LLM config from CLI flags, presets, or defaults
pub fn resolve_llm_config(
    config: &GatewayConfig,
    template: Option<&str>,
    preset_name: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
) -> LlmTemplateConfig {
    // 1. Direct CLI override takes highest priority
    if let Some(p) = provider {
        return LlmTemplateConfig {
            provider: p.to_string(),
            model: model.unwrap_or("gpt-4o").to_string(),
            temperature: 0.2,
        };
    }

    // 2. Named preset from config
    if let Some(preset_name) = preset_name {
        if let Some(preset) = config.llm_presets.get(preset_name) {
            return LlmTemplateConfig {
                provider: preset.provider.clone(),
                model: preset.model.clone(),
                temperature: preset.temperature.unwrap_or(0.2),
            };
        }
    }

    // 3. Role-based preset mapping from config
    if let Some(template_name) = template {
        if let Some(mapped_preset_name) = config.llm_preset_mapping.get(template_name) {
            if let Some(preset) = config.llm_presets.get(mapped_preset_name) {
                return LlmTemplateConfig {
                    provider: preset.provider.clone(),
                    model: preset.model.clone(),
                    temperature: preset.temperature.unwrap_or(0.2),
                };
            }
        }
    }

    // 4. Hardcoded defaults per template (backward compatible)
    match template.unwrap_or("generic") {
        "planner" => LlmTemplateConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            temperature: 0.2,
        },
        "coder" => LlmTemplateConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            temperature: 0.1,
        },
        "researcher" => LlmTemplateConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.3,
        },
        _ => LlmTemplateConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            temperature: 0.2,
        },
    }
}

pub fn init_agent_scaffold(
    config_path: &Path,
    agent_id: &str,
    template: Option<&str>,
    preset: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
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

    let llm_config = resolve_llm_config(&config, template, preset, provider, model);
    let skill_md = render_skill_template(agent_id, template, &llm_config);
    std::fs::write(agent_dir.join("SKILL.md"), skill_md)?;
    std::fs::write(
        agent_dir.join("runtime.lock"),
        default_runtime_lock_contents(),
    )?;

    println!(
        "Initialized agent '{}' in {} (llm: {}/{})",
        agent_id,
        agent_dir.display(),
        llm_config.provider,
        llm_config.model,
    );
    Ok(())
}

pub fn render_skill_template(agent_id: &str, template: Option<&str>, llm_config: &LlmTemplateConfig) -> String {
    let (name_suffix, description, body) = match template.unwrap_or("generic") {
        "planner" => (
            "Planner",
            "Front-door lead agent for ambiguous goals.",
            "You are a planner agent. Interpret ambiguous goals, decide whether to answer directly or structure specialist work, and keep delegation explicit and auditable.",
        ),
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
      provider: "{provider}"
      model: "{model}"
      temperature: {temperature}
---
# {agent_id}

{body}
"#,
        provider = llm_config.provider,
        model = llm_config.model,
        temperature = llm_config.temperature,
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

pub fn handle_agent_presets(config_path: &Path) -> anyhow::Result<()> {
    let config = autonoetic_gateway::config::load_config(config_path)?;

    if config.llm_presets.is_empty() {
        println!("No LLM presets configured. Add presets to config.yaml:");
        println!();
        println!("llm_presets:");
        println!("  agentic:");
        println!("    provider: anthropic");
        println!("    model: claude-sonnet-4-20250514");
        println!("    temperature: 0.2");
        println!("  coding:");
        println!("    provider: anthropic");
        println!("    model: claude-sonnet-4-20250514");
        println!("    temperature: 0.1");
        println!();
        println!("Then map templates to presets:");
        println!();
        println!("llm_preset_mapping:");
        println!("  planner: agentic");
        println!("  coder: coding");
        println!("  researcher: agentic");
        return Ok(());
    }

    println!("{:<20} {:<30} {:<15} {}", "PRESET", "PROVIDER", "MODEL", "TEMP");
    println!("{}", "-".repeat(80));

    for (name, preset) in &config.llm_presets {
        let temp = preset.temperature.unwrap_or(0.0);
        println!(
            "{:<20} {:<30} {:<15} {:.1}",
            name, preset.provider, preset.model, temp
        );
    }

    if !config.llm_preset_mapping.is_empty() {
        println!();
        println!("Template → Preset mappings:");
        for (template, preset_name) in &config.llm_preset_mapping {
            println!("  {} → {}", template, preset_name);
        }
    }

    Ok(())
}

const DEFAULT_CONFIG_TEMPLATE: &str = r#"# Autonoetic Gateway Configuration
# See docs/quickstart-planner-specialist-chat.md for full documentation

agents_dir: "./agents"
port: 4000
ofp_port: 4200
tls: false
default_lead_agent_id: "planner.default"
max_concurrent_spawns: 4
max_pending_spawns_per_agent: 4
background_scheduler_enabled: false

# Agent install approval policy: always, risk_based (default), or never
# agent_install_approval_policy: risk_based

# LLM presets for role-specific model selection
# Presets are referenced by name in templates and agent init commands
llm_presets:
  agentic:
    provider: "openrouter"
    model: "google/gemini-2.5-flash-lite"
    temperature: 0.2
  coding:
    provider: "openrouter"
    model: "google/gemini-2.5-flash-lite"
    temperature: 0.1
  research:
    provider: "openrouter"
    model: "google/gemini-2.5-flash-lite"
    temperature: 0.3
  fallback:
    provider: "openai"
    model: "gpt-4o"
    temperature: 0.2

# Template → Preset mapping
# Used during 'agent bootstrap' and 'agent init --template <name>'
llm_preset_mapping:
  planner: agentic
  researcher: research
  architect: agentic
  coder: coding
  debugger: coding
  auditor: agentic
  specialized_builder: agentic
  default: agentic
"#;

pub fn handle_init_config(output: Option<&str>, overwrite: bool) -> anyhow::Result<()> {
    let output_path = output.unwrap_or("config.yaml");
    let path = std::path::Path::new(output_path);

    if path.exists() && !overwrite {
        anyhow::bail!(
            "Config file already exists at {}. Use --overwrite to replace it.",
            path.display()
        );
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    std::fs::write(path, DEFAULT_CONFIG_TEMPLATE)?;
    println!("Created config file at {}", path.display());
    println!();
    println!("Next steps:");
    println!("  1. Edit the file to set your LLM provider and API keys");
    println!("  2. Bootstrap agents: autonoetic agent bootstrap --config {}", path.display());
    println!("  3. Start gateway: autonoetic gateway start --config {}", path.display());
    println!();
    println!("Tip: Use 'autonoetic agent presets --config {}' to list configured presets.", path.display());

    Ok(())
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

pub fn handle_agent_bootstrap(
    config_path: &Path,
    from: Option<&str>,
    overwrite: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        config_path.exists(),
        "Config file not found at {}. Create it first (or pass a valid --config path) before running 'agent bootstrap'.",
        config_path.display()
    );
    let config = autonoetic_gateway::config::load_config(config_path)?;
    std::fs::create_dir_all(&config.agents_dir)?;

    let reference_root = resolve_reference_agents_dir(from)?;
    let bundles = discover_reference_bundles(&reference_root)?;
    anyhow::ensure!(
        !bundles.is_empty(),
        "No reference bundles found under {}",
        reference_root.display()
    );

    let mut copied = 0_usize;
    let mut overwritten = 0_usize;
    let mut skipped = 0_usize;

    for bundle in bundles {
        let agent_id = bundle
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid bundle directory name: {}", bundle.display()))?;
        let target_dir = config.agents_dir.join(agent_id);
        if target_dir.exists() {
            if overwrite {
                std::fs::remove_dir_all(&target_dir)?;
                copy_dir_recursive(&bundle, &target_dir)?;
                overwritten += 1;
                println!(
                    "Overwrote '{}' from {}",
                    agent_id,
                    bundle.display()
                );
            } else {
                skipped += 1;
                println!(
                    "Skipped '{}' (already exists at {})",
                    agent_id,
                    target_dir.display()
                );
            }
            continue;
        }
        copy_dir_recursive(&bundle, &target_dir)?;
        copied += 1;
        println!(
            "Installed '{}' from {}",
            agent_id,
            bundle.display()
        );
    }

    println!(
        "Bootstrap complete: {} installed, {} overwritten, {} skipped (target: {}).",
        copied,
        overwritten,
        skipped,
        config.agents_dir.display()
    );

    Ok(())
}

fn resolve_reference_agents_dir(from: Option<&str>) -> anyhow::Result<std::path::PathBuf> {
    if let Some(path) = from {
        let explicit = std::path::PathBuf::from(path);
        anyhow::ensure!(
            explicit.is_dir(),
            "Provided --from path is not a directory: {}",
            explicit.display()
        );
        return Ok(explicit);
    }

    if let Ok(path) = std::env::var("AUTONOETIC_REFERENCE_AGENTS_DIR") {
        let env_path = std::path::PathBuf::from(path);
        if env_path.is_dir() {
            return Ok(env_path);
        }
    }

    let mut candidates = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../agents")];
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("agents"));
        candidates.push(cwd.join("../agents"));
    }

    for candidate in candidates {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "Could not auto-detect reference bundles directory. Provide --from <path> or set AUTONOETIC_REFERENCE_AGENTS_DIR."
    )
}

fn discover_reference_bundles(root: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut bundles = Vec::new();
    for group in std::fs::read_dir(root)? {
        let group = group?;
        if !group.file_type()?.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(group.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let bundle_dir = entry.path();
            if bundle_dir.join("SKILL.md").exists() {
                bundles.push(bundle_dir);
            }
        }
    }
    bundles.sort();
    Ok(bundles)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(src.is_dir(), "Source is not a directory: {}", src.display());
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
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
            for msg in &request.messages {
                if msg.content.contains("sandbox command denied by ShellExec policy") {
                    anyhow::bail!("mock observed sandbox command denied by ShellExec policy");
                }
            }
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

        init_agent_scaffold(&config_path, "agent_bootstrap", Some("coder"), None, None, None)
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

    #[test]
    fn test_render_skill_template_supports_planner_template() {
        let llm = LlmTemplateConfig::default();
        let skill = render_skill_template("planner.default", Some("planner"), &llm);
        assert!(skill.contains("agent:\n      id: \"planner.default\""));
        assert!(skill.contains("Front-door lead agent for ambiguous goals."));
        assert!(skill.contains("You are a planner agent."));
    }

    #[test]
    fn test_resolve_llm_config_uses_hardcoded_defaults_for_templates() {
        let config = GatewayConfig::default();

        let llm = resolve_llm_config(&config, Some("coder"), None, None, None);
        assert_eq!(llm.provider, "anthropic");
        assert_eq!(llm.model, "claude-sonnet-4-20250514");
        assert_eq!(llm.temperature, 0.1);

        let llm = resolve_llm_config(&config, Some("planner"), None, None, None);
        assert_eq!(llm.provider, "anthropic");
        assert_eq!(llm.temperature, 0.2);
    }

    #[test]
    fn test_resolve_llm_config_uses_presets_from_config() {
        let mut config = GatewayConfig::default();
        config.llm_presets.insert("fast".to_string(), autonoetic_types::config::LlmPreset {
            provider: "openai".to_string(),
            model: "gpt-4o-mini".to_string(),
            temperature: Some(0.0),
            fallback_provider: None,
            fallback_model: None,
        });
        config.llm_preset_mapping.insert("coder".to_string(), "fast".to_string());

        let llm = resolve_llm_config(&config, Some("coder"), None, None, None);
        assert_eq!(llm.provider, "openai");
        assert_eq!(llm.model, "gpt-4o-mini");
        assert_eq!(llm.temperature, 0.0);
    }

    #[test]
    fn test_resolve_llm_config_cli_override_wins() {
        let config = GatewayConfig::default();

        let llm = resolve_llm_config(&config, Some("coder"), None, Some("google"), Some("gemini-pro"));
        assert_eq!(llm.provider, "google");
        assert_eq!(llm.model, "gemini-pro");
    }

    fn write_reference_bundle(root: &std::path::Path, group: &str, agent_id: &str, marker: &str) {
        let dir = root.join(group).join(agent_id);
        std::fs::create_dir_all(&dir).expect("bundle dir should create");
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: \"{agent_id}\"\ndescription: \"{marker}\"\nmetadata:\n  autonoetic:\n    version: \"1.0\"\n    runtime:\n      engine: \"autonoetic\"\n      gateway_version: \"0.1.0\"\n      sdk_version: \"0.1.0\"\n      type: \"stateful\"\n      sandbox: \"bubblewrap\"\n      runtime_lock: \"runtime.lock\"\n    agent:\n      id: \"{agent_id}\"\n      name: \"{agent_id}\"\n      description: \"{marker}\"\n---\n#{agent_id}\n"
            ),
        )
        .expect("skill should write");
        std::fs::write(dir.join("runtime.lock"), default_runtime_lock_contents())
            .expect("runtime.lock should write");
    }

    #[test]
    fn test_handle_agent_bootstrap_installs_reference_bundles() {
        let temp = tempdir().expect("tempdir should create");
        let reference_root = temp.path().join("reference_agents");
        write_reference_bundle(&reference_root, "lead", "planner.default", "planner");
        write_reference_bundle(&reference_root, "specialists", "coder.default", "coder");

        let config_path = temp.path().join("config.yaml");
        let agents_dir = temp.path().join("runtime_agents");
        std::fs::write(
            &config_path,
            format!(
                "agents_dir: \"{}\"\nport: 4000\nofp_port: 4200\ntls: false\n",
                agents_dir.display()
            ),
        )
        .expect("config should write");

        handle_agent_bootstrap(
            &config_path,
            Some(reference_root.to_str().expect("utf-8 path")),
            false,
        )
        .expect("bootstrap should succeed");

        assert!(agents_dir.join("planner.default").join("SKILL.md").exists());
        assert!(agents_dir.join("coder.default").join("runtime.lock").exists());
    }

    #[test]
    fn test_handle_agent_bootstrap_overwrite_behavior() {
        let temp = tempdir().expect("tempdir should create");
        let reference_root = temp.path().join("reference_agents");
        write_reference_bundle(&reference_root, "lead", "planner.default", "v1");

        let config_path = temp.path().join("config.yaml");
        let agents_dir = temp.path().join("runtime_agents");
        std::fs::write(
            &config_path,
            format!(
                "agents_dir: \"{}\"\nport: 4000\nofp_port: 4200\ntls: false\n",
                agents_dir.display()
            ),
        )
        .expect("config should write");

        handle_agent_bootstrap(
            &config_path,
            Some(reference_root.to_str().expect("utf-8 path")),
            false,
        )
        .expect("first bootstrap should succeed");

        let installed_path = agents_dir.join("planner.default").join("SKILL.md");
        let first = std::fs::read_to_string(&installed_path).expect("installed skill should read");
        assert!(first.contains("description: \"v1\""));

        write_reference_bundle(&reference_root, "lead", "planner.default", "v2");
        handle_agent_bootstrap(
            &config_path,
            Some(reference_root.to_str().expect("utf-8 path")),
            false,
        )
        .expect("second bootstrap should succeed");
        let second = std::fs::read_to_string(&installed_path).expect("installed skill should read");
        assert!(second.contains("description: \"v1\""));

        handle_agent_bootstrap(
            &config_path,
            Some(reference_root.to_str().expect("utf-8 path")),
            true,
        )
        .expect("overwrite bootstrap should succeed");
        let third = std::fs::read_to_string(&installed_path).expect("installed skill should read");
        assert!(third.contains("description: \"v2\""));
    }

    #[test]
    fn test_handle_agent_bootstrap_requires_existing_config_file() {
        let temp = tempdir().expect("tempdir should create");
        let config_path = temp.path().join("missing-config.yaml");
        let reference_root = temp.path().join("reference_agents");
        write_reference_bundle(&reference_root, "lead", "planner.default", "planner");

        let err = handle_agent_bootstrap(
            &config_path,
            Some(reference_root.to_str().expect("utf-8 path")),
            false,
        )
        .expect_err("missing config should fail fast");
        assert!(err.to_string().contains("Config file not found"));
    }
}
