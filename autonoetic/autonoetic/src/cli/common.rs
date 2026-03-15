use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

use autonoetic_gateway::llm::{CompletionRequest, Message};
use autonoetic_types::causal_chain::CausalChainEntry;
use std::collections::BTreeMap;

// Re-exports for modules
pub use autonoetic_mcp::{
    AgentExecutor as McpAgentExecutor, McpClient, McpServer, McpTool, McpTransportConfig,
};

#[derive(Parser)]
#[command(
    name = "autonoetic",
    about = "CLI for managing the Autonoetic Agent System",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to a custom config.yaml or policy.yaml (default: ~/.ccos/)
    #[arg(global = true, long)]
    pub config: Option<String>,

    /// Overrides the Gateway log level (trace, debug, info, warn, error)
    #[arg(global = true, long)]
    pub log_level: Option<String>,

    /// Disables all prompts (essential for CI/CD)
    #[arg(global = true, long)]
    pub non_interactive: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage the Gateway lifecycle
    Gateway(GatewayArgs),
    /// Manage Autonoetic Agents
    Agent(AgentArgs),
    /// Chat with an agent through gateway JSON-RPC ingress
    Chat(ChatArgs),
    /// Inspect causal chain traces
    Trace(TraceArgs),
    /// Ecosystem and Skills management
    Skill(SkillArgs),
    /// Federation and Cluster management
    Federate(FederateArgs),
    /// MCP Integration management
    Mcp(McpArgs),
}

// ---------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct GatewayArgs {
    #[command(subcommand)]
    pub command: GatewayCommands,
}

#[derive(Subcommand)]
pub enum GatewayCommands {
    /// Starts the Gateway daemon in the foreground
    Start {
        /// Run in the background
        #[arg(short, long)]
        daemon: bool,
        /// Override the default HTTP/TCP ports
        #[arg(long)]
        port: Option<u16>,
        /// Force TLS wrapping on the OFP federation port
        #[arg(long)]
        tls: bool,
    },
    /// Gracefully terminates a background Gateway daemon
    Stop,
    /// Outputs a table of Gateway health, loaded policies, etc.
    Status {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect or decide pending background approvals.
    Approvals {
        #[command(subcommand)]
        command: GatewayApprovalCommands,
    },
}

#[derive(Subcommand)]
pub enum GatewayApprovalCommands {
    /// List pending approval requests.
    List {
        /// Emit machine-readable JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Approve one pending request.
    Approve {
        /// Approval request identifier.
        request_id: String,
        /// Optional approval note.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reject one pending request.
    Reject {
        /// Approval request identifier.
        request_id: String,
        /// Optional rejection note.
        #[arg(long)]
        reason: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Agent
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommands,
}

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Scaffolds a new Autonoetic Agent directory
    Init {
        /// Agent ID to create
        agent_id: String,
        /// Template to use (e.g., researcher, coder, auditor)
        #[arg(long)]
        template: Option<String>,
        /// LLM preset name from config (e.g., agentic, coding, fast)
        #[arg(long)]
        preset: Option<String>,
        /// LLM provider override (e.g., openai, anthropic, gemini, openrouter)
        #[arg(long)]
        provider: Option<String>,
        /// LLM model override (e.g., gpt-4o, claude-sonnet-4-20250514)
        #[arg(long)]
        model: Option<String>,
    },
    /// Boots an Agent and connects it to the Gateway
    Run {
        /// Agent ID to run
        agent_id: String,
        /// Initial message kickoff
        message: Option<String>,
        /// Drops the user into a persistent chat loop
        #[arg(short, long)]
        interactive: bool,
        /// Boots the agent headless
        #[arg(long)]
        headless: bool,
    },
    /// Lists all local Agents registered with the Gateway
    List,
    /// Bootstraps runtime agents from reference bundles
    Bootstrap {
        /// Optional path to reference bundles root (defaults to auto-detection)
        #[arg(long)]
        from: Option<String>,
        /// Overwrite existing target agent directories
        #[arg(long)]
        overwrite: bool,
    },
    /// Shows available LLM presets and template mappings
    Presets,
    /// Creates a default config.yaml with LLM presets
    InitConfig {
        /// Output path for config.yaml (default: ./config.yaml)
        #[arg(long)]
        output: Option<String>,
        /// Overwrite existing config file
        #[arg(long)]
        overwrite: bool,
    },
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ChatArgs {
    /// Optional target agent ID. If omitted, gateway ingress resolves to session/default lead agent.
    pub agent_id: Option<String>,
    /// Stable sender identity for the terminal client.
    #[arg(long)]
    pub sender_id: Option<String>,
    /// Stable channel identity for the terminal surface.
    #[arg(long)]
    pub channel_id: Option<String>,
    /// Stable conversation/session identifier.
    #[arg(long)]
    pub session_id: Option<String>,
    /// Suppress prompts and banners for deterministic scripted tests.
    #[arg(long)]
    pub test_mode: bool,
}

// ---------------------------------------------------------------------------
// Trace
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct TraceArgs {
    #[command(subcommand)]
    pub command: TraceCommands,
}

#[derive(Subcommand)]
pub enum TraceCommands {
    /// List known sessions across agent traces
    Sessions {
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show all events for one session
    Show {
        /// Session identifier
        session_id: String,
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show one specific event by log_id
    Event {
        /// Event/log identifier
        log_id: String,
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Rebuild unified session timeline from gateway + agent causal logs
    Rebuild {
        /// Session identifier to rebuild
        session_id: String,
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
        /// Skip integrity checks
        #[arg(long)]
        skip_checks: bool,
    },
    /// Follow session events in real-time as they happen
    Follow {
        /// Session identifier to follow
        session_id: String,
        /// Restrict to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Fork a session from a snapshot to explore alternative paths
    Fork {
        /// Source session ID to fork from
        session_id: String,
        /// Branch message to append (e.g., "Try a different approach")
        #[arg(long)]
        message: Option<String>,
        /// New session ID (auto-generated if not provided)
        #[arg(long)]
        new_session_id: Option<String>,
        /// Fork from specific turn number (default: latest)
        #[arg(long)]
        at_turn: Option<usize>,
        /// Target agent ID (defaults to source agent)
        #[arg(long)]
        agent: Option<String>,
        /// Start interactive chat after forking
        #[arg(long)]
        interactive: bool,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
    /// Show conversation history for a session
    History {
        /// Session identifier
        session_id: String,
        /// Restrict lookup to one agent
        #[arg(long)]
        agent: Option<String>,
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
}

// ---------------------------------------------------------------------------
// Skill
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommands,
}

#[derive(Subcommand)]
pub enum SkillCommands {
    /// Downloads and installs an AgentSkills.io compliant bundle
    Install {
        /// GitHub URL or Skill ID
        url_or_id: String,
        /// Target agent ID
        #[arg(long)]
        agent: Option<String>,
    },
    /// Removes a skill from an Agent's capability list
    Uninstall {
        /// Name of the skill to uninstall
        skill_name: String,
        /// Target agent ID
        #[arg(long)]
        agent: String,
    },
}

// ---------------------------------------------------------------------------
// Federate
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct FederateArgs {
    #[command(subcommand)]
    pub command: FederateCommands,
}

#[derive(Subcommand)]
pub enum FederateCommands {
    /// Connects the local Gateway to a remote peer via OFP
    Join {
        /// Remote peer address
        peer_address: String,
    },
    /// Outputs the local PeerRegistry
    List,
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCommands,
}

#[derive(Subcommand)]
pub enum McpCommands {
    /// Registers a local MCP server with the Gateway
    Add {
        /// MCP Server name
        server_name: String,
        /// Subprocess command (stdio transport).
        #[arg(long)]
        command: Option<String>,
        /// Optional SSE endpoint transport URL.
        #[arg(long)]
        sse_url: Option<String>,
        /// Optional command arguments
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Temporarily runs the Gateway as an MCP Server on stdio
    Expose {
        /// Agent ID to expose
        agent_id: String,
    },
}

// ===========================================================================
// Shared Utilities
// ===========================================================================

pub fn dirs_or_default() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".ccos"))
        .unwrap_or_else(|| PathBuf::from(".ccos"))
}

pub fn mcp_registry_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.join("mcp_servers.json"))
        .unwrap_or_else(|| PathBuf::from("mcp_servers.json"))
}

pub fn load_mcp_servers(path: &Path) -> anyhow::Result<Vec<McpServer>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(path)?;
    let servers = serde_json::from_str::<Vec<McpServer>>(&raw)?;
    Ok(servers)
}

pub fn save_mcp_servers(path: &Path, servers: &[McpServer]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(servers)?;
    std::fs::write(path, body)?;
    Ok(())
}

pub fn default_terminal_sender_id() -> String {
    std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "terminal-user".to_string())
}

pub fn default_terminal_channel_id(sender_id: &str, target_hint: &str) -> String {
    format!("terminal:{}:{}", sender_id, target_hint)
}

pub fn terminal_channel_envelope(
    channel_id: &str,
    sender_id: &str,
    session_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "channel": {
            "kind": "terminal",
            "channel_id": channel_id,
            "sender_id": sender_id,
            "session_id": session_id
        }
    })
}

#[derive(Debug)]
pub struct AgentTrace {
    pub agent_id: String,
    pub entries: Vec<CausalChainEntry>,
}

#[derive(Debug)]
pub struct TraceEntry {
    pub agent_id: String,
    pub entry: CausalChainEntry,
}

#[derive(Debug)]
pub struct SessionSummary {
    pub agent_id: String,
    pub session_id: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub event_count: usize,
    pub max_event_seq: u64,
}

pub fn collect_session_summaries(traces: &[AgentTrace]) -> Vec<SessionSummary> {
    let mut by_session: BTreeMap<(String, String), SessionSummary> = BTreeMap::new();
    for trace in traces {
        for entry in &trace.entries {
            let key = (trace.agent_id.clone(), entry.session_id.clone());
            let summary = by_session.entry(key).or_insert_with(|| SessionSummary {
                agent_id: trace.agent_id.clone(),
                session_id: entry.session_id.clone(),
                first_timestamp: entry.timestamp.clone(),
                last_timestamp: entry.timestamp.clone(),
                event_count: 0,
                max_event_seq: entry.event_seq,
            });
            summary.event_count += 1;
            if entry.timestamp < summary.first_timestamp {
                summary.first_timestamp = entry.timestamp.clone();
            }
            if entry.timestamp > summary.last_timestamp {
                summary.last_timestamp = entry.timestamp.clone();
            }
            if entry.event_seq > summary.max_event_seq {
                summary.max_event_seq = entry.event_seq;
            }
        }
    }

    by_session.into_values().collect::<Vec<_>>()
}

pub struct CliAgentExecutor {
    pub agents_dir: PathBuf,
    pub client: reqwest::Client,
}

#[async_trait::async_trait]
impl McpAgentExecutor for CliAgentExecutor {
    async fn call_agent(&self, agent_id: &str, message: &str) -> anyhow::Result<String> {
        let repo = autonoetic_gateway::AgentRepository::new(self.agents_dir.clone());
        let loaded = repo.get(agent_id).await?;
        let llm_config = loaded
            .manifest
            .llm_config
            .ok_or_else(|| anyhow::anyhow!("Agent '{}' is missing llm_config", agent_id))?;

        let driver =
            autonoetic_gateway::llm::build_driver(llm_config.clone(), self.client.clone())?;
        let req = CompletionRequest::simple(
            llm_config.model,
            vec![Message::system(loaded.instructions), Message::user(message)],
        );
        let resp = driver.complete(&req).await?;
        if resp.text.trim().is_empty() {
            anyhow::bail!("Agent '{}' returned an empty response", agent_id);
        }
        Ok(resp.text)
    }
}

pub struct ActivatedMcpServer {
    pub name: String,
    pub tools: Vec<autonoetic_mcp::McpTool>,
    pub _client: McpClient,
}

pub struct McpRuntime {
    pub servers: Vec<ActivatedMcpServer>,
}

impl McpRuntime {
    pub fn empty() -> Self {
        Self { servers: vec![] }
    }

    pub fn summary_lines(&self) -> Vec<String> {
        if self.servers.is_empty() {
            return vec!["MCP activation: no registered MCP servers.".to_string()];
        }

        let mut lines = vec![format!(
            "MCP activation: {} server(s) active, {} tool(s) total.",
            self.servers.len(),
            self.servers.iter().map(|s| s.tools.len()).sum::<usize>()
        )];
        for server in &self.servers {
            lines.push(format!(
                "  MCP server '{}' => {} tool(s)",
                server.name,
                server.tools.len()
            ));
            for tool in &server.tools {
                lines.push(format!("    - {}", tool.name));
            }
        }
        lines
    }
}

pub async fn activate_registered_mcp_servers(config_path: &Path) -> anyhow::Result<McpRuntime> {
    let registry_path = mcp_registry_path(config_path);
    let servers = load_mcp_servers(&registry_path)?;
    if servers.is_empty() {
        return Ok(McpRuntime::empty());
    }

    let mut activated = Vec::with_capacity(servers.len());
    for server in servers {
        let mut client = McpClient::connect(&server).await?;
        let tools = client.list_tools().await?;
        activated.push(ActivatedMcpServer {
            name: server.name,
            tools,
            _client: client,
        });
    }

    Ok(McpRuntime { servers: activated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_session_summaries_groups_sessions() {
        let traces = vec![AgentTrace {
            agent_id: "agent_demo".to_string(),
            entries: vec![
                CausalChainEntry {
                    timestamp: "2026-03-06T10:00:00Z".to_string(),
                    log_id: "l1".to_string(),
                    actor_id: "agent_demo".to_string(),
                    session_id: "s1".to_string(),
                    turn_id: Some("turn-000001".to_string()),
                    event_seq: 1,
                    category: "session".to_string(),
                    action: "start".to_string(),
                    target: None,
                    status: autonoetic_types::causal_chain::EntryStatus::Success,
                    reason: None,
                    payload: None,
                    payload_hash: None,
                    prev_hash: "genesis".to_string(),
                    entry_hash: "h1".to_string(),
                },
                CausalChainEntry {
                    timestamp: "2026-03-06T10:00:02Z".to_string(),
                    log_id: "l2".to_string(),
                    actor_id: "agent_demo".to_string(),
                    session_id: "s1".to_string(),
                    turn_id: Some("turn-000001".to_string()),
                    event_seq: 2,
                    category: "lifecycle".to_string(),
                    action: "wake".to_string(),
                    target: None,
                    status: autonoetic_types::causal_chain::EntryStatus::Success,
                    reason: None,
                    payload: None,
                    payload_hash: None,
                    prev_hash: "h1".to_string(),
                    entry_hash: "h2".to_string(),
                },
                CausalChainEntry {
                    timestamp: "2026-03-06T10:05:00Z".to_string(),
                    log_id: "l3".to_string(),
                    actor_id: "agent_demo".to_string(),
                    session_id: "s2".to_string(),
                    turn_id: Some("turn-000001".to_string()),
                    event_seq: 1,
                    category: "session".to_string(),
                    action: "start".to_string(),
                    target: None,
                    status: autonoetic_types::causal_chain::EntryStatus::Success,
                    reason: None,
                    payload: None,
                    payload_hash: None,
                    prev_hash: "genesis".to_string(),
                    entry_hash: "h3".to_string(),
                },
            ],
        }];

        let sessions = collect_session_summaries(&traces);
        assert_eq!(sessions.len(), 2, "expected one summary per session");
        let s1 = sessions
            .iter()
            .find(|s| s.session_id == "s1")
            .expect("s1 should be present");
        assert_eq!(s1.event_count, 2);
        assert_eq!(s1.max_event_seq, 2);
    }
}
