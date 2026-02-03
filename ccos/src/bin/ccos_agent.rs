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
use chrono::Utc;
use clap::Parser;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

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
}

impl Args {
    /// Merge with config file if --config-path is provided
    /// CLI arguments take precedence over config file values
    fn merge_with_config(mut self) -> anyhow::Result<Self> {
        if let Some(config_path) = &self.config_path {
            match load_agent_config(config_path) {
                Ok(config) => {
                    info!("Loaded agent configuration from: {}", config_path);

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
#[derive(Debug, Deserialize)]
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
}

impl AgentRuntime {
    fn new(args: Args) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

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

        Self {
            args,
            client,
            message_history: Vec::new(),
            llm_client,
            capabilities: Vec::new(),
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
            self.capabilities = caps_resp
                .capabilities
                .into_iter()
                .map(|c| format!("- {} ({}) - {}", c.id, c.version, c.description))
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
        info!(
            "[Agent] Received event from {} (ID: {}): {}",
            event.sender,
            event.id,
            &event.content[..50.min(event.content.len())]
        );

        // Store in history
        self.message_history.push(event.clone());

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
            serde_json::json!(format!("resp-{}", event.id)),
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

    async fn send_simple_echo(&self, event: &ChatEvent) -> anyhow::Result<()> {
        self.send_response(
            event,
            &format!("I am a simple agent. You said: {}", event.content),
        )
        .await
    }

    /// Process message using LLM
    async fn process_with_llm(&mut self, event: ChatEvent) -> anyhow::Result<()> {
        if let Some(llm) = &self.llm_client {
            info!("Processing message with LLM: {}", event.content);

            // Build context from message history
            let context: Vec<String> = self
                .message_history
                .iter()
                .map(|e| format!("{}: {}", e.sender, e.content))
                .collect();

            // Process with LLM
            match llm
                .process_message(&event.content, &context, &self.capabilities)
                .await
            {
                Ok(plan) => {
                    info!("LLM understanding: {}", plan.understanding);
                    info!("Planned {} actions", plan.actions.len());

                    // Collect follow-up events (e.g., continue after skill load)
                    let mut follow_up_events: Vec<ChatEvent> = Vec::new();

                    // Execute planned actions through Gateway
                    for (idx, action) in plan.actions.iter().enumerate() {
                        info!(
                            "Executing action {}/{}: {}",
                            idx + 1,
                            plan.actions.len(),
                            action.capability_id
                        );
                        info!("Reasoning: {}", action.reasoning);

                        // Add session/run/step IDs to inputs
                        let mut inputs = action.inputs.clone();
                        if let Some(obj) = inputs.as_object_mut() {
                            obj.insert(
                                "session_id".to_string(),
                                serde_json::json!(self.args.session_id),
                            );
                            obj.insert(
                                "run_id".to_string(),
                                serde_json::json!(format!("run-{}-{}", event.id, idx)),
                            );
                            obj.insert(
                                "step_id".to_string(),
                                serde_json::json!(format!("step-{}-{}", event.id, idx)),
                            );

                            // AUTO-FIX: Mark all LLM-planned outbound messages as public for egress
                            if action.capability_id == "ccos.chat.egress.send_outbound" {
                                obj.insert(
                                    "content_class".to_string(),
                                    serde_json::json!("public"),
                                );
                            }
                        }

                        match self.execute_capability(&action.capability_id, inputs).await {
                            Ok(response) => {
                                info!("Action {} succeeded: {:?}", action.capability_id, response);

                                // Check if this was a skill load and we need to continue onboarding
                                if action.capability_id == "ccos.skill.load" && response.success {
                                    if let Some(result) = &response.result {
                                        if let Some(skill_def) = result.get("skill_definition") {
                                            // Create a follow-up to continue onboarding
                                            let follow_up_content = format!(
                                                "The skill has been loaded. Here is the skill definition:\n{}\n\nPlease continue with the onboarding process to make this skill operational. Execute all necessary onboarding steps.",
                                                skill_def
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
                            }
                        }
                    }

                    // Send response back to user if available
                    if !plan.response.is_empty() {
                        info!("Agent response: {}", plan.response);
                        let mut inputs = serde_json::Map::new();
                        inputs.insert("content".to_string(), serde_json::json!(plan.response));
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
                warn!("Capability {} failed: {:?}", capability_id, exec_resp.error);
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
