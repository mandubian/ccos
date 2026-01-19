//! CCOS Operations Module
//!
//! This module contains pure logic functions extracted from CLI commands.
//! These functions return RuntimeResult<T> with serializable structs and are
//! used both by CLI commands and as native capabilities.

pub mod approval;
pub mod browser_discovery;
pub mod config;
pub mod discover;
pub mod fs;
pub mod governance;
pub mod introspection_service;
pub mod llm;
pub mod native;
pub mod plan;
pub mod server;
pub mod server_discovery_pipeline;

#[cfg(test)]
mod tests;

/// Server information for ops functions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub description: Option<String>,
    pub source: Option<String>,
    pub matching_capabilities: Option<Vec<String>>,
    pub status: String,
    pub health_score: Option<f64>,
    pub auth_status: Option<String>,
}

/// Server list output
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerListOutput {
    pub servers: Vec<ServerInfo>,
    pub count: usize,
}

/// Approval type for better categorization in CLI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ApprovalType {
    ServerDiscovery,
    Effect,
    LlmPrompt,
    Synthesis,
}

/// Approval queue item
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApprovalItem {
    pub id: String,
    pub approval_type: ApprovalType,
    pub title: String,       // e.g. Server Name, Capability ID, or Synthesis ID
    pub description: String, // e.g. Endpoint, Intent, or Synthesis Goal
    pub risk_level: String,
    pub source: String, // Who requested it (e.g. "cli", "agent", "planner")
    pub goal: Option<String>,
    pub status: String,
    pub requested_at: String,
}

/// Approval list output
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApprovalListOutput {
    pub items: Vec<ApprovalItem>,
    pub count: usize,
}

/// Config information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfigInfo {
    pub config_path: String,
    pub warnings: Vec<String>,
    pub is_valid: bool,
}
