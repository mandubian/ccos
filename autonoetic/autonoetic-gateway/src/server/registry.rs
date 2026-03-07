//! OFP Peer Registry.
//!
//! Tracks connected OFP gateways and their advertised agents. Used to transparently
//! route messages to remote agents instead of local ones.

use autonoetic_ofp::wire::RemoteAgentInfo;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerState {
    Connected,
    Reconnecting(u32), // attempt number
    Disconnected,
}

#[derive(Debug, Clone)]
pub struct PeerEntry {
    pub node_id: String,
    pub node_name: String,
    pub address: SocketAddr,
    pub agents: Vec<RemoteAgentInfo>,
    pub state: PeerState,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    pub protocol_version: u32,
    pub negotiated_extensions: Vec<String>,
}

#[derive(Default, Clone)]
pub struct PeerRegistry {
    /// Maps node_id -> PeerEntry
    peers: Arc<RwLock<HashMap<String, PeerEntry>>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a peer connection.
    pub async fn add_peer(&self, entry: PeerEntry) {
        let mut w = self.peers.write().await;
        w.insert(entry.node_id.clone(), entry);
    }

    /// Mark a peer as disconnected.
    pub async fn mark_disconnected(&self, node_id: &str) {
        let mut w = self.peers.write().await;
        if let Some(peer) = w.get_mut(node_id) {
            peer.state = PeerState::Disconnected;
        }
    }

    /// Get a peer completely.
    pub async fn get_peer(&self, node_id: &str) -> Option<PeerEntry> {
        let r = self.peers.read().await;
        r.get(node_id).cloned()
    }

    /// Get all currently connected peers.
    pub async fn connected_peers(&self) -> Vec<PeerEntry> {
        let r = self.peers.read().await;
        r.values()
            .filter(|p| p.state == PeerState::Connected)
            .cloned()
            .collect()
    }

    /// Add an agent to a specific peer (e.g., from an AgentSpawned notification).
    pub async fn add_agent(&self, node_id: &str, agent: RemoteAgentInfo) {
        let mut w = self.peers.write().await;
        if let Some(peer) = w.get_mut(node_id) {
            peer.agents.retain(|a| a.id != agent.id); // Deduplicate
            peer.agents.push(agent);
        }
    }

    /// Remove an agent from a specific peer.
    pub async fn remove_agent(&self, node_id: &str, agent_id: &str) {
        let mut w = self.peers.write().await;
        if let Some(peer) = w.get_mut(node_id) {
            peer.agents.retain(|a| a.id != agent_id);
        }
    }

    /// Search across all connected peers to find which node hosts a specific agent ID.
    pub async fn resolve_agent_node(&self, agent_id: &str) -> Option<String> {
        let r = self.peers.read().await;
        for (node_id, peer) in r.iter() {
            if peer.state == PeerState::Connected {
                if peer
                    .agents
                    .iter()
                    .any(|a| a.id == agent_id || a.name == agent_id)
                {
                    return Some(node_id.clone());
                }
            }
        }
        None
    }

    /// Check whether a connected peer advertised a specific agent identity.
    pub async fn peer_hosts_agent(&self, node_id: &str, agent_id: &str) -> bool {
        let r = self.peers.read().await;
        r.get(node_id)
            .filter(|peer| peer.state == PeerState::Connected)
            .map(|peer| {
                peer.agents
                    .iter()
                    .any(|agent| agent.id == agent_id || agent.name == agent_id)
            })
            .unwrap_or(false)
    }
}
