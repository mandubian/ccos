// Agent Simple* types moved under ccos::agent::types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimpleAgentCard {
    pub agent_id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub capabilities: Vec<String>,
    pub endpoint: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimpleDiscoveryQuery {
    pub capability_id: Option<String>,
    pub version_constraint: Option<String>,
    pub agent_id: Option<String>,
    pub discovery_tags: Option<Vec<String>>,
    pub discovery_query: Option<HashMap<String, serde_json::Value>>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimpleDiscoveryOptions {
    pub timeout_ms: Option<u64>,
    pub cache_policy: Option<SimpleCachePolicy>,
    pub include_offline: Option<bool>,
    pub max_results: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SimpleCachePolicy {
    UseCache,
    NoCache,
    RefreshCache,
}
