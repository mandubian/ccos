//! Capability enums matching the `capabilities` block in SKILL.md.

use serde::{Deserialize, Serialize};

/// A typed capability that an Agent or Skill may request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Capability {
    ToolInvoke {
        allowed: Vec<String>,
    },
    MemoryRead {
        scopes: Vec<String>,
    },
    MemoryWrite {
        scopes: Vec<String>,
    },
    MemoryShare {
        allowed_targets: Vec<String>,
    },
    MemorySearch {
        scopes: Vec<String>,
    },
    NetConnect {
        hosts: Vec<String>,
    },
    AgentSpawn {
        max_children: u32,
    },
    AgentMessage {
        patterns: Vec<String>,
    },
    BackgroundReevaluation {
        min_interval_secs: u64,
        allow_reasoning: bool,
    },
    ShellExec {
        patterns: Vec<String>,
    },
}
