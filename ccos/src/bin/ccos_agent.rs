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

use ccos::chat::agent_llm::{ActionResult, AgentLlmClient, IterativeAgentPlan, LlmConfig};
use ccos::chat::agent_log::{AgentLogRequest, AgentLogResponse, PlannedCapability};
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
use tracing::{debug, error, info, warn};

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

    /// LLM Max Tokens
    #[arg(long, env = "CCOS_LLM_MAX_TOKENS")]
    llm_max_tokens: Option<u32>,

    /// Gateway HTTP timeout in seconds
    #[arg(long, env = "CCOS_GATEWAY_TIMEOUT_SECS", default_value_t = 120)]
    gateway_timeout_secs: u64,

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
    #[arg(
        long,
        env = "CCOS_AGENT_PERSIST_SKILL_SECRETS",
        default_value = "false"
    )]
    persist_skill_secrets: bool,

    /// Known skill URL hints (repeatable). Format: "name=url".
    /// When the user mentions a skill by name the agent will use the hinted URL
    /// instead of asking. Example: --skill-url-hint "moltbook=http://localhost:8765/skill.md"
    #[arg(long, env = "CCOS_SKILL_URL_HINTS", value_delimiter = ',')]
    skill_url_hint: Option<Vec<String>>,

    // ── Autonomous Mode Configuration ─────────────────────────────────────────
    /// Enable autonomous iterative mode (consult LLM after each action)
    #[arg(long, env = "CCOS_AUTONOMOUS_ENABLED")]
    autonomous_enabled: Option<bool>,

    /// Maximum iterations per user request (safety limit, 0 = use config default)
    #[arg(long, env = "CCOS_AUTONOMOUS_MAX_ITERATIONS")]
    autonomous_max_iterations: Option<u32>,

    /// Enable intermediate progress responses to user
    #[arg(long, env = "CCOS_AUTONOMOUS_INTERMEDIATE")]
    autonomous_intermediate: Option<bool>,

    /// Failure handling: "ask_user" or "abort"
    #[arg(long, env = "CCOS_AUTONOMOUS_FAILURE")]
    autonomous_failure: Option<String>,

    /// Configuration reference (populated from config file)
    #[arg(skip)]
    config: AgentConfig,
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
                            if let Some(ref api_key_env) = profile.api_key_env {
                                if self.llm_api_key.is_none() {
                                    self.llm_api_key = std::env::var(api_key_env).ok();
                                }
                            }
                            if self.llm_max_tokens.is_none() {
                                self.llm_max_tokens = profile.max_tokens;
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

                    // Store config for later use
                    self.config = config;
                }
                Err(e) => {
                    warn!("Failed to load config from {}: {}", config_path, e);
                    // Continue with CLI args only, use default config
                    self.config = AgentConfig::default();
                }
            }
        } else {
            // No config path provided, use default
            self.config = AgentConfig::default();
        }

        // Apply CLI overrides for autonomous mode settings
        // CLI args take precedence over config file
        if let Some(enabled) = self.autonomous_enabled {
            self.config.autonomous_agent.enabled = enabled;
        }
        if let Some(max_iter) = self.autonomous_max_iterations {
            if max_iter > 0 {
                self.config.autonomous_agent.max_iterations = max_iter;
            }
        }
        if let Some(intermediate) = self.autonomous_intermediate {
            self.config.autonomous_agent.send_intermediate_responses = intermediate;
        }
        if let Some(ref failure) = self.autonomous_failure {
            self.config.autonomous_agent.failure_handling = failure.clone();
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
            .timeout(Duration::from_secs(args.gateway_timeout_secs))
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
                    max_tokens: args.llm_max_tokens.unwrap_or(1000),
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
    #[allow(dead_code)]
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
            let _ = self.transition_run_state("Failed", Some(reason)).await;
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

        // Start heartbeat task
        let _heartbeat_handle = self.spawn_heartbeat_task();

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

        info!(
            "[Agent→Gateway] Poll events session={} url={}",
            self.args.session_id, url
        );
        let response = self
            .client
            .get(&url)
            .header("X-Agent-Token", &self.args.token)
            .send()
            .await?;

        if response.status().is_success() {
            let events_resp: EventsResponse = response.json().await?;
            info!(
                "[Gateway→Agent] Events response session={} count={}",
                self.args.session_id,
                events_resp.messages.len()
            );
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
                let _ = self
                    .transition_run_state("Active", Some("resumed by user"))
                    .await;
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
            serde_json::json!(self
                .args
                .run_id
                .clone()
                .unwrap_or_else(|| format!("resp-{}", event.id))),
        );
        inputs.insert(
            "step_id".to_string(),
            serde_json::json!(format!("resp-step-{}", event.id)),
        );

        inputs.insert("content_class".to_string(), serde_json::json!("public"));

        if let Err(e) = self
            .execute_capability(
                "ccos.chat.egress.send_outbound",
                serde_json::Value::Object(inputs),
            )
            .await
        {
            error!("Failed to send outbound response: {}", e);
        }
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

    #[allow(dead_code)]
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

    /// Process message using iterative LLM consultation
    async fn process_with_llm(&mut self, event: ChatEvent) -> anyhow::Result<()> {
        if self.llm_client.is_none() {
            return self.send_simple_echo(&event).await;
        }

        // Clone config to avoid borrow issues
        let config = self.args.config.autonomous_agent.clone();

        info!(
            "Processing message with iterative LLM: {}",
            redact_text_for_logs(&event.content)
        );
        info!(
            "Autonomous mode: enabled={}, max_iterations={}, intermediate={}",
            config.enabled, config.max_iterations, config.send_intermediate_responses
        );

        // Build initial context from message history
        let mut context: Vec<String> = self
            .message_history
            .iter()
            .map(|e| format!("{}: {}", e.sender, e.content))
            .collect();

        let agent_context = self.build_llm_context();
        let mut iteration = 0;
        let mut action_history: Vec<ActionResult> = Vec::new();
        let mut consecutive_failures = 0;
        let mut final_response_sent = false;

        loop {
            iteration += 1;

            // Safety limit check
            if iteration > config.max_iterations {
                warn!("Max iterations ({}) reached", config.max_iterations);
                let summary = self.format_action_summary(&action_history);
                let msg = format!(
                    "I've reached the maximum number of steps ({}). Here's what I completed:\n\n{}",
                    config.max_iterations, summary
                );
                self.send_response(&event, &msg).await?;
                self.record_in_history("agent", msg, &event.channel_id);
                break;
            }

            // Consult LLM
            let plan = if iteration == 1 {
                // First iteration - get initial plan
                info!("Iteration {}: Getting initial plan", iteration);
                match self
                    .llm_client
                    .as_ref()
                    .unwrap()
                    .process_message(&event.content, &context, &self.capabilities, &agent_context)
                    .await
                {
                    Ok(initial_plan) => {
                        info!(
                            "Initial plan: understanding='{}', actions={}",
                            initial_plan.understanding,
                            initial_plan.actions.len()
                        );

                        // Send initial response if provided and intermediate mode is on
                        if config.send_intermediate_responses && !initial_plan.response.is_empty() {
                            self.send_response(&event, &initial_plan.response).await?;
                            self.record_in_history(
                                "agent",
                                initial_plan.response.clone(),
                                &event.channel_id,
                            );
                        }

                        // Convert to iterative plan format
                        let plan = IterativeAgentPlan {
                            understanding: initial_plan.understanding,
                            task_complete: initial_plan.actions.is_empty(),
                            reasoning: "Initial plan from LLM".to_string(),
                            actions: initial_plan.actions,
                            response: initial_plan.response,
                        };

                        // Log the initial consultation to Causal Chain
                        if let Err(e) = self
                            .log_llm_consultation(iteration, true, &plan, &event)
                            .await
                        {
                            warn!("Failed to log initial LLM consultation: {}", e);
                        }

                        plan
                    }
                    Err(e) => {
                        warn!("LLM initial planning failed: {}", e);
                        let error_msg =
                            format!("I encountered an error planning your request: {}", e);
                        self.send_response(&event, &error_msg).await?;
                        self.record_in_history("agent", error_msg, &event.channel_id);
                        return Ok(());
                    }
                }
            } else {
                // Subsequent iterations - consult with results
                info!(
                    "Iteration {}: Consulting LLM with action results",
                    iteration
                );

                let last_result = action_history
                    .last()
                    .and_then(|r| r.result.clone())
                    .unwrap_or_else(|| serde_json::json!({}));

                match self
                    .llm_client
                    .as_ref()
                    .unwrap()
                    .consult_after_action(
                        &event.content,
                        &action_history,
                        &last_result,
                        &context,
                        &self.capabilities,
                        &agent_context,
                    )
                    .await
                {
                    Ok(plan) => {
                        info!(
                            "Iterative response: task_complete={}, actions={}, reasoning='{}'",
                            plan.task_complete,
                            plan.actions.len(),
                            plan.reasoning
                        );

                        // Log the follow-up consultation to Causal Chain
                        if let Err(e) = self
                            .log_llm_consultation(iteration, false, &plan, &event)
                            .await
                        {
                            warn!("Failed to log follow-up LLM consultation: {}", e);
                        }

                        plan
                    }
                    Err(e) => {
                        warn!("LLM iterative consultation failed: {}", e);
                        // Ask user what to do
                        let summary = self.format_action_summary(&action_history);
                        let msg = format!(
                            "I encountered an error while working on your request: {}\n\nHere's what I've accomplished so far:\n\n{}\n\nHow would you like me to proceed?",
                            e, summary
                        );
                        self.send_response(&event, &msg).await?;
                        self.record_in_history("agent", msg, &event.channel_id);
                        return Ok(());
                    }
                }
            };

            // Check if task is complete
            if plan.task_complete {
                info!("Task marked complete by LLM at iteration {}", iteration);
                if !plan.response.is_empty() {
                    self.send_response(&event, &plan.response).await?;
                    self.record_in_history("agent", plan.response, &event.channel_id);
                    final_response_sent = true;
                }
                break;
            }

            // Check if no actions planned (but task not marked complete)
            if plan.actions.is_empty() {
                warn!("No actions planned but task not marked complete");
                let summary = self.format_action_summary(&action_history);
                let msg = format!(
                    "I'm not sure how to proceed with your request. Here's what I've done so far:\n\n{}\n\nCould you clarify what you'd like me to do next?",
                    summary
                );
                self.send_response(&event, &msg).await?;
                self.record_in_history("agent", msg, &event.channel_id);
                break;
            }

            // Execute ONLY the first planned action (iterative mode)
            let action = &plan.actions[0];
            info!(
                "Iteration {}: Executing action: {}",
                iteration, action.capability_id
            );
            info!("Reasoning: {}", action.reasoning);

            // Budget check
            let (exceeded, reason) = self.check_budget();
            if exceeded {
                if let Some(reason) = reason {
                    if self.handle_budget_exceeded(&reason, &event).await {
                        let summary = self.format_action_summary(&action_history);
                        let msg = format!(
                            "I've paused due to budget constraints: {}\n\nHere's what I completed:\n\n{}\n\nPlease let me know if you'd like me to continue.",
                            reason, summary
                        );
                        self.send_response(&event, &msg).await?;
                        break;
                    }
                }
            }

            // Prepare action inputs
            let mut inputs = action.inputs.clone();
            if let Some(obj) = inputs.as_object_mut() {
                obj.insert(
                    "session_id".to_string(),
                    serde_json::json!(self.args.session_id),
                );
                let run_id = self
                    .args
                    .run_id
                    .clone()
                    .unwrap_or_else(|| format!("run-{}-{}", event.id, iteration));
                obj.insert("run_id".to_string(), serde_json::json!(run_id.clone()));
                obj.insert(
                    "step_id".to_string(),
                    serde_json::json!(format!("{}-step-{}", run_id, iteration)),
                );

                // Mark outbound messages as public
                if action.capability_id == "ccos.chat.egress.send_outbound"
                    || action.capability_id == "ccos.chat.egress.prepare_outbound"
                {
                    obj.insert("content_class".to_string(), serde_json::json!("public"));
                }

                // Inject auth for skill operations
                if action.capability_id == "ccos.skill.load" {
                    self.maybe_inject_wrapper_skill_auth(obj);
                }
                if action.capability_id == "ccos.skill.execute" {
                    // Convert obj to Value for the function call
                    let mut params_value = serde_json::Value::Object(obj.clone());
                    Self::maybe_inject_direct_skill_auth(
                        &self.skill_bearer_tokens,
                        &action.capability_id,
                        &mut params_value,
                    );
                    // Update obj from the modified params_value
                    if let Some(new_obj) = params_value.as_object() {
                        *obj = new_obj.clone();
                    }
                }
            }

            // Execute the action
            let result = match self.execute_capability(&action.capability_id, inputs).await {
                Ok(resp) => resp,
                Err(e) => ExecuteResponse {
                    success: false,
                    result: None,
                    error: Some(e.to_string()),
                },
            };

            // Record the action result
            let action_result = ActionResult {
                capability_id: action.capability_id.clone(),
                success: result.success,
                result: result.result.clone(),
                error: result.error.clone(),
                iteration,
            };
            action_history.push(action_result.clone());

            // Update context
            context.push(format!(
                "Step {}: {} - {}",
                iteration,
                action.capability_id,
                if result.success { "success" } else { "failed" }
            ));

            // Manage context size
            if context.len() > config.max_context_entries {
                match config.context_strategy.as_str() {
                    "truncate" => {
                        // Remove oldest entries (keep most recent)
                        let remove_count = context.len() - config.max_context_entries;
                        context.drain(0..remove_count);
                    }
                    _ => {
                        // Default to truncate
                        let remove_count = context.len() - config.max_context_entries;
                        context.drain(0..remove_count);
                    }
                }
            }

            // Track consecutive failures
            if result.success {
                consecutive_failures = 0;
            } else {
                consecutive_failures += 1;
            }

            // Record in history
            let result_summary = if let Some(ref res) = result.result {
                format!(
                    "[step {}] {}: {}",
                    iteration,
                    action.capability_id,
                    redact_json_for_logs(res)
                )
            } else {
                format!(
                    "[step {}] {}: {}",
                    iteration,
                    action.capability_id,
                    if result.success { "ok" } else { "failed" }
                )
            };
            self.record_in_history("system", result_summary, &event.channel_id);

            // Handle failure
            if !result.success {
                let error_msg = result
                    .error
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string());
                warn!("Action failed: {}", error_msg);

                // Check if we should ask user or abort
                if config.failure_handling == "ask_user"
                    || consecutive_failures >= config.max_consecutive_failures
                {
                    let summary = self.format_action_summary(&action_history);
                    let msg = format!(
                        "I encountered an issue with {}:\n\nError: {}\n\nHere's what I've accomplished so far:\n\n{}\n\nWould you like me to retry, try a different approach, or would you prefer to handle this manually?",
                        action.capability_id, error_msg, summary
                    );
                    self.send_response(&event, &msg).await?;
                    self.record_in_history("agent", msg, &event.channel_id);
                    return Ok(());
                }
            }

            // Send intermediate response if enabled
            if config.send_intermediate_responses && !plan.response.is_empty() {
                self.send_response(&event, &plan.response).await?;
                self.record_in_history("agent", plan.response.clone(), &event.channel_id);
            }

            // Handle skill load special case
            if action.capability_id == "ccos.skill.load" && result.success {
                if let Some(ref res) = result.result {
                    if let Some(skill_id) = res.get("skill_id").and_then(|v| v.as_str()) {
                        self.last_loaded_skill_id = Some(skill_id.to_string());
                        self.loaded_skill_ids.insert(skill_id.to_string());

                        if let Some(url) = res.get("url").and_then(|v| v.as_str()) {
                            self.loaded_skill_urls.insert(url.to_string());
                        }

                        if let Some(caps) = res
                            .get("registered_capabilities")
                            .or_else(|| res.get("capabilities"))
                            .and_then(|v| v.as_array())
                        {
                            for cap in caps.iter().filter_map(|c| c.as_str()) {
                                self.loaded_skill_capabilities.insert(cap.to_string());
                            }
                        }

                        let _ = self.fetch_capabilities().await;

                        // Cache secret if present
                        self.maybe_cache_secret_from_response(&action.capability_id, &result);
                    }
                }
            }
        }

        // Transition run state if applicable
        if let Some(ref _run_id) = self.args.run_id {
            if !final_response_sent {
                // Task didn't complete successfully
                let summary = self.format_action_summary(&action_history);
                let _ = self
                    .transition_run_state("PausedExternalEvent", Some(&summary))
                    .await;
            } else if action_history.iter().all(|a| a.success) {
                // All actions succeeded
                let _ = self.transition_run_state("Done", None).await;
            }
        }

        Ok(())
    }

    /// Format action history for user messages
    fn format_action_summary(&self, history: &[ActionResult]) -> String {
        if history.is_empty() {
            return "No actions taken yet.".to_string();
        }

        history
            .iter()
            .map(|r| {
                let status = if r.success { "✓" } else { "✗" };
                let result_str = r
                    .result
                    .as_ref()
                    .map(|res| {
                        let s = redact_json_for_logs(res).to_string();
                        if s.len() > 150 {
                            format!("{}...", &s[..150])
                        } else {
                            s
                        }
                    })
                    .unwrap_or_else(|| "completed".to_string());
                format!(
                    "{} Step {}: {} - {}",
                    status, r.iteration, r.capability_id, result_str
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
        if value.chars().count() <= max_chars {
            return value.to_string();
        }
        let truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
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
        lines.push(format!("current_time: {}", Utc::now().to_rfc3339()));
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

    #[allow(dead_code)]
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
                        warn!(
                            "Failed to persist skill token to SecretStore ({}): {}",
                            key, e
                        );
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

    #[allow(dead_code)]
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

        let redacted_inputs = redact_json_for_logs(&inputs);
        info!(
            "[Agent→Gateway] Execute cap={} session={} url={}",
            capability_id, self.args.session_id, url
        );
        debug!(
            "[Agent→Gateway] Execute inputs cap={} inputs={}",
            capability_id, redacted_inputs
        );
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
                let error_msg = exec_resp
                    .error
                    .clone()
                    .unwrap_or_else(|| "Unknown error (None)".to_string());
                warn!(
                    "Capability {} failed: {}",
                    capability_id,
                    redact_text_for_logs(&error_msg)
                );
            }
            if let Some(result) = &exec_resp.result {
                debug!(
                    "[Gateway→Agent] Execute result cap={} result={}",
                    capability_id,
                    redact_json_for_logs(result)
                );
            }
            Ok(exec_resp)
        } else {
            warn!(
                "[Gateway→Agent] Execute failed cap={} status={}",
                capability_id,
                response.status()
            );
            Err(anyhow::anyhow!(
                "Capability execution failed: {}",
                response.status()
            ))
        }
    }

    /// Log an LLM consultation to the Causal Chain via the Gateway
    ///
    /// This preserves a complete timeline of agent decision-making,
    /// including understanding, reasoning, and planned actions.
    async fn log_llm_consultation(
        &self,
        iteration: u32,
        is_initial: bool,
        plan: &IterativeAgentPlan,
        event: &ChatEvent,
    ) -> anyhow::Result<()> {
        let url = format!("{}/chat/agent/log", self.args.gateway_url);

        // Build run_id and step_id
        let run_id = self
            .args
            .run_id
            .clone()
            .unwrap_or_else(|| format!("run-{}", event.id));
        let step_id = format!("{}-llm-{}", run_id, iteration);

        // Build planned capabilities
        let planned_capabilities: Vec<PlannedCapability> = plan
            .actions
            .iter()
            .map(|action| {
                let mut cap = PlannedCapability::new(&action.capability_id, &action.reasoning);
                if let Some(inputs) = action.inputs.as_object() {
                    // Sanitize inputs - remove sensitive fields
                    let mut sanitized = serde_json::Map::new();
                    for (k, v) in inputs.iter() {
                        if k.contains("secret")
                            || k.contains("token")
                            || k.contains("password")
                            || k.contains("key")
                        {
                            sanitized.insert(k.clone(), serde_json::json!("[REDACTED]"));
                        } else {
                            sanitized.insert(k.clone(), v.clone());
                        }
                    }
                    cap = cap.with_inputs(serde_json::Value::Object(sanitized));
                }
                cap
            })
            .collect();

        // Build the request
        let mut request = if is_initial {
            AgentLogRequest::initial(
                &self.args.session_id,
                &run_id,
                &step_id,
                &plan.understanding,
                &plan.reasoning,
            )
        } else {
            AgentLogRequest::follow_up(
                &self.args.session_id,
                &run_id,
                &step_id,
                iteration,
                &plan.understanding,
                &plan.reasoning,
                plan.task_complete,
            )
        };

        // Add planned capabilities
        for cap in planned_capabilities {
            request = request.with_planned_capability(cap);
        }

        // Add model info if available
        if let Some(ref model) = self.args.llm_model {
            request = request.with_model(model);
        }

        // Send to gateway
        let response = self
            .client
            .post(&url)
            .header("X-Agent-Token", &self.args.token)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let log_resp: AgentLogResponse = response.json().await?;
            if log_resp.success {
                debug!(
                    "Logged LLM consultation: iteration={}, complete={}, action_id={}",
                    iteration, plan.task_complete, log_resp.action_id
                );
            } else {
                warn!("Failed to log LLM consultation: {:?}", log_resp.error);
            }
        } else {
            warn!(
                "Failed to log LLM consultation: status={}",
                response.status()
            );
        }

        Ok(())
    }

    /// Spawn a background task to send heartbeats to the Gateway
    fn spawn_heartbeat_task(&self) -> tokio::task::JoinHandle<()> {
        let client = self.client.clone();
        let gateway_url = self.args.gateway_url.clone();
        let token = self.args.token.clone();
        let session_id = self.args.session_id.clone();
        let config_path = self.args.config_path.clone();

        // Load interval from config or env var, default to 1 second
        let interval_secs = config_path
            .as_ref()
            .and_then(|path| load_agent_config(path).ok())
            .map(|config| config.realtime_tracking.heartbeat_send_interval_secs)
            .or_else(|| {
                std::env::var("CCOS_HEARTBEAT_INTERVAL_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .unwrap_or(1u64);

        let _pid = std::process::id();

        tokio::spawn(async move {
            let url = format!("{}/chat/heartbeat/{}", gateway_url, session_id);
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            let mut current_step: u32 = 0;

            loop {
                interval.tick().await;

                // For now, we use a static step count. In a real implementation,
                // we'd share the step_count from AgentRuntime via Arc<AtomicU32>
                // Build heartbeat payload
                let payload = serde_json::json!({
                    "current_step": current_step,
                    "memory_mb": None::<u64>, // Skip memory for now
                });

                // Send heartbeat
                match client
                    .post(&url)
                    .header("X-Agent-Token", &token)
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if !response.status().is_success() {
                            warn!("Heartbeat failed with status: {}", response.status());
                        }
                    }
                    Err(e) => {
                        warn!("Failed to send heartbeat: {}", e);
                    }
                }

                // Increment step counter (placeholder - should be actual step count)
                current_step = current_step.wrapping_add(1);
            }
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("ccos_agent=info".parse().unwrap()),
        )
        .with_ansi(false)
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
