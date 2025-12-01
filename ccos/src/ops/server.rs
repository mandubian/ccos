//! Server operations - pure logic functions for server management

use crate::discovery::{
    ApprovalQueue, DiscoverySource, PendingDiscovery, RegistrySearcher, RiskAssessment, RiskLevel,
    ServerInfo as DiscoveryServerInfo,
};
use crate::mcp::core::MCPDiscoveryService;
use crate::capability_marketplace::mcp_discovery::MCPServerConfig;
use crate::mcp::types::DiscoveryOptions;
use crate::synthesis::introspection::api_introspector::APIIntrospector;
use chrono::Utc;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::io::{self, Write};
use uuid::Uuid;
use super::{ServerInfo, ServerListOutput};

/// List configured servers
pub async fn list_servers() -> RuntimeResult<ServerListOutput> {
    let queue = ApprovalQueue::new("."); // TODO: use configured path
    let approved = queue.list_approved()?;

    let servers: Vec<ServerInfo> = approved.into_iter().map(|server| {
        let id = server.id.clone();
        let name = server.server_info.name.clone();
        let endpoint = server.server_info.endpoint.clone();
        ServerInfo {
            id,
            name,
            endpoint,
            status: if server.should_dismiss() {
                "failing".to_string()
            } else {
                "healthy".to_string()
            },
            health_score: Some(server.error_rate()),
        }
    }).collect();

    Ok(ServerListOutput {
        servers: servers.clone(),
        count: servers.len(),
    })
}

/// Add a new server
pub async fn add_server(url: String, name: Option<String>) -> RuntimeResult<String> {
    let queue = ApprovalQueue::new(".");
    let name_str = name.unwrap_or_else(|| "manual-server".to_string());

    let discovery = PendingDiscovery {
        id: format!("manual-{}", Uuid::new_v4()),
        source: DiscoverySource::Manual {
            user: "cli".to_string(),
        },
        server_info: DiscoveryServerInfo {
            name: name_str.clone(),
            endpoint: url.clone(),
            description: Some("Manually added via CLI".to_string()),
            auth_env_var: Some(ApprovalQueue::suggest_auth_env_var(&name_str)),
        },
        domain_match: vec![],
        risk_assessment: RiskAssessment {
            level: RiskLevel::Low,
            reasons: vec!["manual_add".to_string()],
        },
        requested_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::hours(24 * 30),
        requesting_goal: None,
    };

    queue.add(discovery.clone())?;
    Ok(discovery.id)
}

/// Remove a server
pub async fn remove_server(name: String) -> RuntimeResult<()> {
    // TODO: Implement server removal logic
    Ok(())
}

/// Show server health status
pub async fn server_health(name: Option<String>) -> RuntimeResult<Vec<ServerInfo>> {
    let queue = ApprovalQueue::new(".");
    let approved = queue.list_approved()?;

    let target_servers: Vec<_> = if let Some(n) = name {
        approved
            .into_iter()
            .filter(|s| s.server_info.name == n || s.id == n)
            .collect()
    } else {
        approved
    };

    let health_info = target_servers.into_iter().map(|server| {
        let id = server.id.clone();
        let name = server.server_info.name.clone();
        let endpoint = server.server_info.endpoint.clone();
        ServerInfo {
            id,
            name,
            endpoint,
            status: if server.should_dismiss() {
                "failing".to_string()
            } else {
                "healthy".to_string()
            },
            health_score: Some(server.error_rate()),
        }
    }).collect();

    Ok(health_info)
}

/// Search for servers in registry and overrides
pub async fn search_servers(
    query: String,
    capability: Option<String>,
    _llm: bool,
    _llm_model: Option<String>,
) -> RuntimeResult<Vec<ServerInfo>> {
    let searcher = RegistrySearcher::new();
    let initial_results = searcher.search(&query).await?;

    // For now, return the initial results without filtering
    // TODO: Implement capability filtering and LLM-based discovery
    Ok(initial_results
        .into_iter()
        .map(|result| {
            let server_info = result.server_info;
            ServerInfo {
                id: server_info.name.clone(),
                name: server_info.name,
                endpoint: server_info.endpoint,
                status: "pending".to_string(),
                health_score: None,
            }
        })
        .collect())
}