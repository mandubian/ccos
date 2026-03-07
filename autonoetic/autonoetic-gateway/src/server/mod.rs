//! Gateway Event Loop Server.

use crate::server::registry::PeerRegistry;
use autonoetic_types::config::GatewayConfig;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

pub mod jsonrpc;
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
        let jsonrpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.config.port);
        let ofp_addr: SocketAddr = format!("0.0.0.0:{}", self.config.ofp_port)
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid OFP bind address: {}", e))?;
        let jsonrpc_router = Arc::new(crate::router::JsonRpcRouter::new(
            self.config.as_ref().clone(),
        ));
        let background_scheduler =
            crate::scheduler::start_background_scheduler(jsonrpc_router.execution_service());

        tracing::info!(
            "GatewayServer starting (jsonrpc_port={}, ofp_port={}, node_id={})",
            self.config.port,
            self.config.ofp_port,
            node_id
        );

        // Phase 5/7: start OFP and JSON-RPC listeners concurrently.
        // Missing federation identity is a hard failure by design.
        tokio::try_join!(
            ofp::start_ofp_server(
                ofp_addr,
                node_id,
                node_name,
                shared_secret,
                self.registry.clone(),
                jsonrpc_router.clone(),
            ),
            jsonrpc::start_jsonrpc_server(jsonrpc_addr, (*jsonrpc_router).clone()),
            background_scheduler,
        )?;
        Ok(())
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    std::env::var(name)
        .map_err(|_| anyhow::anyhow!("Missing required environment variable {}", name))
}
