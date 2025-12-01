//! CCOS Operations Module
//!
//! This module contains pure logic functions extracted from CLI commands.
//! These functions return RuntimeResult<T> with serializable structs and are
//! used both by CLI commands and as native capabilities.

pub mod approval;
pub mod config;
pub mod discover;
pub mod governance;
pub mod plan;
pub mod server;

#[cfg(test)]
mod tests;


/// Server information for ops functions
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub status: String,
    pub health_score: Option<f64>,
}

/// Server list output
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerListOutput {
    pub servers: Vec<ServerInfo>,
    pub count: usize,
}

/// Approval queue item
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApprovalItem {
    pub id: String,
    pub server_name: String,
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