//! CCOS Agent Runtime (The Deputy)
//!
//! A jailed agent that:
//! - Connects to the Gateway (Sheriff) with a token
//! - Polls for events/messages
//! - Runs LLM loop to process messages
//! - Executes capabilities through the Gateway
//!
//! Usage:
//!   ccos-agent --token <TOKEN> --gateway-url <URL> --session-id <ID>

use ccos::chat::agent_llm::{AgentLlmClient, LlmConfig};
use ccos::config::types::AgentConfig;
use ccos::secrets::SecretStore;
use ccos::utils::fs::get_workspace_root;
use chrono::Utc;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{error, info, warn};

use ccos::utils::log_redaction::{redact_json_for_logs, redact_text_for_logs};

/// Agent CLI arguments
#[derive(Parser, Debug)]
#[command(name = "ccos-agent")]
#[command(about = "CCOS Agent Runtime - The Deputy")]
struct Args {
    /// Path to agent configuration file (TOML format)
    #[arg(long, env = "CCOS_AGENT_CONFIG_PATH")]
    config_path: Option<String>,

    /// Authentication token from the Gateway
    #[arg(long, env = "CCOS_AGENT_TOKEN")]
    token: String,

    /// Gateway URL
    #[arg(
        long,
        env = "CCOS_GATEWAY_URL",
        default_value = "http://localhost:8822"
    )]
    gateway_url: String,

    /// Session ID assigned by the Gateway
    #[arg(long, env = "CCOS_SESSION_ID")]
    session_id: String,

    /// Optional Run ID for correlation (provided by Gateway when using /chat/run)
    #[arg(long, env = "CCOS_RUN_ID")]
    run_id: Option<String>,

    /// Agent ID (from config or env)
    #[arg(long, env = "CCOS_AGENT_ID")]
    agent_id: Option<String>,

    /// Agent display name (optional)
    #[arg(long, env = "CCOS_AGENT_NAME")]
    agent_name: Option<String>,

    /// Polling interval in milliseconds
    #[arg(long, default_value = "1000")]
    poll_interval_ms: u64,

    /// Enable LLM processing (requires API key)
    #[arg(long, env = "CCOS_AGENT_ENABLE_LLM", default_value = "false")]
    enable_llm: bool,

    /// LLM Profile to use (e.g., "google:gemini-1.5-pro")
    #[arg(long, env = "CCOS_LLM_PROFILE")]
    llm_profile: Option<String>,

    /// LLM Provider (openai, anthropic, local)
    #[arg(long, env = "CCOS_LLM_PROVIDER")]
    llm_provider: Option<String>,

    /// LLM API key
    #[arg(long, env = "CCOS_LLM_API_KEY")]
    llm_api_key: Option<String>,

    /// LLM Model
    #[arg(long, env = "CCOS_LLM_MODEL")]
    llm_model: Option<String>,

    /// LLM Base URL
    #[arg(long, env = "CCOS_LLM_BASE_URL")]
    llm_base_url: Option<String>,

    /// Allowlist of agent context keys to share with LLM (comma-separated)
    #[arg(long, env = "CCOS_LLM_CONTEXT_ALLOWLIST", value_delimiter = ',')]
    llm_context_allowlist: Option<Vec<String>>,

    // ── Budget Enforcement ──────────────────────────────────────────────────
    /// Maximum steps (actions) the agent can execute before pausing (0 = unlimited)
    #[arg(long, env = "CCOS_AGENT_MAX_STEPS", default_value = "0")]
    max_steps: u32,

    /// Maximum duration in seconds the agent can run before pausing (0 = unlimited)
    #[arg(long, env = "CCOS_AGENT_MAX_DURATION_SECS", default_value = "0")]
    max_duration_secs: u64,

    /// Budget policy: "hard_stop" (exit) or "pause_approval" (wait for approval)
    #[arg(
        long,
        env = "CCOS_AGENT_BUDGET_POLICY",
        default_value = "pause_approval"
    )]
    budget_policy: String,

    /// Persist discovered per-skill bearer tokens to .ccos/secrets.toml (opt-in).
    #[arg(long, env = "CCOS_AGENT_PERSIST_SKILL_SECRETS", default_value = "false")]
    persist_skill_secrets: bool,

    /// Known skill URL hints (repeatable). Format: "name=url".
    /// When the user mentions a skill by name the agent will use the hinted URL
    /// instead of asking. Example: --skill-url-hint "moltbook=http://localhost:8765/skill.md"
    #[arg(long, env = "CCOS_SKILL_URL_HINTS", value_delimiter = ',')]
    skill_url_hint: Option<Vec<String>>,
}

impl Args {
    /// Merge with config file if --config-path is provided
    /// CLI arguments take precedence over config file values
    fn merge_with_config(mut self) -> anyhow::Result<Self> {
        if let Some(config_path) = &self.config_path {
            match load_agent_config(config_path) {
                Ok(config) => {
                    info!("Loaded agent configuration from: {}", config_path);

                    if self.agent_id.is_none() {
                        self.agent_id = Some(config.agent_id.clone());
                    }
                    if self.agent_name.is_none() {
                        self.agent_name = config.agent_name.clone();
                    }
                    if self.llm_context_allowlist.is_none() {
                        self.llm_context_allowlist = config.llm_context_allowlist.clone();
                    }

                    // Apply config values only if CLI args are at defaults
                    // Gateway URL - could be derived from network config if needed
                    // For now, keep CLI default or use env var

                    // Select the profile to use
                    let profile_to_find = if self.llm_profile.is_some() {
                        self.llm_profile.as_deref()
                    } else {
                        config
                            .llm_profiles
                            .as_ref()
                            .and_then(|p| p.default.as_deref())
                    };

                    // Apply config values only if CLI args are at defaults
                    if let Some(profile_name) = profile_to_find {
                        if let Some(profile) = find_llm_profile(&config, profile_name) {
                            info!("Using LLM profile: {}", profile_name);

                            // Priority: CLI > Config Profile > Default
                            if self.llm_provider.is_none() {
                                self.llm_provider = Some(profile.provider.clone());
                            }
                            if self.llm_model.is_none() {
                                self.llm_model = Some(profile.model.clone());
                            }
                            if self.llm_base_url.is_none() {
                                self.llm_base_url = profile.base_url.clone();
                            }
                            // API key from environment variable specified in config
                            if let Some(ref api_key_env) = profile.api_key_env {
                                if self.llm_api_key.is_none() {
                                    self.llm_api_key = std::env::var(api_key_env).ok();
                                }
                            }
                        } else {
                            warn!("LLM profile not found in config: {}", profile_name);
                        }
                    }

                    // Enable LLM if provider is config-set (even if not CLI-set) and enabled in generic config
                    if !self.enable_llm && config.capabilities.llm.enabled {
                        // If we found a provider from config, we can enable LLM
                        if self.llm_provider.is_some() {
                            self.enable_llm = true;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to load config from {}: {}", config_path, e);
                    // Continue with CLI args only
                }
            }
        }
        Ok(self)
    }
}

/// Load agent configuration from TOML file
fn load_agent_config(config_path: &str) -> anyhow::Result<AgentConfig> {
    let path = Path::new(config_path);
    let actual_path = if path.exists() {
        path.to_path_buf()
    } else {
        let parent_path = Path::new("..").join(config_path);
        if parent_path.exists() {
            parent_path
        } else {
            return Err(anyhow::anyhow!(
                "Config file not found: '{}' (also tried '../{}'). Run from the workspace root directory.",
                config_path, config_path
            ));
        }
    };

    let mut content = std::fs::read_to_string(&actual_path).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read config file '{}': {}",
            actual_path.display(),
            e
        )
    })?;

    // Skip "# RTFS" header if present
    if content.starts_with("# RTFS") {
        content = content.lines().skip(1).collect::<Vec<_>>().join("\n");
    }

    toml::from_str(&content).map_err(|e| anyhow::anyhow!("Failed to parse agent config: {}", e))
}

/// Find an LLM profile by name in the agent config
fn find_llm_profile(
    config: &AgentConfig,
    profile_name: &str,
) -> Option<ccos::config::types::LlmProfile> {
    let llm_profiles = config.llm_profiles.as_ref()?;

    // Check explicit profiles first
    if let Some(profile) = llm_profiles
        .profiles
        .iter()
        .find(|p| p.name == profile_name)
    {
        return Some(profile.clone());
    }

    // Check model sets (e.g., "openrouter_free:fast")
    if let Some((set_name, model_name)) = profile_name.split_once(':') {
        if let Some(ref model_sets) = llm_profiles.model_sets {
            if let Some(model_set) = model_sets.iter().find(|s| s.name == set_name) {
                if let Some(model) = model_set.models.iter().find(|m| m.name == model_name) {
                    return Some(ccos::config::types::LlmProfile {
                        name: profile_name.to_string(),
                        provider: model_set.provider.clone(),
                        model: model.model.clone(),
                        api_key_env: model_set.api_key_env.clone(),
                        api_key: model_set.api_key.clone(),
                        base_url: model_set.base_url.clone(),
                        temperature: None,
                        max_tokens: model.max_output_tokens,
                    });
                }
            }
        }
    }

    None
}

/// Event from the Gateway
#[derive(Debug, Deserialize, Clone)]
struct ChatEvent {
    #[allow(dead_code)]
    id: String,
    channel_id: String,
    content: String,
    sender: String,
    #[allow(dead_code)]
    timestamp: String,
}

/// Events response from /chat/events
#[derive(Debug, Deserialize)]
struct EventsResponse {
    messages: Vec<ChatEvent>,
    #[allow(dead_code)]
    has_more: bool,
}

/// Execute request for /chat/execute
#[derive(Debug, Serialize)]
struct ExecuteRequest {
    capability_id: String,
    inputs: serde_json::Value,
    session_id: String,
}

/// Execute response
#[derive(Debug, Deserialize, Serialize)]
struct ExecuteResponse {
    success: bool,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

/// Capability info from Gateway
#[derive(Debug, Deserialize, Clone)]
struct CapabilityInfo {
    id: String,
    #[allow(dead_code)]
    name: String,
    description: String,
    version: String,
    #[serde(default)]
    input_schema: Option<serde_json::Value>,
}

/// Capabilities response
#[derive(Debug, Deserialize)]
struct CapabilitiesListResponse {
    capabilities: Vec<CapabilityInfo>,
}

/// Agent Runtime state
struct AgentRuntime {
    args: Args,
    client: Client,
    message_history: Vec<ChatEvent>,
    llm_client: Option<AgentLlmClient>,
    capabilities: Vec<String>,
    last_loaded_skill_id: Option<String>,
    loaded_skill_urls: HashSet<String>,
    loaded_skill_ids: HashSet<String>,
    loaded_skill_capabilities: HashSet<String>,
    // Ephemeral per-skill bearer tokens (e.g. secrets returned by register-agent during onboarding).
    // This avoids prompting the LLM with secrets while still enabling authenticated follow-up calls.
    skill_bearer_tokens: HashMap<String, String>,
    capability_required: HashMap<String, Vec<String>>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    llm_context_allowlist: Vec<String>,
    /// Parsed skill URL hints: name -> url
    skill_url_hints: HashMap<String, String>,
    /// Loaded skill definitions (raw content): skill_id -> definition text
    loaded_skill_definitions: HashMap<String, String>,
    // ── Budget Tracking ─────────────────────────────────────────────────────
    /// Number of actions executed since agent start
    step_count: u32,
    /// Timestamp when the agent started
    start_time: Instant,
    /// Whether the agent is paused due to budget exhaustion
    budget_paused: bool,
}

impl AgentRuntime {
    fn new(args: Args) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        let agent_id = args.agent_id.clone();
        let agent_name = args.agent_name.clone();
        let llm_context_allowlist = args.llm_context_allowlist.clone().unwrap_or_else(|| {
            vec![
                "agent_id".to_string(),
                "agent_name".to_string(),
                "llm_model".to_string(),
            ]
        });

        // Initialize LLM client if enabled
        let llm_client = if args.enable_llm {
            if let Some(api_key) = &args.llm_api_key {
                // Apply defaults if still missing after config merge
                let provider = args
                    .llm_provider
                    .clone()
                    .unwrap_or_else(|| "local".to_string());
                let model = args
                    .llm_model
                    .clone()
                    .unwrap_or_else(|| "gpt-3.5-turbo".to_string());

                let llm_config = LlmConfig {
                    provider: provider.clone(),
                    api_key: api_key.clone(),
                    model: model.clone(),
                    base_url: args.llm_base_url.clone(),
                };
                match AgentLlmClient::new(llm_config) {
                    Ok(client) => {
                        info!("LLM client initialized with provider: {}", provider);
                        Some(client)
                    }
                    Err(e) => {
                        warn!("Failed to initialize LLM client: {}", e);
                        None
                    }
                }
            } else {
                warn!("LLM enabled but no API key provided. Use --llm-api-key or CCOS_LLM_API_KEY env var");
                None
            }
        } else {
            None
        };

        // Parse --skill-url-hint entries ("name=url")
        let skill_url_hints: HashMap<String, String> = args
            .skill_url_hint
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter_map(|entry| {
                let parts: Vec<&str> = entry.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].to_string(), parts[1].to_string()))
                } else {
                    warn!("Ignoring malformed --skill-url-hint: {}", entry);
                    None
                }
            })
            .collect();
        if !skill_url_hints.is_empty() {
            info!("Loaded {} skill URL hint(s)", skill_url_hints.len());
        }

        Self {
            args,
            client,
            message_history: Vec::new(),
            llm_client,
            capabilities: Vec::new(),
            last_loaded_skill_id: None,
            loaded_skill_urls: HashSet::new(),
            loaded_skill_ids: HashSet::new(),
            loaded_skill_capabilities: HashSet::new(),
            skill_bearer_tokens: HashMap::new(),
            capability_required: HashMap::new(),
            agent_id,
            agent_name,
            llm_context_allowlist,
            skill_url_hints,
            loaded_skill_definitions: HashMap::new(),
            // Budget tracking
            step_count: 0,
            start_time: Instant::now(),
            budget_paused: false,
        }
    }

    // ── Budget Enforcement Helpers ──────────────────────────────────────────

    /// Check if the agent has exceeded its budget
    /// Returns (exceeded, reason) tuple
    fn check_budget(&self) -> (bool, Option<String>) {
        // Check step limit
        if self.args.max_steps > 0 && self.step_count >= self.args.max_steps {
            return (
                true,
                Some(format!(
                    "Step limit exceeded: {}/{}",
                    self.step_count, self.args.max_steps
                )),
            );
        }

        // Check duration limit
        if self.args.max_duration_secs > 0 {
            let elapsed = self.start_time.elapsed().as_secs();
            if elapsed >= self.args.max_duration_secs {
                return (
                    true,
                    Some(format!(
                        "Duration limit exceeded: {}s/{}s",
                        elapsed, self.args.max_duration_secs
                    )),
                );
            }
        }

        (false, None)
    }

    /// Increment step counter and return the new count
    fn increment_step(&mut self) -> u32 {
        self.step_count += 1;
        self.step_count
    }

    /// Handle budget exhaustion according to policy
    /// Returns true if the action should be skipped (budget exceeded), false otherwise
    async fn handle_budget_exceeded(&mut self, reason: &str, event: &ChatEvent) -> bool {
        warn!("[Budget] {}", reason);

        if self.args.budget_policy == "hard_stop" {
            error!("[Budget] Hard stop: agent shutting down due to budget exhaustion");
            // Send a message to the user before stopping
            let _ = self
                .send_response(
                    event,
                    &format!("⚠️ Budget exceeded: {}. Agent is stopping.", reason),
                )
                .await;
            let _ = self
                .transition_run_state("Failed", Some(reason))
                .await;
            // Exit the process
            std::process::exit(0);
        } else {
            // pause_approval: set paused flag and notify
            self.budget_paused = true;
            info!("[Budget] Paused: waiting for approval to continue");
            let _ = self.send_response(
                event,
                &format!(
                    "⏸️ Budget limit reached: {}. Agent is paused. Reply 'continue' to resume with a new budget.",
                    reason
                ),
            ).await;
            let _ = self
                .transition_run_state("PausedApproval", Some(reason))
                .await;
            true // skip further actions
        }
    }

    /// Main agent loop
    async fn run(&mut self) -> anyhow::Result<()> {
        info!(
            "Starting CCOS Agent for session {} connected to {}",
            self.args.session_id, self.args.gateway_url
        );

        // Verify connection to Gateway
        self.verify_connection().await?;

        // Fetch available capabilities
        self.fetch_capabilities().await?;

        // Main event loop
        loop {
            match self.poll_events().await {
                Ok(events) => {
                    for event in events {
                        self.process_event(event).await?;
                    }
                }
                Err(e) => {
                    warn!("Failed to poll events: {}", e);
                }
            }

            // Wait before next poll
            sleep(Duration::from_millis(self.args.poll_interval_ms)).await;
        }
    }

    /// Verify connection to Gateway
    async fn verify_connection(&self) -> anyhow::Result<()> {
        let url = format!("{}/chat/health", self.args.gateway_url);
        let response = self
            .client
            .get(&url)
            .header("X-Agent-Token", &self.args.token)
            .send()
            .await?;

        if response.status().is_success() {
            info!("Connected to Gateway successfully");
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Gateway health check failed: {}",
                response.status()
            ))
        }
    }

    /// Fetch available capabilities from Gateway
    async fn fetch_capabilities(&mut self) -> anyhow::Result<()> {
        let url = format!("{}/chat/capabilities", self.args.gateway_url);
        let response = self
            .client
            .get(&url)
            .header("X-Agent-Token", &self.args.token)
            .send()
            .await?;

        if response.status().is_success() {
            let caps_resp: CapabilitiesListResponse = response.json().await?;
            self.capability_required.clear();
            self.capabilities = caps_resp
                .capabilities
                .into_iter()
                .filter(|c| {
                    c.id != "ccos.chat.egress.prepare_outbound"
                        && !c.id.starts_with("ccos.chat.transform.")
                })
                .map(|c| {
                    if let Some(schema) = &c.input_schema {
                        if let Some(required) = Self::extract_required_fields(schema) {
                            if !required.is_empty() {
                                self.capability_required.insert(c.id.clone(), required);
                            }
                        }
                    }
                    if let Some(schema) = &c.input_schema {
                        format!(
                            "- {} ({}) - {} | inputs: {}",
                            c.id, c.version, c.description, schema
                        )
                    } else {
                        format!("- {} ({}) - {}", c.id, c.version, c.description)
                    }
                })
                .collect();
            info!(
                "Fetched {} capabilities from Gateway",
                self.capabilities.len()
            );
            Ok(())
        } else {
            warn!("Failed to fetch capabilities: {}", response.status());
            // Don't fail completely, just traverse with empty capabilities
            Ok(())
        }
    }

    /// Poll for new events from the Gateway
    async fn poll_events(&self) -> anyhow::Result<Vec<ChatEvent>> {
        let url = format!(
            "{}/chat/events/{}",
            self.args.gateway_url, self.args.session_id
        );

        let response = self
            .client
            .get(&url)
            .header("X-Agent-Token", &self.args.token)
            .send()
            .await?;

        if response.status().is_success() {
            let events_resp: EventsResponse = response.json().await?;
            Ok(events_resp.messages)
        } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            Err(anyhow::anyhow!("Authentication failed - invalid token"))
        } else {
            Err(anyhow::anyhow!(
                "Failed to poll events: {}",
                response.status()
            ))
        }
    }

    /// Process a single event
    async fn process_event(&mut self, event: ChatEvent) -> anyhow::Result<()> {
        let redacted_preview = redact_text_for_logs(&event.content);
        info!(
            "[Agent] Received event from {} (ID: {}): {}",
            event.sender,
            event.id,
            &redacted_preview[..50.min(redacted_preview.len())]
        );

        // Store in history
        self.message_history.push(event.clone());

        // Check if the agent is paused and the user sent "continue"
        if self.budget_paused {
            let content_lower = event.content.to_lowercase();
            if content_lower.contains("continue") || content_lower.contains("resume") {
                info!("[Budget] Resuming: user sent continue command");
                self.budget_paused = false;
                // Reset counters for new budget window
                self.step_count = 0;
                self.start_time = Instant::now();
                let _ = self
                    .send_response(&event, "▶️ Resuming! Budget has been reset.")
                    .await;
                let _ = self.transition_run_state("Active", Some("resumed by user")).await;
                return Ok(());
            } else {
                // Agent is paused, only respond to "continue"
                let _ = self
                    .send_response(
                        &event,
                        "⏸️ I'm currently paused due to budget limits. Reply 'continue' to resume.",
                    )
                    .await;
                return Ok(());
            }
        }

        if let Some(_) = &self.llm_client {
            self.process_with_llm(event).await?;
        } else {
            self.send_simple_echo(&event).await?;
        }

        Ok(())
    }

    async fn send_response(&self, event: &ChatEvent, text: &str) -> anyhow::Result<()> {
        let mut inputs = serde_json::Map::new();
        inputs.insert("content".to_string(), serde_json::json!(text));
        inputs.insert(
            "channel_id".to_string(),
            serde_json::json!(event.channel_id),
        );
        inputs.insert(
            "session_id".to_string(),
            serde_json::json!(self.args.session_id),
        );
        inputs.insert(
            "run_id".to_string(),
            serde_json::json!(
                self.args
                    .run_id
                    .clone()
                    .unwrap_or_else(|| format!("resp-{}", event.id))
            ),
        );
        inputs.insert(
            "step_id".to_string(),
            serde_json::json!(format!("resp-step-{}", event.id)),
        );

        inputs.insert("content_class".to_string(), serde_json::json!("public"));

        let _ = self
            .execute_capability(
                "ccos.chat.egress.send_outbound",
                serde_json::Value::Object(inputs),
            )
            .await;
        Ok(())
    }

    /// Record a synthetic message in the conversation history so the LLM
    /// sees agent responses and action results on subsequent turns.
    fn record_in_history(&mut self, sender: &str, content: String, channel_id: &str) {
        self.message_history.push(ChatEvent {
            id: format!("synth-{}", uuid::Uuid::new_v4()),
            channel_id: channel_id.to_string(),
            content,
            sender: sender.to_string(),
            timestamp: Utc::now().to_rfc3339(),
        });
    }

    async fn send_simple_echo(&self, event: &ChatEvent) -> anyhow::Result<()> {
        self.send_response(
            event,
            &format!("I am a simple agent. You said: {}", event.content),
        )
        .await
    }

    async fn transition_run_state(
        &self,
        new_state: &str,
        reason: Option<&str>,
    ) -> anyhow::Result<()> {
        let Some(run_id) = &self.args.run_id else {
            return Ok(());
        };

        let url = format!(
            "{}/chat/run/{}/transition",
            self.args.gateway_url.trim_end_matches('/'),
            run_id
        );
        let mut body = serde_json::json!({ "new_state": new_state });
        if let Some(reason) = reason {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("reason".to_string(), serde_json::json!(reason));
            }
        }

        // Best-effort: run transitions are advisory; don't fail the agent loop if they fail.
        let resp = self
            .client
            .post(url)
            .header("X-Agent-Token", &self.args.token)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("run transition failed: {} {}", status, text);
        }

        Ok(())
    }

    async fn fetch_run_completion_predicate(&self) -> anyhow::Result<Option<String>> {
        let Some(run_id) = &self.args.run_id else {
            return Ok(None);
        };

        let url = format!(
            "{}/chat/run/{}",
            self.args.gateway_url.trim_end_matches('/'),
            run_id
        );
        let resp = self
            .client
            .get(url)
            .header("X-Agent-Token", &self.args.token)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        Ok(json
            .get("completion_predicate")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()))
    }

    /// Process message using LLM
    async fn process_with_llm(&mut self, event: ChatEvent) -> anyhow::Result<()> {
        if let Some(llm) = &self.llm_client {
            info!(
                "Processing message with LLM: {}",
                redact_text_for_logs(&event.content)
            );

            // Build context from message history
            let context: Vec<String> = self
                .message_history
                .iter()
                .map(|e| format!("{}: {}", e.sender, e.content))
                .collect();

            let agent_context = self.build_llm_context();

            // Process with LLM
            match llm
                .process_message(&event.content, &context, &self.capabilities, &agent_context)
                .await
            {
                Ok(plan) => {
                    info!("LLM understanding: {}", plan.understanding);
                    info!("Planned {} actions", plan.actions.len());

                    // Hybrid: send immediate response first (what we're about to do).
                    if !plan.response.is_empty() {
                        self.send_response(&event, &plan.response).await?;
                        self.record_in_history("agent", plan.response.clone(), &event.channel_id);
                    }

                    // Collect follow-up events (e.g., continue after skill load)
                    let mut follow_up_events: Vec<ChatEvent> = Vec::new();
                    let mut failed_actions: Vec<String> = Vec::new();
                    let mut completed_actions: Vec<String> = Vec::new();
                    let mut missing_inputs: Vec<(String, Vec<String>)> = Vec::new();
                    let mut skill_load_failures: Vec<String> = Vec::new();
                    // User-facing messages extracted from action results (e.g., verification tweets, instructions).
                    let mut user_facing_messages: Vec<String> = Vec::new();

                    // Execute planned actions through Gateway
                    for (idx, action) in plan.actions.iter().enumerate() {
                        // ── Budget Check ────────────────────────────────────
                        let (exceeded, reason) = self.check_budget();
                        if exceeded {
                            if let Some(reason) = reason {
                                if self.handle_budget_exceeded(&reason, &event).await {
                                    // Skip remaining actions
                                    failed_actions.push(format!(
                                        "{}: skipped (budget exceeded)",
                                        action.capability_id
                                    ));
                                    break;
                                }
                            }
                        }

                        info!(
                            "Executing action {}/{}: {}",
                            idx + 1,
                            plan.actions.len(),
                            action.capability_id
                        );
                        info!("Reasoning: {}", action.reasoning);

                        if action.capability_id == "ccos.skill.load" {
                            let existing_url = action
                                .inputs
                                .get("url")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            if let Some(url) = existing_url {
                                if self.loaded_skill_urls.contains(&url) {
                                    completed_actions.push(format!(
                                        "ccos.skill.load (skipped, already loaded url: {})",
                                        url
                                    ));

                                    let known_skill_id = self
                                        .last_loaded_skill_id
                                        .clone()
                                        .unwrap_or_else(|| "unknown".to_string());
                                    let caps = if self.loaded_skill_capabilities.is_empty() {
                                        "<none>".to_string()
                                    } else {
                                        let mut caps: Vec<String> = self
                                            .loaded_skill_capabilities
                                            .iter()
                                            .cloned()
                                            .collect();
                                        caps.sort();
                                        caps.join(", ")
                                    };
                                    let follow_up_content = format!(
                                        "The skill is already loaded. Skill ID: {}\n\nRegistered capabilities:\n{}\n\nPlease continue with onboarding using ccos.skill.execute for the steps required.",
                                        known_skill_id, caps
                                    );
                                    let follow_up_event = ChatEvent {
                                        id: format!("followup-{}", event.id),
                                        channel_id: event.channel_id.clone(),
                                        content: follow_up_content,
                                        sender: "system".to_string(),
                                        timestamp: Utc::now().to_rfc3339(),
                                    };
                                    follow_up_events.push(follow_up_event);
                                    continue;
                                }
                            }
                        }

                        // Add session/run/step IDs to inputs
                        let mut inputs = action.inputs.clone();
                        let mut skip_action = false;
                        if let Some(obj) = inputs.as_object_mut() {
                            obj.insert(
                                "session_id".to_string(),
                                serde_json::json!(self.args.session_id),
                            );
                            let run_id = obj
                                .get("run_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .or_else(|| self.args.run_id.clone())
                                .unwrap_or_else(|| format!("run-{}-{}", event.id, idx));
                            obj.insert("run_id".to_string(), serde_json::json!(run_id.clone()));

                            let step_id = obj
                                .get("step_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| format!("{}-step-{}-{}", run_id, event.id, idx));
                            obj.insert("step_id".to_string(), serde_json::json!(step_id));

                            // AUTO-FIX: Mark all LLM-planned outbound messages as public for egress
                            if action.capability_id == "ccos.chat.egress.send_outbound"
                                || action.capability_id == "ccos.chat.egress.prepare_outbound"
                            {
                                obj.insert(
                                    "content_class".to_string(),
                                    serde_json::json!("public"),
                                );
                            }

                            if action.capability_id == "ccos.skill.execute" {
                                if let Some(last_skill_id) = &self.last_loaded_skill_id {
                                    let skill_value = obj.get("skill").and_then(|v| v.as_str());
                                    if skill_value.is_none() {
                                        obj.insert(
                                            "skill".to_string(),
                                            serde_json::json!(last_skill_id),
                                        );
                                    }
                                }

                                // Guardrail: if the LLM "asked a question" but still planned a skill.execute
                                // without specifying an operation, don't execute (it will always fail).
                                // We'll just rely on the natural-language response we already sent.
                                if obj.get("operation").and_then(|v| v.as_str()).is_none() {
                                    warn!(
                                        "Skipping ccos.skill.execute: missing operation (LLM plan likely needs clarification)"
                                    );
                                    skip_action = true;
                                }

                                let operation = obj.get("operation").and_then(|v| v.as_str());
                                let skill = obj.get("skill").and_then(|v| v.as_str());
                                let cap_id = match (operation, skill) {
                                    (Some(op), _) if op.contains('.') => Some(op.to_string()),
                                    (Some(op), Some(sk)) => Some(format!("{}.{}", sk, op)),
                                    _ => None,
                                };

                                if let Some(cap_id) = cap_id {
                                    if let Some(required) =
                                        self.capability_required.get(&cap_id).cloned()
                                    {
                                        let params_value = obj
                                            .entry("params".to_string())
                                            .or_insert_with(|| serde_json::json!({}));
                                        if let Some(pmap) = params_value.as_object_mut() {
                                            for field in &required {
                                                if !pmap.contains_key(field) {
                                                    if let Some(value) =
                                                        self.autofill_value_for_field(field)
                                                    {
                                                        pmap.insert(
                                                            field.to_string(),
                                                            serde_json::json!(value),
                                                        );
                                                    }
                                                }
                                            }
                                            let missing = required
                                                .into_iter()
                                                .filter(|k| !pmap.contains_key(k))
                                                .collect::<Vec<_>>();
                                            if !missing.is_empty() {
                                                missing_inputs.push((cap_id.clone(), missing));
                                                failed_actions.push(format!(
                                                    "{}: missing required inputs",
                                                    cap_id
                                                ));
                                                skip_action = true;
                                            }
                                        }
                                    }
                                }

                                // Best-effort: if we already have a bearer token for this skill,
                                // inject it into params.headers.Authorization unless the LLM provided one.
                                self.maybe_inject_wrapper_skill_auth(obj);
                            }
                        }

                        if skip_action {
                            continue;
                        }

                        // Guardrail: if the LLM planned an action but left required
                        // inputs as null, it means it doesn't have the value yet (it
                        // likely asked the user in the response). Skip the action and
                        // treat the null fields as missing inputs that need user input.
                        if let Some(obj) = inputs.as_object() {
                            let skip_keys = ["session_id", "run_id", "step_id", "content_class"];
                            let mut null_fields: Vec<String> = obj
                                .iter()
                                .filter(|(k, v)| v.is_null() && !skip_keys.contains(&k.as_str()))
                                .map(|(k, _)| k.clone())
                                .collect();
                            // Also check inside nested "params" (used by ccos.skill.execute).
                            if let Some(params) = obj.get("params").and_then(|v| v.as_object()) {
                                for (k, v) in params {
                                    if v.is_null() && !skip_keys.contains(&k.as_str()) {
                                        null_fields.push(format!("params.{}", k));
                                    }
                                }
                            }
                            if !null_fields.is_empty() {
                                warn!(
                                    "Skipping {}: LLM left required inputs as null: {}",
                                    action.capability_id,
                                    null_fields.join(", ")
                                );
                                missing_inputs.push((
                                    action.capability_id.clone(),
                                    null_fields,
                                ));
                                continue;
                            }
                        }

                        // Best-effort: if a direct skill capability is being called (skill.op),
                        // inject Authorization header from cached onboarding secret.
                        Self::maybe_inject_direct_skill_auth(
                            &self.skill_bearer_tokens,
                            &action.capability_id,
                            &mut inputs,
                        );

                        match self.execute_capability(&action.capability_id, inputs).await {
                            Ok(response) => {
                                if !response.success {
                                    let error_msg = response
                                        .error
                                        .clone()
                                        .or_else(|| {
                                            response.result.as_ref().and_then(|r| {
                                                r.get("error")
                                                    .and_then(|v| v.as_str())
                                                    .map(|s| s.to_string())
                                            })
                                        })
                                        .unwrap_or_else(|| "Unknown error".to_string());
                                    if let Some((cap_id, missing)) =
                                        Self::parse_missing_inputs(&error_msg)
                                    {
                                        missing_inputs.push((cap_id, missing));
                                    } else {
                                        let missing = Self::extract_missing_fields(&error_msg);
                                        if !missing.is_empty() {
                                            missing_inputs
                                                .push((action.capability_id.clone(), missing));
                                        }
                                    }
                                    if action.capability_id == "ccos.skill.load" {
                                        skill_load_failures.push(error_msg.clone());
                                    }
                                    warn!("Action {} failed: {}", action.capability_id, error_msg);
                                    failed_actions
                                        .push(format!("{}: {}", action.capability_id, error_msg));
                                    self.record_in_history(
                                        "system",
                                        format!("[action failed] {}: {}", action.capability_id, error_msg),
                                        &event.channel_id,
                                    );
                                    continue;
                                }
                                let redacted_response = serde_json::to_value(&response)
                                    .ok()
                                    .map(|value| redact_json_for_logs(&value).to_string())
                                    .unwrap_or_else(|| "<non-serializable response>".to_string());
                                info!(
                                    "Action {} succeeded: {}",
                                    action.capability_id, redacted_response
                                );
                                let mut action_summary = action.capability_id.clone();
                                if action.capability_id == "ccos.skill.load" && response.success {
                                    if let Some(result) = &response.result {
                                        if let Some(skill_id) = result.get("skill_id") {
                                            action_summary =
                                                format!("ccos.skill.load (skill_id: {})", skill_id);
                                        }
                                    }
                                }
                                if action.capability_id == "ccos.skill.execute" && response.success
                                {
                                    if let Some(result) = &response.result {
                                        if let Some(body) =
                                            result.get("body").and_then(|v| v.as_str())
                                        {
                                            if let Ok(json) =
                                                serde_json::from_str::<serde_json::Value>(body)
                                            {
                                                if let Some(agent_id) =
                                                    json.get("agent_id").and_then(|v| v.as_str())
                                                {
                                                    action_summary = format!(
                                                        "ccos.skill.execute (registered agent_id: {})",
                                                        agent_id
                                                    );
                                                }

                                                // Cache onboarding secret (if any) for subsequent authenticated calls.
                                                if let Some(secret) =
                                                    json.get("secret").and_then(|v| v.as_str())
                                                {
                                                    if let Some(skill_id) = action
                                                        .inputs
                                                        .get("skill")
                                                        .and_then(|v| v.as_str())
                                                    {
                                                        self.skill_bearer_tokens.insert(
                                                            skill_id.to_string(),
                                                            secret.to_string(),
                                                        );

                                                        if self.args.persist_skill_secrets {
                                                            let key = Self::secret_store_key_for_skill(skill_id);
                                                            match SecretStore::new(Some(get_workspace_root())) {
                                                                Ok(mut store) => {
                                                                    if let Err(e) = store.set_local(&key, secret.to_string()) {
                                                                        warn!("Failed to persist skill token to SecretStore ({}): {}", key, e);
                                                                    }
                                                                }
                                                                Err(e) => {
                                                                    warn!("Failed to open SecretStore for persistence: {}", e);
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                // Extract user-facing messages from the response body.
                                // Many skill operations return a JSON body with "message",
                                // "verification_tweet_text", or similar fields that the user
                                // needs to see and act on.
                                if response.success {
                                    if let Some(result) = &response.result {
                                        if let Some(body_str) =
                                            result.get("body").and_then(|v| v.as_str())
                                        {
                                            if let Ok(body_json) =
                                                serde_json::from_str::<serde_json::Value>(body_str)
                                            {
                                                // Collect all string fields that look user-facing.
                                                let mut parts = Vec::new();
                                                if let Some(msg) =
                                                    body_json.get("message").and_then(|v| v.as_str())
                                                {
                                                    parts.push(msg.to_string());
                                                }
                                                // Surface any field containing instructions/text for the user
                                                for key in &[
                                                    "verification_tweet_text",
                                                    "instructions",
                                                    "next_step",
                                                    "action_required",
                                                ] {
                                                    if let Some(val) =
                                                        body_json.get(*key).and_then(|v| v.as_str())
                                                    {
                                                        parts.push(format!("{}: {}", key, val));
                                                    }
                                                }
                                                if !parts.is_empty() {
                                                    user_facing_messages.push(format!(
                                                        "[{}] {}",
                                                        action.capability_id,
                                                        parts.join("\n")
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }

                                // If a skill operation is executed directly and returns a secret, cache it.
                                if response.success {
                                    self.maybe_cache_secret_from_response(
                                        &action.capability_id,
                                        &response,
                                    );
                                }
                                completed_actions.push(action_summary.clone());
                                self.increment_step();

                                // Record action result in history so LLM context carries forward.
                                let result_summary = if let Some(result) = &response.result {
                                    // Compact summary: capability + key fields (redacted)
                                    let redacted = redact_json_for_logs(result);
                                    format!("[action ok] {}: {}", action.capability_id, redacted)
                                } else {
                                    format!("[action ok] {}", action.capability_id)
                                };
                                self.record_in_history("system", result_summary, &event.channel_id);

                                // Check if this was a skill load and we need to continue onboarding
                                if action.capability_id == "ccos.skill.load" && response.success {
                                    if let Some(result) = &response.result {
                                        let loaded_skill_id = result
                                            .get("skill_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown");
                                        let loaded_caps = result
                                            .get("registered_capabilities")
                                            .or_else(|| result.get("capabilities"))
                                            .and_then(|v| v.as_array())
                                            .map(|caps| {
                                                caps.iter()
                                                    .filter_map(|c| c.as_str())
                                                    .collect::<Vec<_>>()
                                                    .join(", ")
                                            })
                                            .unwrap_or_else(|| "<none>".to_string());
                                        let requires_approval = result
                                            .get("requires_approval")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);
                                        info!(
                                            "Skill loaded: id={}, requires_approval={}, capabilities=[{}]",
                                            loaded_skill_id, requires_approval, loaded_caps
                                        );
                                        if let Some(skill_id) =
                                            result.get("skill_id").and_then(|v| v.as_str())
                                        {
                                            self.last_loaded_skill_id = Some(skill_id.to_string());
                                            self.loaded_skill_ids.insert(skill_id.to_string());
                                            if let Some(url) =
                                                result.get("url").and_then(|v| v.as_str())
                                            {
                                                self.loaded_skill_urls.insert(url.to_string());
                                            }
                                        }
                                        if let Some(caps) = result
                                            .get("registered_capabilities")
                                            .or_else(|| result.get("capabilities"))
                                            .and_then(|v| v.as_array())
                                        {
                                            for cap in caps.iter().filter_map(|c| c.as_str()) {
                                                self.loaded_skill_capabilities
                                                    .insert(cap.to_string());
                                            }
                                        }
                                        let _ = self.fetch_capabilities().await;
                                        if result.get("skill_definition").is_some() {
                                            let skill_id = result
                                                .get("skill_id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("unknown");

                                            // Store the raw skill definition for future LLM context.
                                            if let Some(def_text) = result
                                                .get("skill_definition")
                                                .and_then(|v| v.as_str())
                                            {
                                                self.loaded_skill_definitions.insert(
                                                    skill_id.to_string(),
                                                    def_text.to_string(),
                                                );
                                            }

                                            let cap_lines = result
                                                .get("registered_capabilities")
                                                .or_else(|| result.get("capabilities"))
                                                .and_then(|v| v.as_array())
                                                .map(|caps| {
                                                    caps.iter()
                                                        .filter_map(|c| c.as_str())
                                                        .map(|c| format!("- {}", c))
                                                        .collect::<Vec<_>>()
                                                        .join("\n")
                                                })
                                                .unwrap_or_else(|| "- <none>".to_string());

                                            // Include skill definition in follow-up so LLM
                                            // can reason about onboarding steps.
                                            let def_block = result
                                                .get("skill_definition")
                                                .and_then(|v| v.as_str())
                                                .map(|d| {
                                                    // Truncate if very long to keep prompt manageable.
                                                    let max = 3000;
                                                    if d.len() > max {
                                                        format!(
                                                            "\n\nSkill definition (truncated):\n{}...",
                                                            &d[..max]
                                                        )
                                                    } else {
                                                        format!("\n\nSkill definition:\n{}", d)
                                                    }
                                                })
                                                .unwrap_or_default();

                                            // Create a follow-up to continue onboarding
                                            let follow_up_content = format!(
                                                "The skill has been loaded. Skill ID: {}\n\nRegistered capabilities:\n{}{}\n\nPlease continue with the onboarding process to make this skill operational. Use ccos.skill.execute with this skill_id and the capability names above. Follow the onboarding steps described in the skill definition.",
                                                skill_id, cap_lines, def_block
                                            );
                                            let follow_up_event = ChatEvent {
                                                id: format!("followup-{}", event.id),
                                                channel_id: event.channel_id.clone(),
                                                content: follow_up_content,
                                                sender: "system".to_string(),
                                                timestamp: Utc::now().to_rfc3339(),
                                            };
                                            // Process the follow-up (this is recursive but bounded by the conversation)
                                            info!("Skill loaded. Continuing with onboarding...");
                                            follow_up_events.push(follow_up_event);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Action {} failed: {}", action.capability_id, e);
                                failed_actions.push(format!("{}: {}", action.capability_id, e));
                            }
                        }
                    }

                    // If this agent is running under a Run, best-effort transition the Run based on outcome.
                    // This keeps the Gateway/Agent generic: the agent reports Done/Failed/Paused when it knows.
                    if self.args.run_id.is_some() {
                        let has_follow_up = !follow_up_events.is_empty();

                        if self.budget_paused {
                            // Budget pause transitions are handled where the pause is triggered.
                        } else if !failed_actions.is_empty() {
                            // Truncate to keep audit small.
                            let reason = failed_actions
                                .join("; ");
                            let reason = if reason.len() > 500 {
                                format!("{}...", &reason[..500])
                            } else {
                                reason
                            };
                            let _ = self.transition_run_state("Failed", Some(&reason)).await;
                        } else if !missing_inputs.is_empty() || !skill_load_failures.is_empty() {
                            let _ = self
                                .transition_run_state(
                                    "PausedExternalEvent",
                                    Some("awaiting_user_input"),
                                )
                                .await;
                        } else if !user_facing_messages.is_empty() {
                            // Action results contain instructions for the user (e.g. "post this tweet").
                            // The run is not done -- a human action is pending.
                            let _ = self
                                .transition_run_state(
                                    "PausedExternalEvent",
                                    Some("awaiting_human_action"),
                                )
                                .await;
                        } else if !plan.actions.is_empty() && !has_follow_up {
                            let predicate = self
                                .fetch_run_completion_predicate()
                                .await
                                .ok()
                                .flatten();
                            let should_done = match predicate.as_deref() {
                                Some("manual") | Some("never") => false,
                                Some("always") => true,
                                // If a predicate is set but we don't understand it, don't auto-complete.
                                Some(_) => false,
                                // Default: treat lack of a predicate as "finish when this step completed".
                                None => true,
                            };
                            if should_done {
                                let _ = self.transition_run_state("Done", None).await;
                            }
                        }
                    }

                    // Send follow-up summary (what actually happened + next steps).
                    if !completed_actions.is_empty() || !failed_actions.is_empty() {
                        let mut summary = String::new();
                        if !completed_actions.is_empty() {
                            summary.push_str("Completed:\n");
                            for item in &completed_actions {
                                summary.push_str(&format!("- {}\n", item));
                            }
                        }
                        if !failed_actions.is_empty() {
                            if !summary.is_empty() {
                                summary.push_str("\n");
                            }
                            summary.push_str("Failed:\n");
                            for failure in &failed_actions {
                                summary.push_str(&format!("- {}\n", failure));
                            }
                        }

                        // Include user-facing messages from action results (verification tweets, instructions, etc.)
                        if !user_facing_messages.is_empty() {
                            summary.push_str("\nAction results:\n");
                            for msg in &user_facing_messages {
                                summary.push_str(&format!("{}\n", msg));
                            }
                        }

                        summary.push_str("\nNext:\n");
                        if !skill_load_failures.is_empty() {
                            summary.push_str("- The skill failed to load. Please provide a valid skill URL (Markdown/YAML/JSON), or the correct skill id/registry entry.\n");
                        }
                        if !missing_inputs.is_empty() {
                            for (cap_id, missing) in &missing_inputs {
                                summary.push_str(&format!(
                                    "- Please provide {} for {}.\n",
                                    missing.join(", "),
                                    cap_id
                                ));
                            }
                        } else if !user_facing_messages.is_empty() {
                            // The action results contain instructions for the user -- don't
                            // say "tell me to continue" because a human action is pending.
                            summary.push_str("- Please follow the instructions above and provide the required information so I can proceed with the next step.\n");
                        } else if skill_load_failures.is_empty() {
                            // Heuristic next-steps for multi-stage onboarding flows:
                            summary.push_str("- Tell me if you want me to continue.\n");
                            summary.push_str("- If a capability requires extra inputs or a human action (e.g., a URL/code), provide what it asks for and I'll proceed.\n");
                        }

                        info!("Agent summary: {}", summary);
                        let mut inputs = serde_json::Map::new();
                        inputs.insert("content".to_string(), serde_json::json!(summary));
                        inputs.insert(
                            "channel_id".to_string(),
                            serde_json::json!(event.channel_id),
                        );
                        inputs.insert("content_class".to_string(), serde_json::json!("public"));
                        inputs.insert(
                            "session_id".to_string(),
                            serde_json::json!(self.args.session_id),
                        );
                        inputs.insert(
                            "run_id".to_string(),
                            serde_json::json!(format!("resp-{}", event.id)),
                        );
                        inputs.insert(
                            "step_id".to_string(),
                            serde_json::json!(format!("resp-step-{}", event.id)),
                        );

                        let _ = self
                            .execute_capability(
                                "ccos.chat.egress.send_outbound",
                                serde_json::Value::Object(inputs),
                            )
                            .await;

                        // Record summary in history for next-turn context.
                        self.record_in_history("agent", summary, &event.channel_id);
                    }

                    // Process any follow-up events (e.g., continue onboarding)
                    for follow_up in follow_up_events {
                        info!(
                            "Processing follow-up: {}",
                            &follow_up.content[..50.min(follow_up.content.len())]
                        );
                        // Box the recursive call to avoid infinitely-sized future
                        if let Err(e) = Box::pin(self.process_event(follow_up)).await {
                            warn!("Follow-up processing failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("LLM processing failed: {}", e);
                    // Fallback: send simple response
                    self.send_simple_response(&event).await?;
                }
            }
        } else {
            warn!("LLM not available, using simple response");
            self.send_simple_response(&event).await?;
        }

        Ok(())
    }

    /// Send a simple response (for testing without LLM)
    async fn send_simple_response(&self, event: &ChatEvent) -> anyhow::Result<()> {
        info!("Would respond to: {}", event.content);

        // Echo response logic
        let mut inputs = serde_json::Map::new();
        inputs.insert("content_class".to_string(), serde_json::json!("public"));
        inputs.insert(
            "content".to_string(),
            serde_json::json!(format!(
                "I am a simple echo agent. You said: {}",
                event.content
            )),
        );
        inputs.insert(
            "channel_id".to_string(),
            serde_json::json!(event.channel_id),
        );
        inputs.insert(
            "session_id".to_string(),
            serde_json::json!(self.args.session_id),
        );
        inputs.insert(
            "run_id".to_string(),
            serde_json::json!(format!("echo-run-{}", event.id)),
        );
        inputs.insert(
            "step_id".to_string(),
            serde_json::json!(format!("echo-step-{}", event.id)),
        );

        let inputs_value = serde_json::Value::Object(inputs);

        self.execute_capability("ccos.chat.egress.send_outbound", inputs_value)
            .await?;

        Ok(())
    }

    fn parse_missing_inputs(error_msg: &str) -> Option<(String, Vec<String>)> {
        let prefix = "Missing required inputs for ";
        let start = error_msg.find(prefix)?;
        let remainder = &error_msg[start + prefix.len()..];
        let (cap_id, rest) = remainder.split_once(':')?;
        let missing = rest
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return None;
        }
        Some((cap_id.trim().to_string(), missing))
    }

    fn extract_missing_fields(error_msg: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut rest = error_msg;
        let needle = "missing field `";
        while let Some(start) = rest.find(needle) {
            let after = &rest[start + needle.len()..];
            if let Some(end) = after.find('`') {
                let field = after[..end].trim();
                if !field.is_empty() && !fields.contains(&field.to_string()) {
                    fields.push(field.to_string());
                }
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
        fields
    }

    fn extract_required_fields(schema: &serde_json::Value) -> Option<Vec<String>> {
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>();
        Some(required)
    }

    fn build_llm_context(&self) -> String {
        let mut lines = Vec::new();
        for key in &self.llm_context_allowlist {
            if let Some(val) = self.context_value(key) {
                lines.push(format!("{}: {}", key, val));
            }
        }
        // Expose skill URL hints so the LLM can resolve skill names to URLs.
        for (name, url) in &self.skill_url_hints {
            lines.push(format!("skill_url_hint: {}={}", name, url));
        }
        // Include loaded skill definitions so the LLM can reason about
        // onboarding steps, operations, and authentication flows.
        for (skill_id, definition) in &self.loaded_skill_definitions {
            let max = 3000;
            let def_text = if definition.len() > max {
                format!("{}...", &definition[..max])
            } else {
                definition.clone()
            };
            lines.push(format!(
                "loaded_skill[{}] definition:\n{}",
                skill_id, def_text
            ));
        }
        lines.join("\n")
    }

    fn context_value(&self, key: &str) -> Option<String> {
        match key {
            "agent_id" => self.agent_id.clone(),
            "agent_name" => self.agent_name.clone(),
            "llm_model" => self.args.llm_model.clone(),
            _ => None,
        }
    }

    fn autofill_value_for_field(&self, field: &str) -> Option<String> {
        match field {
            "name" | "agent_name" => self
                .allowed_context_value("agent_name")
                .or_else(|| self.allowed_context_value("agent_id")),
            "model" | "llm_model" => self.allowed_context_value("llm_model"),
            "agent_id" => self.allowed_context_value("agent_id"),
            _ => None,
        }
    }

    fn maybe_inject_wrapper_skill_auth(
        &self,
        obj: &mut serde_json::Map<String, serde_json::Value>,
    ) {
        let Some(skill_id) = obj.get("skill").and_then(|v| v.as_str()) else {
            return;
        };
        let token = self
            .skill_bearer_tokens
            .get(skill_id)
            .cloned()
            .or_else(|| Self::load_skill_token_from_secret_store(skill_id));
        let Some(token) = token else { return };

        // Ensure params.headers.Authorization exists (but don't override if present).
        let params = obj
            .entry("params".to_string())
            .or_insert_with(|| serde_json::json!({}));
        let Some(params_obj) = params.as_object_mut() else {
            return;
        };
        let headers = params_obj
            .entry("headers".to_string())
            .or_insert_with(|| serde_json::json!({}));
        let Some(headers_obj) = headers.as_object_mut() else {
            return;
        };

        if headers_obj
            .keys()
            .any(|k| k.eq_ignore_ascii_case("authorization"))
        {
            return;
        }

        headers_obj.insert(
            "Authorization".to_string(),
            serde_json::json!(format!("Bearer {}", token)),
        );
    }

    fn maybe_inject_direct_skill_auth(
        tokens: &HashMap<String, String>,
        capability_id: &str,
        inputs: &mut serde_json::Value,
    ) {
        // Only apply to "<skill>.<op>" style capabilities (and skip ccos.*).
        if capability_id.starts_with("ccos.") {
            return;
        }

        let Some((skill_id, _op)) = capability_id.split_once('.') else {
            return;
        };
        let token = tokens
            .get(skill_id)
            .cloned()
            .or_else(|| Self::load_skill_token_from_secret_store(skill_id));
        let Some(token) = token else { return };
        let Some(obj) = inputs.as_object_mut() else {
            return;
        };

        let headers = obj
            .entry("headers".to_string())
            .or_insert_with(|| serde_json::json!({}));
        let Some(headers_obj) = headers.as_object_mut() else {
            return;
        };
        if headers_obj
            .keys()
            .any(|k| k.eq_ignore_ascii_case("authorization"))
        {
            return;
        }

        headers_obj.insert(
            "Authorization".to_string(),
            serde_json::json!(format!("Bearer {}", token)),
        );
    }

    fn maybe_cache_secret_from_response(
        &mut self,
        capability_id: &str,
        response: &ExecuteResponse,
    ) {
        // Works for direct skill operations: the HTTP executor returns {"status","body","headers"}.
        if capability_id.starts_with("ccos.") {
            return;
        }
        let Some((skill_id, _op)) = capability_id.split_once('.') else {
            return;
        };
        let Some(result) = response.result.as_ref() else {
            return;
        };
        let Some(body) = result.get("body").and_then(|v| v.as_str()) else {
            return;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        let Some(secret) = json.get("secret").and_then(|v| v.as_str()) else {
            return;
        };

        self.skill_bearer_tokens
            .insert(skill_id.to_string(), secret.to_string());

        if self.args.persist_skill_secrets {
            let key = Self::secret_store_key_for_skill(skill_id);
            match SecretStore::new(Some(get_workspace_root())) {
                Ok(mut store) => {
                    if let Err(e) = store.set_local(&key, secret.to_string()) {
                        warn!("Failed to persist skill token to SecretStore ({}): {}", key, e);
                    }
                }
                Err(e) => {
                    warn!("Failed to open SecretStore for persistence: {}", e);
                }
            }
        }
    }

    fn secret_store_key_for_skill(skill_id: &str) -> String {
        let mut out = String::from("CCOS_SKILL_");
        for ch in skill_id.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_uppercase());
            } else {
                out.push('_');
            }
        }
        out.push_str("_BEARER_TOKEN");
        out
    }

    fn load_skill_token_from_secret_store(skill_id: &str) -> Option<String> {
        let key = Self::secret_store_key_for_skill(skill_id);
        SecretStore::new(Some(get_workspace_root()))
            .ok()
            .and_then(|store| store.get(&key))
    }

    fn allowed_context_value(&self, key: &str) -> Option<String> {
        if self.llm_context_allowlist.iter().any(|k| k == key) {
            self.context_value(key)
        } else {
            None
        }
    }

    /// Execute a capability through the Gateway
    async fn execute_capability(
        &self,
        capability_id: &str,
        inputs: serde_json::Value,
    ) -> anyhow::Result<ExecuteResponse> {
        let url = format!("{}/chat/execute", self.args.gateway_url);

        let request = ExecuteRequest {
            capability_id: capability_id.to_string(),
            inputs,
            session_id: self.args.session_id.clone(),
        };

        let response = self
            .client
            .post(&url)
            .header("X-Agent-Token", &self.args.token)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let exec_resp: ExecuteResponse = response.json().await?;
            if exec_resp.success {
                info!("Capability {} executed successfully", capability_id);
            } else {
                warn!(
                    "Capability {} failed: {}",
                    capability_id,
                    redact_text_for_logs(&format!("{:?}", exec_resp.error))
                );
            }
            Ok(exec_resp)
        } else {
            Err(anyhow::anyhow!(
                "Capability execution failed: {}",
                response.status()
            ))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("ccos_agent=info")
        .init();

    // Parse arguments and merge with config file
    let args = Args::parse().merge_with_config()?;

    info!("CCOS Agent Runtime starting...");
    if let Some(config_path) = &args.config_path {
        info!("Using configuration: {}", config_path);
    }
    info!("Session ID: {}", args.session_id);
    info!("Gateway URL: {}", args.gateway_url);
    info!("LLM Enabled: {}", args.enable_llm);
    if args.enable_llm {
        info!(
            "LLM Provider: {}",
            args.llm_provider.as_deref().unwrap_or("local")
        );
        info!(
            "LLM Model: {}",
            args.llm_model.as_deref().unwrap_or("gpt-3.5-turbo")
        );
    }

    // Validate required arguments
    if args.token.is_empty() {
        error!("Token is required. Use --token or set CCOS_AGENT_TOKEN env var");
        std::process::exit(1);
    }

    if args.session_id.is_empty() {
        error!("Session ID is required. Use --session-id or set CCOS_SESSION_ID env var");
        std::process::exit(1);
    }

    // Create and run agent
    let mut agent = AgentRuntime::new(args);

    // Run until error (in production, handle graceful shutdown)
    if let Err(e) = agent.run().await {
        error!("Agent runtime error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_parsing() {
        let args = Args::parse_from([
            "ccos-agent",
            "--token",
            "test-token",
            "--session-id",
            "test-session",
            "--gateway-url",
            "http://localhost:8080",
        ]);

        assert_eq!(args.token, "test-token");
        assert_eq!(args.session_id, "test-session");
        assert_eq!(args.gateway_url, "http://localhost:8080");
        assert!(!args.enable_llm);
        assert!(args.llm_provider.is_none());
    }
}
