// Core types for DialoguePlanner

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Unique identifier for a dialogue session
pub type DialogueId = String;

/// Current state of the dialogue
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DialogueState {
    /// Initial state, analyzing goal
    Analyzing,
    /// Waiting for entity response
    WaitingForInput,
    /// Processing entity input
    Processing,
    /// Dialogue reached a plan
    Completed { plan_id: Option<String> },
    /// Dialogue was abandoned
    Abandoned { reason: String },
}

/// Level of autonomy for the dialogue
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AutonomyLevel {
    /// Human guides everything, CCOS suggests
    Guided,
    /// CCOS proposes, human approves
    Supervised,
    /// CCOS acts, human reviews after
    Reviewed,
    /// CCOS fully autonomous (governance-gated)
    Autonomous,
}

impl Default for AutonomyLevel {
    fn default() -> Self {
        AutonomyLevel::Guided
    }
}

/// Configuration for a dialogue session
#[derive(Clone, Debug)]
pub struct DialogueConfig {
    /// Starting autonomy level
    pub autonomy: AutonomyLevel,
    /// Maximum conversation turns before timeout
    pub max_turns: usize,
    /// Timeout per turn in seconds
    pub turn_timeout_secs: u64,
    /// Whether to persist conversation history
    pub persist_history: bool,
    /// Allow autonomous discovery of MCP servers
    pub allow_auto_discovery: bool,
    /// Allow autonomous synthesis
    pub allow_auto_synthesis: bool,
    /// Allow autonomous execution
    pub allow_auto_execution: bool,
}

impl Default for DialogueConfig {
    fn default() -> Self {
        Self {
            autonomy: AutonomyLevel::Guided,
            max_turns: 50,
            turn_timeout_secs: 300,
            persist_history: true,
            allow_auto_discovery: false,
            allow_auto_synthesis: false,
            allow_auto_execution: false,
        }
    }
}

/// A single turn in the conversation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationTurn {
    /// Turn number (0-indexed)
    pub index: usize,
    /// Who spoke
    pub speaker: Speaker,
    /// What was said
    pub message: String,
    /// Actions taken during this turn
    pub actions: Vec<TurnAction>,
    /// Timestamp
    #[serde(skip)]
    pub timestamp: Option<Instant>,
}

/// Who is speaking
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Speaker {
    /// CCOS is speaking
    Ccos,
    /// The external entity is speaking
    Entity,
    /// System message (internal)
    System,
}

/// Actions that can be taken during a turn
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TurnAction {
    /// Created new intent in graph
    IntentCreated {
        intent_id: String,
        description: String,
        parent: Option<String>,
    },

    /// Modified an existing intent
    IntentRefined {
        intent_id: String,
        old_description: String,
        new_description: String,
    },

    /// Resolved a capability for an intent
    CapabilityResolved {
        intent_id: String,
        capability_id: String,
        source: String, // "local", "remote", "synthesized", "builtin"
    },

    /// Discovered new MCP server(s)
    ServersDiscovered {
        domain: String,
        servers: Vec<DiscoveredServer>,
    },

    /// Connected to an MCP server
    ServerConnected {
        server_id: String,
        server_name: String,
        capabilities_count: usize,
    },

    /// Synthesized a new capability
    CapabilitySynthesized {
        capability_id: String,
        description: String,
        safety_status: String,
    },

    /// Queued something for approval
    ApprovalQueued {
        request_id: String,
        category: String,
        description: String,
    },

    /// Approval decision made
    ApprovalDecided { request_id: String, approved: bool },

    /// Executed a step for grounding
    StepExecuted {
        intent_id: String,
        capability_id: String,
        success: bool,
        result_preview: Option<String>,
    },

    /// Generated plan fragment
    PlanFragmentGenerated {
        rtfs_preview: String,
        intent_ids_covered: Vec<String>,
    },

    /// Goal analysis completed
    GoalAnalyzed {
        feasibility: f32,
        missing_domains: Vec<String>,
        suggestions_count: usize,
    },
}

/// Information about a discovered server
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveredServer {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub capabilities_preview: Vec<String>,
}

/// Goal analysis result
#[derive(Clone, Debug)]
pub struct GoalAnalysis {
    /// Original goal
    pub goal: String,
    /// Required capability domains (inferred)
    pub required_domains: Vec<String>,
    /// Available domains in marketplace
    pub available_domains: Vec<String>,
    /// Missing domains
    pub missing_domains: Vec<String>,
    /// Feasibility score (0.0 - 1.0)
    pub feasibility: f32,
    /// Suggestions for the entity
    pub suggestions: Vec<Suggestion>,
    /// Can we proceed without dialogue?
    pub can_proceed_immediately: bool,
}

/// Suggestion to improve goal feasibility
#[derive(Clone, Debug)]
pub enum Suggestion {
    /// Discover servers for a domain
    Discover {
        domain: String,
        example_servers: Vec<String>,
    },
    /// Synthesize a missing capability
    Synthesize {
        description: String,
        input_hint: Option<String>,
        output_hint: Option<String>,
    },
    /// Refine the goal to be more achievable
    RefineGoal { alternative: String, reason: String },
    /// Connect a specific known server
    ConnectServer {
        server_id: String,
        server_name: String,
        provides: Vec<String>,
    },
}

/// What the entity's input means
#[derive(Clone, Debug)]
pub enum InputIntent {
    /// Entity wants to refine the goal
    RefineGoal { new_goal: String },
    /// Entity wants to discover capabilities in a domain
    Discover { domain: String },
    /// Entity wants to connect a specific server
    ConnectServer { server_id: String },
    /// Entity wants to synthesize a capability
    Synthesize { description: String },
    /// Entity approved/rejected something
    Approval { request_id: String, approved: bool },
    /// Entity selected an option
    SelectOption { option_id: String },
    /// Entity provided information
    ProvideInfo { key: String, value: String },
    /// Entity wants to proceed with current plan
    Proceed,
    /// Entity wants to abandon the dialogue
    Abandon { reason: Option<String> },
    /// Entity asked a question (needs clarification)
    Question { text: String },
    /// Entity wants details on a specific result
    Details { index: usize },
    /// Entity wants to see more results
    ShowMore,
    /// Entity wants to go back to previous view
    Back,
    /// Entity wants to explore a documentation URL to find API links
    Explore { index: usize },
    /// Unclear input, needs re-prompting
    Unclear { raw_input: String },
}

/// Result of processing entity input
#[derive(Clone, Debug)]
pub struct ProcessingResult {
    /// Actions taken
    pub actions: Vec<TurnAction>,
    /// Next message to send to entity
    pub next_message: Option<String>,
    /// If we have a completed plan
    pub completed_plan: Option<CompletedPlan>,
    /// Should we continue the dialogue?
    pub should_continue: bool,
    /// Reason if abandoning
    pub abandon_reason: Option<String>,
}

/// A completed plan from dialogue
#[derive(Clone, Debug)]
pub struct CompletedPlan {
    /// The RTFS plan
    pub rtfs_plan: String,
    /// Intent IDs involved
    pub intent_ids: Vec<String>,
    /// Plan ID if archived
    pub plan_id: Option<String>,
    /// Conversation that produced this plan
    pub conversation_summary: String,
}

/// Complete dialogue history
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DialogueHistory {
    /// Unique dialogue ID
    pub id: DialogueId,
    /// Original goal
    pub goal: String,
    /// All turns in the conversation
    pub turns: Vec<ConversationTurn>,
    /// Final state
    pub final_state: DialogueState,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Resulting plan ID (if completed)
    pub result_plan_id: Option<String>,
}
