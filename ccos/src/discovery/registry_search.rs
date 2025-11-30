use crate::mcp::registry::MCPRegistryClient;
use crate::discovery::approval_queue::{DiscoverySource, ServerInfo};
use rtfs::runtime::error::RuntimeResult;

pub struct RegistrySearcher {
    mcp_client: MCPRegistryClient,
}

#[derive(Debug, Clone)]
pub struct RegistrySearchResult {
    pub source: DiscoverySource,
    pub server_info: ServerInfo,
    pub match_score: f32,
}

impl RegistrySearcher {
    pub fn new() -> Self {
        Self {
            mcp_client: MCPRegistryClient::new(),
        }
    }

    pub async fn search(&self, query: &str) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let mcp_servers = self.mcp_client.search_servers(query).await?;
        
        let results = mcp_servers.into_iter().map(|server| {
            let endpoint = if let Some(remotes) = &server.remotes {
                MCPRegistryClient::select_best_remote_url(remotes).unwrap_or_default()
            } else {
                String::new()
            };
            
            RegistrySearchResult {
                source: DiscoverySource::McpRegistry { name: server.name.clone() },
                server_info: ServerInfo {
                    name: server.name,
                    endpoint,
                    description: Some(server.description),
                },
                match_score: 1.0, // Default score
            }
        }).collect();
        
        Ok(results)
    }
}

