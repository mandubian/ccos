//! Gateway Event Loop Server.

use crate::server::registry::PeerRegistry;
use autonoetic_types::config::GatewayConfig;
use std::net::SocketAddr;
use std::sync::Arc;

pub mod ofp;
pub mod registry;
pub mod router;

pub struct GatewayServer {
    config: Arc<GatewayConfig>,
    registry: PeerRegistry,
}

impl GatewayServer {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config: Arc::new(config),
            registry: PeerRegistry::new(),
        }
    }

    /// Run the main event loop for the Gateway daemon.
    pub async fn run(&self) -> anyhow::Result<()> {
        let node_id = required_env("AUTONOETIC_NODE_ID")?;
        let node_name = required_env("AUTONOETIC_NODE_NAME")?;
        let shared_secret = required_env("AUTONOETIC_SHARED_SECRET")?;
        let ofp_addr: SocketAddr = format!("0.0.0.0:{}", self.config.ofp_port)
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid OFP bind address: {}", e))?;

        tracing::info!(
            "GatewayServer starting (jsonrpc_port={}, ofp_port={}, node_id={})",
            self.config.port,
            self.config.ofp_port,
            node_id
        );

        // Phase 5: start OFP listener in normal gateway startup path.
        // Missing federation identity is a hard failure by design.
        ofp::start_ofp_server(
            ofp_addr,
            node_id,
            node_name,
            shared_secret,
            self.registry.clone(),
        )
        .await
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|_| anyhow::anyhow!("Missing required environment variable {}", name))
}
