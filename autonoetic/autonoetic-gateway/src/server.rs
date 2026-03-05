//! Gateway Event Loop Server.

use autonoetic_types::config::GatewayConfig;
use std::sync::Arc;

pub struct GatewayServer {
    config: Arc<GatewayConfig>,
}

impl GatewayServer {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Run the main event loop for the Gateway daemon.
    pub async fn run(&self) -> anyhow::Result<()> {
        let port = self.config.port;
        tracing::info!("GatewayServer starting on port {}", port);

        // Stub for JSON-RPC IPC listener
        // TODO: Bind Unix socket / loopback TCP here

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    }
}
