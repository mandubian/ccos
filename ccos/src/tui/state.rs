//! TUI Application State
//!
//! Manages all state for the CCOS Control Center TUI:
//! - Multi-view navigation (Goals, Discover, Servers, etc.)
//! - Goal input and execution status  
//! - Decomposition tree nodes
//! - Capability resolutions
//! - Trace events timeline
//! - LLM prompts and responses

use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::Instant;

/// Maximum number of events to retain
const MAX_EVENTS: usize = 500;
const MAX_LLM_HISTORY: usize = 10;

/// Main navigation view in the Control Center
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Goals, // Goal input → plan construction
    Plans,     // Browse/manage saved plans (future)
    Execute,   // Live execution monitoring (future)
    Discover,  // Capability browser (future)
    Servers,   // MCP server management (future)
    Approvals, // Approval queue (future)
    Config,    // Configuration (future)
}

impl View {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Goals => "Goals",
            Self::Plans => "Plans",
            Self::Execute => "Execute",
            Self::Discover => "Discover",
            Self::Servers => "Servers",
            Self::Approvals => "Approvals",
            Self::Config => "Config",
        }
    }

    pub fn shortcut(&self) -> char {
        match self {
            Self::Goals => '1',
            Self::Plans => '2',
            Self::Execute => '3',
            Self::Discover => '4',
            Self::Servers => '5',
            Self::Approvals => '6',
            Self::Config => '7',
        }
    }

    pub fn all() -> &'static [View] {
        &[
            View::Goals,
            View::Plans,
            View::Execute,
            View::Discover,
            View::Servers,
            View::Approvals,
            View::Config,
        ]
    }

    pub fn is_implemented(&self) -> bool {
        matches!(self, Self::Goals | Self::Servers)
    }
}

/// Active panel across all views
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePanel {
    #[default]
    GoalInput,
    RtfsPlan,
    DecompositionTree,
    CapabilityResolution,
    TraceTimeline,
    LlmInspector,

    // Discover View Panels
    DiscoverList,
    DiscoverDetails,
    DiscoverInput,

    // Servers View Panels
    ServersList,
    ServerDetails,
}

impl ActivePanel {
    pub fn next(&self) -> Self {
        match self {
            // Goals View Navigation
            Self::GoalInput => Self::RtfsPlan,
            Self::RtfsPlan => Self::DecompositionTree,
            Self::DecompositionTree => Self::CapabilityResolution,
            Self::CapabilityResolution => Self::TraceTimeline,
            Self::TraceTimeline => Self::LlmInspector,
            Self::LlmInspector => Self::GoalInput,

            // Discover View Navigation
            Self::DiscoverInput => Self::DiscoverList,
            Self::DiscoverList => Self::DiscoverDetails,
            Self::DiscoverDetails => Self::DiscoverInput,

            // Servers View Navigation
            Self::ServersList => Self::ServerDetails,
            Self::ServerDetails => Self::ServersList,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            // Goals View Navigation
            Self::GoalInput => Self::LlmInspector,
            Self::RtfsPlan => Self::GoalInput,
            Self::DecompositionTree => Self::RtfsPlan,
            Self::CapabilityResolution => Self::DecompositionTree,
            Self::TraceTimeline => Self::CapabilityResolution,
            Self::LlmInspector => Self::TraceTimeline,

            // Discover View Navigation
            Self::DiscoverInput => Self::DiscoverDetails,
            Self::DiscoverList => Self::DiscoverInput,
            Self::DiscoverDetails => Self::DiscoverList,

            // Servers View Navigation
            Self::ServersList => Self::ServerDetails,
            Self::ServerDetails => Self::ServersList,
        }
    }
}

/// Execution mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    #[default]
    Idle,
    Received, // Goal received, about to start planning
    Planning,
    Executing,
    Complete,
    Error,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "IDLE"),
            Self::Received => write!(f, "RECEIVED"),
            Self::Planning => write!(f, "PLANNING"),
            Self::Executing => write!(f, "EXECUTING"),
            Self::Complete => write!(f, "COMPLETE"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Status of a decomposition node
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeStatus {
    Pending,
    Resolving,
    Resolved { capability: String },
    Synthesizing,
    Failed { reason: String },
    UserInput,
}

/// A node in the decomposition tree
#[derive(Debug, Clone)]
pub struct DecompNode {
    pub id: String,
    pub description: String,
    pub intent_type: String,
    pub status: NodeStatus,
    pub depth: usize,
    pub children: Vec<String>, // IDs of child nodes
    pub params: std::collections::HashMap<String, String>,
}

/// A capability resolution record
#[derive(Debug, Clone)]
pub struct CapabilityResolution {
    pub intent_id: String,
    pub intent_desc: String,
    pub capability_name: String,
    pub source: CapabilitySource,
    pub embed_score: Option<f32>,
    pub heuristic_score: Option<f32>,
    pub timestamp: Instant,
}

#[derive(Debug, Clone)]
pub enum CapabilitySource {
    McpServer(String),
    LocalRtfs(String),
    Synthesized,
    Builtin,
    Unknown,
}

/// Connection status for an MCP server
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerStatus {
    #[default]
    Unknown,
    Connected,
    Disconnected,
    Connecting,
    Error,
}

impl ServerStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Unknown => "○",
            Self::Connected => "●",
            Self::Disconnected => "○",
            Self::Connecting => "◐",
            Self::Error => "✗",
        }
    }
}

/// MCP Server information for TUI display
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub endpoint: String,
    pub status: ServerStatus,
    pub tool_count: Option<usize>,
    pub tools: Vec<String>,
    pub last_checked: Option<Instant>,
}

impl std::fmt::Display for CapabilitySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::McpServer(s) => write!(f, "MCP: {}", s),
            Self::LocalRtfs(p) => write!(f, "RTFS: {}", p),
            Self::Synthesized => write!(f, "Synthesized"),
            Self::Builtin => write!(f, "Builtin"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// A trace event for the timeline
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub timestamp: Instant,
    pub event_type: TraceEventType,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceEventType {
    DecompositionStart,
    DecompositionComplete,
    ToolDiscovery,
    LlmCall,
    ResolutionStart,
    ResolutionComplete,
    ResolutionFailed,
    SynthesisTriggered,
    LearningApplied,
    Error,
    Info,
}

impl TraceEventType {
    pub fn icon(&self) -> &'static str {
        match self {
            Self::DecompositionStart => "📂",
            Self::DecompositionComplete => "📂",
            Self::ToolDiscovery => "🔍",
            Self::LlmCall => "🤖",
            Self::ResolutionStart => "🔗",
            Self::ResolutionComplete => "✅",
            Self::ResolutionFailed => "❌",
            Self::SynthesisTriggered => "⚡",
            Self::LearningApplied => "📚",
            Self::Error => "⚠️",
            Self::Info => "ℹ️",
        }
    }

    /// Returns true if this is an important event that should always be shown
    pub fn is_important(&self) -> bool {
        match self {
            Self::DecompositionStart => true,
            Self::DecompositionComplete => true,
            Self::LlmCall => true,
            Self::ResolutionComplete => true,
            Self::ResolutionFailed => true,
            Self::SynthesisTriggered => true,
            Self::Error => true,
            // These are verbose/debug events
            Self::ToolDiscovery => false,
            Self::ResolutionStart => false,
            Self::LearningApplied => false,
            Self::Info => false,
        }
    }
}

/// Popup states for the Discovery search flow
#[derive(Debug, Clone, Default)]
pub enum DiscoverPopup {
    /// No popup visible
    #[default]
    None,
    /// Search results popup - shows matching servers
    SearchResults {
        servers: Vec<DiscoverySearchResult>,
        selected: usize,
    },
    /// Loading popup while introspecting a server
    Introspecting {
        server_name: String,
        endpoint: String,
        logs: Vec<String>,
    },
    /// Introspection results - shows discovered tools
    IntrospectionResults {
        server_name: String,
        endpoint: String,
        tools: Vec<DiscoveredCapability>,
        selected: usize,
        selected_tools: std::collections::HashSet<usize>,
    },
    /// Error popup
    Error { title: String, message: String },
}

/// Entry in the discovery list (header or capability)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEntry {
    Header { name: String, is_local: bool },
    Capability(usize), // index into discovered_capabilities
}

/// A server found in a discovery search (from MCP registry)
#[derive(Debug, Clone)]
pub struct DiscoverySearchResult {
    pub name: String,
    pub endpoint: String,
    pub description: Option<String>,
    pub source: String,
}

/// An LLM interaction record
#[derive(Debug, Clone)]
pub struct LlmInteraction {
    pub timestamp: Instant,
    pub model: String,
    pub prompt: String,
    pub response: Option<String>,
    pub tokens_prompt: usize,
    pub tokens_response: usize,
    pub duration_ms: u64,
}

/// Main application state
#[derive(Debug)]
pub struct AppState {
    // Navigation State
    pub current_view: View,
    pub active_panel: ActivePanel,
    pub should_quit: bool,
    pub show_help: bool,

    // Goal Input
    pub goal_input: String,
    pub cursor_position: usize,
    pub mode: ExecutionMode,

    // RTFS Plan (final generated program)
    pub rtfs_plan: Option<String>,
    pub rtfs_plan_scroll: usize,

    // Decomposition Tree
    pub decomp_nodes: Vec<DecompNode>,
    pub decomp_root_id: Option<String>,
    pub decomp_selected: usize,
    pub decomp_expanded: std::collections::HashSet<String>,
    pub show_intent_popup: bool,

    // Capability Resolution
    pub resolutions: VecDeque<CapabilityResolution>,
    pub resolution_selected: usize,

    // Trace Timeline
    pub trace_entries: VecDeque<TraceEntry>,
    pub trace_scroll: usize,
    pub trace_selected: usize,
    pub verbose_trace: bool, // Toggle to show all events vs only important ones
    pub show_trace_popup: bool, // Show popup with full trace details

    // LLM Inspector
    pub llm_history: VecDeque<LlmInteraction>,
    pub llm_selected: usize,
    pub llm_prompt_scroll: usize,
    pub llm_response_scroll: usize,

    // Stats for status bar
    pub capability_count: usize,
    pub learning_patterns_count: usize,

    // Timing
    pub start_time: Option<Instant>,

    // Animation
    pub spinner_frame: usize,

    // =========================================
    // Servers View State
    // =========================================
    pub servers: Vec<ServerInfo>,
    pub servers_selected: usize,
    pub servers_loading: bool,

    // =========================================
    // Discover State
    // =========================================
    pub discovered_capabilities: Vec<DiscoveredCapability>,
    pub discover_selected: usize,
    pub discover_loading: bool,
    pub discover_search_hint: String, // Formerly discover_filter
    pub discover_input_active: bool,
    pub discover_popup: DiscoverPopup,
    pub discover_local_collapsed: bool,
    pub discover_collapsed_sources: HashSet<String>,
}

/// A discovered capability for the Discover view
#[derive(Debug, Clone)]
pub struct DiscoveredCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String, // Server name or "Local"
    pub category: CapabilityCategory,
    pub version: Option<String>,
    pub input_schema: Option<String>,  // Stringified schema
    pub output_schema: Option<String>, // Stringified schema
    pub permissions: Vec<String>,
    pub effects: Vec<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Category for grouping capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityCategory {
    McpTool,
    RtfsFunction,
    Builtin,
    Synthesized,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_view: View::Goals,
            active_panel: ActivePanel::GoalInput,
            should_quit: false,
            show_help: false,

            goal_input: String::new(),
            cursor_position: 0,
            mode: ExecutionMode::Idle,

            rtfs_plan: None,
            rtfs_plan_scroll: 0,

            decomp_nodes: Vec::new(),
            decomp_root_id: None,
            decomp_selected: 0,
            decomp_expanded: std::collections::HashSet::new(),
            show_intent_popup: false,

            resolutions: VecDeque::with_capacity(MAX_EVENTS),
            resolution_selected: 0,

            trace_entries: VecDeque::with_capacity(MAX_EVENTS),
            trace_scroll: 0,
            trace_selected: 0,
            verbose_trace: false,
            show_trace_popup: false,

            llm_history: VecDeque::with_capacity(MAX_LLM_HISTORY),
            llm_selected: 0,
            llm_prompt_scroll: 0,
            llm_response_scroll: 0,

            capability_count: 0,
            learning_patterns_count: 0,

            start_time: None,

            spinner_frame: 0,

            // Servers View
            servers: Vec::new(),
            servers_selected: 0,
            servers_loading: false,

            // Discover View
            discovered_capabilities: Vec::new(),
            discover_selected: 0,
            discover_loading: false,
            discover_search_hint: String::new(),
            discover_input_active: false,
            discover_popup: DiscoverPopup::None,
            discover_local_collapsed: false,
            discover_collapsed_sources: HashSet::new(),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a trace entry
    pub fn add_trace(
        &mut self,
        event_type: TraceEventType,
        message: String,
        details: Option<String>,
    ) {
        if self.trace_entries.len() >= MAX_EVENTS {
            self.trace_entries.pop_front();
        }
        self.trace_entries.push_back(TraceEntry {
            timestamp: Instant::now(),
            event_type,
            message,
            details,
        });
    }

    /// Add an LLM interaction
    pub fn add_llm_interaction(&mut self, interaction: LlmInteraction) {
        if self.llm_history.len() >= MAX_LLM_HISTORY {
            self.llm_history.pop_front();
        }
        self.llm_history.push_back(interaction);
        self.llm_selected = self.llm_history.len().saturating_sub(1);
    }

    /// Add a capability resolution
    pub fn add_resolution(&mut self, resolution: CapabilityResolution) {
        if self.resolutions.len() >= MAX_EVENTS {
            self.resolutions.pop_front();
        }
        self.resolutions.push_back(resolution);
    }

    /// Clear all state for new goal
    pub fn reset_for_new_goal(&mut self) {
        self.decomp_nodes.clear();
        self.decomp_root_id = None;
        self.decomp_selected = 0;
        self.decomp_expanded.clear();
        self.rtfs_plan = None;
        self.rtfs_plan_scroll = 0;
        self.resolutions.clear();
        self.resolution_selected = 0;
        self.trace_entries.clear();
        self.trace_scroll = 0;
        self.llm_history.clear();
        self.llm_selected = 0;
        // Mode is set by caller (Received initially, then Planning when work starts)
        self.start_time = Some(Instant::now());
    }

    /// Get elapsed time since goal started
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Advance animation frame (call on each tick when running)
    pub fn tick(&mut self) {
        // Advance spinner for execution modes
        if self.mode == ExecutionMode::Received
            || self.mode == ExecutionMode::Planning
            || self.mode == ExecutionMode::Executing
            // Also advance for loading states
            || self.discover_loading
            || self.servers_loading
        {
            self.spinner_frame = (self.spinner_frame + 1) % 8;
        }
    }

    /// Get current spinner icon for running states
    pub fn spinner_icon(&self) -> &'static str {
        const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
        SPINNER_FRAMES[self.spinner_frame % 8]
    }

    /// Check if currently running
    pub fn is_running(&self) -> bool {
        matches!(
            self.mode,
            ExecutionMode::Received | ExecutionMode::Planning | ExecutionMode::Executing
        )
    }

    /// Return discovered capabilities filtered by the current search hint
    pub fn filtered_discovered_caps(&self) -> Vec<(usize, &DiscoveredCapability)> {
        if self.discover_search_hint.trim().is_empty() {
            return self.discovered_capabilities.iter().enumerate().collect();
        }

        let query = self.discover_search_hint.to_lowercase();
        self.discovered_capabilities
            .iter()
            .enumerate()
            .filter(|(_, cap)| {
                let name = cap.name.to_lowercase();
                let desc = cap.description.to_lowercase();
                let source = cap.source.to_lowercase();

                name.contains(&query) || desc.contains(&query) || source.contains(&query)
            })
            .collect()
    }

    /// Count of filtered discovered capabilities for selection bounds
    pub fn filtered_discovered_count(&self) -> usize {
        self.filtered_discovered_caps().len()
    }

    /// Return all visible discovery entries (headers + non-collapsed capabilities)
    pub fn visible_discovery_entries(&self) -> Vec<DiscoveryEntry> {
        let filtered_caps = self.filtered_discovered_caps();
        let mut entries = Vec::new();

        // Group filtered capabilities by source
        let mut by_source: std::collections::BTreeMap<String, Vec<usize>> =
            std::collections::BTreeMap::new();

        for (display_idx, (_, cap)) in filtered_caps.iter().enumerate() {
            by_source
                .entry(cap.source.clone())
                .or_default()
                .push(display_idx);
        }

        // First handle Local/Builtin capabilities (grouping multiple sources)
        let mut local_caps_indices = Vec::new();
        let local_source_names = ["Local Registry", "Local", "Core"];
        for name in local_source_names {
            if let Some(mut indices) = by_source.remove(name) {
                local_caps_indices.append(&mut indices);
            }
        }

        // Also handle "Known API: ..." sources as local
        let known_api_sources: Vec<String> = by_source
            .keys()
            .filter(|s| s.starts_with("Known API:"))
            .cloned()
            .collect();
        for name in known_api_sources {
            if let Some(mut indices) = by_source.remove(&name) {
                local_caps_indices.append(&mut indices);
            }
        }

        if !local_caps_indices.is_empty() {
            let header_name = "Local Capabilities".to_string();
            let collapsed = self.discover_local_collapsed
                || self.discover_collapsed_sources.contains(&header_name);

            entries.push(DiscoveryEntry::Header {
                name: header_name,
                is_local: true,
            });

            if !collapsed {
                // Sort local indices to keep display consistent
                local_caps_indices.sort();
                for idx in local_caps_indices {
                    entries.push(DiscoveryEntry::Capability(idx));
                }
            }
        }

        // Then handle MCP server capabilities
        for (source, caps_indices) in by_source {
            let collapsed = self.discover_collapsed_sources.contains(&source);

            entries.push(DiscoveryEntry::Header {
                name: source.clone(),
                is_local: false,
            });

            if !collapsed {
                for idx in caps_indices {
                    entries.push(DiscoveryEntry::Capability(idx));
                }
            }
        }

        entries
    }
}
