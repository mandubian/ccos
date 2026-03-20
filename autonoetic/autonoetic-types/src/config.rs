//! Gateway configuration types.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Named LLM preset that can be referenced by agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPreset {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub fallback_provider: Option<String>,
    #[serde(default)]
    pub fallback_model: Option<String>,
    /// Set to true if the provider only supports basic chat (no tools at all)
    #[serde(default)]
    pub chat_only: Option<bool>,
    /// Optional context window for CLI "% of context" when preset is applied to SKILL.
    #[serde(default)]
    pub context_window_tokens: Option<u32>,
}

/// When `agent.install` requires human approval before proceeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstallApprovalPolicy {
    /// Always require approval for every install (strictest).
    Always,
    /// Require approval only when the install is classified as high-risk (e.g. broad capabilities, ShellExec, background).
    #[default]
    RiskBased,
    /// Never require approval for install; promotion gate only (dev/convenience).
    Never,
}

/// Schema enforcement mode for agent.spawn payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SchemaEnforcementMode {
    /// Disabled - pass through payloads without enforcement.
    Disabled,
    /// Use deterministic coercion (defaults, type coercion).
    #[default]
    Deterministic,
    /// (Later) Use LLM for complex transformations.
    Llm,
}

/// Configuration for schema enforcement on agent.spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaEnforcementConfig {
    /// Enforcement mode: disabled, deterministic, or llm.
    #[serde(default)]
    pub mode: SchemaEnforcementMode,
    /// Log all enforcement decisions to causal chain.
    #[serde(default = "default_true")]
    pub audit: bool,
    /// Agent-specific overrides (agent_id -> mode).
    #[serde(default)]
    pub agent_overrides: std::collections::HashMap<String, SchemaEnforcementMode>,
}

fn default_true() -> bool {
    true
}

impl Default for SchemaEnforcementConfig {
    fn default() -> Self {
        Self {
            mode: SchemaEnforcementMode::Deterministic,
            audit: true,
            agent_overrides: std::collections::HashMap::new(),
        }
    }
}

/// Session-scoped resource limits enforced by the gateway (role-agnostic).
///
/// All limits are optional: `None` means unlimited for that dimension.
/// Counters are keyed by **session id** (the same id passed to `agent.spawn` / chat),
/// so nested specialist runs in one user session share one budget pool.
///
/// **Related (not duplicated here):** per-agent [`crate::agent::Capability::AgentSpawn`]
/// `max_children` still caps how many child runs a single agent may start per session;
/// configure that on the lead manifest. Future versions may add optional alignment
/// between these knobs via config only.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionBudgetConfig {
    /// Optional profile name for logging and ops (e.g. `dev`, `production`).
    #[serde(default)]
    pub profile: Option<String>,
    /// Maximum LLM `complete()` calls per session (each provider round-trip, including retries).
    #[serde(default)]
    pub max_llm_rounds: Option<u64>,
    /// Maximum tool invocations processed per session (each tool call in a batch counts).
    #[serde(default)]
    pub max_tool_invocations: Option<u64>,
    /// Maximum total LLM tokens (input + output) reported by providers per session.
    #[serde(default)]
    pub max_llm_tokens: Option<u64>,
    /// Maximum wall-clock seconds from first budget touch for this session.
    #[serde(default)]
    pub max_wall_clock_secs: Option<u64>,
    /// Maximum estimated session spend in USD (OpenRouter pricing from the public models API when provider is `openrouter`).
    #[serde(default)]
    pub max_session_price_usd: Option<f64>,
    /// Names of future budget extension modules (reserved; no effect until implemented).
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Top-level Gateway daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Directory containing agent subdirectories, each with a SKILL.md.
    #[serde(default = "default_agents_dir")]
    pub agents_dir: PathBuf,

    /// Port for the local JSON-RPC IPC listener.
    #[serde(default = "default_port")]
    pub port: u16,

    /// OFP federation port.
    #[serde(default = "default_ofp_port")]
    pub ofp_port: u16,

    /// Default lead agent used for ambiguous ingress when target_agent_id is omitted.
    #[serde(default = "default_lead_agent_id")]
    pub default_lead_agent_id: String,

    /// Enable TLS on the OFP port.
    #[serde(default)]
    pub tls: bool,

    /// Maximum number of agent runtime executions allowed concurrently.
    #[serde(default = "default_max_concurrent_spawns")]
    pub max_concurrent_spawns: usize,

    /// Maximum number of pending executions admitted per target agent.
    /// This count includes the currently running execution for that agent.
    #[serde(default = "default_max_pending_spawns_per_agent")]
    pub max_pending_spawns_per_agent: usize,

    /// Enable the gateway-owned background scheduler.
    #[serde(default)]
    pub background_scheduler_enabled: bool,

    /// Tick interval for background due checks.
    #[serde(default = "default_background_tick_secs")]
    pub background_tick_secs: u64,

    /// Global minimum allowed reevaluation interval across agents.
    #[serde(default = "default_background_min_interval_secs")]
    pub background_min_interval_secs: u64,

    /// Max number of due background agents admitted per scheduler tick.
    #[serde(default = "default_max_background_due_per_tick")]
    pub max_background_due_per_tick: usize,

    /// When `agent.install` requires human approval. `risk_based` (default) requires approval only for high-risk installs; `always` for every install; `never` to rely on promotion gate only.
    #[serde(default)]
    pub agent_install_approval_policy: AgentInstallApprovalPolicy,

    /// Schema enforcement configuration for agent.spawn payloads.
    #[serde(default)]
    pub schema_enforcement: SchemaEnforcementConfig,

    /// Named LLM presets for agent bootstrapping (e.g., "agentic" → claude-sonnet).
    #[serde(default)]
    pub llm_presets: HashMap<String, LlmPreset>,

    /// Map role/template names to LLM presets (e.g., "planner" → "agentic", "coder" → "coding").
    #[serde(default)]
    pub llm_preset_mapping: HashMap<String, String>,

    /// Code analysis configuration for agent.install validation.
    /// Controls how the gateway analyzes code for capabilities and security.
    #[serde(default)]
    pub code_analysis: CodeAnalysisConfig,

    /// Optional per-session budgets (LLM rounds, tools, tokens, wall clock).
    #[serde(default)]
    pub session_budget: SessionBudgetConfig,
}

/// Configuration for pluggable code analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysisConfig {
    /// Provider for capability analysis: "pattern", "python_ast", "llm", "composite", "none"
    #[serde(default = "default_capability_provider")]
    pub capability_provider: String,

    /// Provider for security analysis: "pattern", "python_ast", "llm", "composite", "none"
    #[serde(default = "default_security_provider")]
    pub security_provider: String,

    /// Require capabilities to be declared (reject if missing)
    #[serde(default = "default_require_capabilities")]
    pub require_capabilities: bool,

    /// Capability types that always require human approval when detected
    #[serde(default)]
    pub require_approval_for: Vec<String>,

    /// LLM configuration for LLM-based analysis providers
    #[serde(default)]
    pub llm_config: CodeAnalysisLlmConfig,
}

fn default_capability_provider() -> String {
    "pattern".to_string()
}

fn default_security_provider() -> String {
    "pattern".to_string()
}

fn default_require_capabilities() -> bool {
    true
}

impl Default for CodeAnalysisConfig {
    fn default() -> Self {
        Self {
            capability_provider: default_capability_provider(),
            security_provider: default_security_provider(),
            require_capabilities: default_require_capabilities(),
            require_approval_for: vec!["NetworkAccess".to_string(), "CodeExecution".to_string()],
            llm_config: CodeAnalysisLlmConfig::default(),
        }
    }
}

/// LLM configuration for code analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysisLlmConfig {
    /// LLM provider for analysis (e.g., "openrouter", "anthropic")
    #[serde(default = "default_analysis_provider")]
    pub provider: String,

    /// Model for code analysis
    #[serde(default = "default_analysis_model")]
    pub model: String,

    /// Temperature (lower = more deterministic)
    #[serde(default = "default_analysis_temperature")]
    pub temperature: f32,

    /// Timeout in seconds
    #[serde(default = "default_analysis_timeout")]
    pub timeout_secs: u64,
}

fn default_analysis_provider() -> String {
    "openrouter".to_string()
}

fn default_analysis_model() -> String {
    "google/gemini-3-flash-preview".to_string()
}

fn default_analysis_temperature() -> f32 {
    0.1
}

fn default_analysis_timeout() -> u64 {
    30
}

impl Default for CodeAnalysisLlmConfig {
    fn default() -> Self {
        Self {
            provider: default_analysis_provider(),
            model: default_analysis_model(),
            temperature: default_analysis_temperature(),
            timeout_secs: default_analysis_timeout(),
        }
    }
}

fn default_agents_dir() -> PathBuf {
    PathBuf::from("./agents")
}

fn default_port() -> u16 {
    4000
}

fn default_ofp_port() -> u16 {
    4200
}

fn default_lead_agent_id() -> String {
    "planner.default".to_string()
}

fn default_max_concurrent_spawns() -> usize {
    8
}

fn default_max_pending_spawns_per_agent() -> usize {
    4
}

fn default_background_tick_secs() -> u64 {
    5
}

fn default_background_min_interval_secs() -> u64 {
    60
}

fn default_max_background_due_per_tick() -> usize {
    32
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            agents_dir: default_agents_dir(),
            port: default_port(),
            ofp_port: default_ofp_port(),
            default_lead_agent_id: default_lead_agent_id(),
            tls: false,
            max_concurrent_spawns: default_max_concurrent_spawns(),
            max_pending_spawns_per_agent: default_max_pending_spawns_per_agent(),
            background_scheduler_enabled: false,
            background_tick_secs: default_background_tick_secs(),
            background_min_interval_secs: default_background_min_interval_secs(),
            max_background_due_per_tick: default_max_background_due_per_tick(),
            agent_install_approval_policy: AgentInstallApprovalPolicy::default(),
            schema_enforcement: SchemaEnforcementConfig::default(),
            llm_presets: HashMap::new(),
            llm_preset_mapping: HashMap::new(),
            code_analysis: CodeAnalysisConfig::default(),
            session_budget: SessionBudgetConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_scheduler_defaults() {
        let config = GatewayConfig::default();
        assert!(!config.background_scheduler_enabled);
        assert_eq!(config.background_tick_secs, 5);
        assert_eq!(config.background_min_interval_secs, 60);
        assert_eq!(config.max_background_due_per_tick, 32);
        assert_eq!(config.default_lead_agent_id, "planner.default");
    }

    #[test]
    fn session_budget_config_json_roundtrip() {
        let j = serde_json::json!({
            "profile": "staging",
            "max_llm_rounds": 120,
            "max_tool_invocations": 400,
            "max_llm_tokens": 2_000_000u64,
            "max_wall_clock_secs": 7200,
            "extensions": ["future_org_limiter"]
        });
        let parsed: SessionBudgetConfig = serde_json::from_value(j).expect("parse json");
        assert_eq!(parsed.profile.as_deref(), Some("staging"));
        assert_eq!(parsed.max_llm_rounds, Some(120));
        assert_eq!(parsed.max_tool_invocations, Some(400));
        assert_eq!(parsed.max_llm_tokens, Some(2_000_000));
        assert_eq!(parsed.max_wall_clock_secs, Some(7200));
        assert_eq!(parsed.extensions, vec!["future_org_limiter"]);
    }
}
