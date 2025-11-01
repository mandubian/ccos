//! Generic Session Pool Manager
//!
//! Provides provider-agnostic session management infrastructure.
//! Delegates to provider-specific handlers based on capability metadata.

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Unique identifier for a session
pub type SessionId = String;

/// Generic session handler trait
///
/// Each provider (MCP, GraphQL, gRPC, etc.) implements this trait
/// to manage sessions according to its own protocol.
///
/// The trait is intentionally minimal to support a wide variety of
/// session patterns (stateful, token-based, connection-pooled, etc.)
pub trait SessionHandler: Send + Sync {
    /// Initialize a new session for a capability
    ///
    /// Reads provider-specific metadata (server URL, auth env var, etc.)
    /// and performs protocol-specific initialization (e.g., MCP initialize endpoint).
    ///
    /// Returns a SessionId that can be used for subsequent calls.
    fn initialize_session(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
    ) -> RuntimeResult<SessionId>;

    /// Execute capability call with an existing session
    ///
    /// Uses the session identified by `session_id` to execute the capability.
    /// Provider-specific (e.g., adds Mcp-Session-Id header for MCP).
    fn execute_with_session(
        &self,
        session_id: &SessionId,
        capability_id: &str,
        args: &[Value],
    ) -> RuntimeResult<Value>;

    /// Terminate a session (cleanup)
    ///
    /// Performs protocol-specific cleanup (e.g., MCP terminate endpoint).
    /// After this call, the SessionId is invalid.
    fn terminate_session(&self, session_id: &SessionId) -> RuntimeResult<()>;

    /// Get or reuse an existing session for a capability
    ///
    /// Default implementation always creates a new session.
    /// Override to implement session pooling/reuse.
    fn get_or_create_session(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
    ) -> RuntimeResult<SessionId> {
        self.initialize_session(capability_id, metadata)
    }
}

/// Generic session pool manager
///
/// Routes session requests to provider-specific handlers based on
/// capability metadata. Maintains zero knowledge of provider protocols.
pub struct SessionPoolManager {
    /// Registry of session handlers by provider type
    handlers: HashMap<String, Arc<dyn SessionHandler>>,
}

impl SessionPoolManager {
    /// Create a new session pool manager
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for a provider type
    ///
    /// # Arguments
    /// * `provider_type` - Provider identifier (e.g., "mcp", "graphql")
    /// * `handler` - Provider-specific session handler
    ///
    /// # Example
    /// ```ignore
    /// let mut pool = SessionPoolManager::new();
    /// pool.register_handler("mcp", Arc::new(MCPSessionHandler::new()));
    /// pool.register_handler("graphql", Arc::new(GraphQLSessionHandler::new()));
    /// ```
    pub fn register_handler(&mut self, provider_type: &str, handler: Arc<dyn SessionHandler>) {
        self.handlers.insert(provider_type.to_string(), handler);
    }

    /// Execute capability with session management (generic)
    ///
    /// This is the main entry point. It:
    /// 1. Determines provider type from metadata
    /// 2. Routes to appropriate handler
    /// 3. Gets or creates session
    /// 4. Executes capability
    /// 5. Handles errors (can extend with retry logic)
    ///
    /// # Generic Routing Logic
    /// Provider type is inferred from metadata keys:
    /// - Keys starting with "mcp_" → "mcp" provider
    /// - Keys starting with "graphql_" → "graphql" provider
    /// - etc.
    pub fn execute_with_session(
        &self,
        capability_id: &str,
        metadata: &HashMap<String, String>,
        args: &[Value],
    ) -> RuntimeResult<Value> {
        // Determine provider type from metadata (generic detection)
        let provider_type = self.detect_provider_type(metadata)?;

        // Get handler for this provider
        let handler = self.handlers.get(&provider_type).ok_or_else(|| {
            RuntimeError::Generic(format!(
                "No session handler registered for provider type: {}",
                provider_type
            ))
        })?;

        // Get or create session
        let session_id = handler.get_or_create_session(capability_id, metadata)?;

        // Execute with session
        handler.execute_with_session(&session_id, capability_id, args)
    }

    /// Detect provider type from metadata keys (generic)
    ///
    /// Looks for metadata keys with provider-specific prefixes.
    /// This pattern scales to unlimited providers without code changes.
    ///
    /// # Examples
    /// - "mcp_server_url" → "mcp"
    /// - "graphql_endpoint" → "graphql"
    /// - "grpc_target" → "grpc"
    fn detect_provider_type(&self, metadata: &HashMap<String, String>) -> RuntimeResult<String> {
        // Check for known provider-specific keys
        for (key, _) in metadata.iter() {
            if key.starts_with("mcp_") {
                return Ok("mcp".to_string());
            } else if key.starts_with("graphql_") {
                return Ok("graphql".to_string());
            } else if key.starts_with("grpc_") {
                return Ok("grpc".to_string());
            }
            // Future providers: add more prefixes here
        }

        Err(RuntimeError::Generic(
            "Could not detect provider type from metadata keys".to_string(),
        ))
    }

    /// Terminate all sessions for a capability (cleanup)
    ///
    /// Useful for graceful shutdown or when a capability is unloaded.
    pub fn terminate_all_sessions(&self, capability_id: &str) -> RuntimeResult<()> {
        // Future: track active sessions per capability
        // For now, this is a no-op (handlers manage their own pools)
        eprintln!(
            "SessionPoolManager: terminate_all_sessions for {} (not yet implemented)",
            capability_id
        );
        Ok(())
    }
}

impl Default for SessionPoolManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock session handler for testing
    struct MockSessionHandler {
        name: String,
    }

    impl SessionHandler for MockSessionHandler {
        fn initialize_session(
            &self,
            _capability_id: &str,
            _metadata: &HashMap<String, String>,
        ) -> RuntimeResult<SessionId> {
            Ok(format!("{}_session_123", self.name))
        }

        fn execute_with_session(
            &self,
            session_id: &SessionId,
            capability_id: &str,
            _args: &[Value],
        ) -> RuntimeResult<Value> {
            Ok(Value::String(format!(
                "Executed {} with session {}",
                capability_id, session_id
            )))
        }

        fn terminate_session(&self, _session_id: &SessionId) -> RuntimeResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_provider_detection() {
        let pool = SessionPoolManager::new();

        // MCP provider
        let mut mcp_meta = HashMap::new();
        mcp_meta.insert(
            "mcp_server_url".to_string(),
            "https://mcp.example.com".to_string(),
        );
        assert_eq!(pool.detect_provider_type(&mcp_meta).unwrap(), "mcp");

        // GraphQL provider
        let mut gql_meta = HashMap::new();
        gql_meta.insert(
            "graphql_endpoint".to_string(),
            "https://api.example.com/graphql".to_string(),
        );
        assert_eq!(pool.detect_provider_type(&gql_meta).unwrap(), "graphql");

        // Unknown provider
        let empty_meta = HashMap::new();
        assert!(pool.detect_provider_type(&empty_meta).is_err());
    }

    #[test]
    fn test_handler_registration_and_routing() {
        let mut pool = SessionPoolManager::new();

        // Register mock handlers
        pool.register_handler(
            "mcp",
            Arc::new(MockSessionHandler {
                name: "mcp".to_string(),
            }),
        );
        pool.register_handler(
            "graphql",
            Arc::new(MockSessionHandler {
                name: "graphql".to_string(),
            }),
        );

        // Test MCP routing
        let mut mcp_meta = HashMap::new();
        mcp_meta.insert(
            "mcp_server_url".to_string(),
            "https://mcp.example.com".to_string(),
        );

        let result = pool
            .execute_with_session("test.mcp.capability", &mcp_meta, &[])
            .unwrap();

        if let Value::String(s) = result {
            assert!(s.contains("mcp_session_123"));
            assert!(s.contains("test.mcp.capability"));
        } else {
            panic!("Expected string result");
        }

        // Test GraphQL routing
        let mut gql_meta = HashMap::new();
        gql_meta.insert(
            "graphql_endpoint".to_string(),
            "https://api.example.com/graphql".to_string(),
        );

        let result = pool
            .execute_with_session("test.graphql.capability", &gql_meta, &[])
            .unwrap();

        if let Value::String(s) = result {
            assert!(s.contains("graphql_session_123"));
            assert!(s.contains("test.graphql.capability"));
        } else {
            panic!("Expected string result");
        }
    }

    #[test]
    fn test_missing_handler() {
        let pool = SessionPoolManager::new(); // No handlers registered

        let mut meta = HashMap::new();
        meta.insert(
            "mcp_server_url".to_string(),
            "https://mcp.example.com".to_string(),
        );

        let result = pool.execute_with_session("test.capability", &meta, &[]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No session handler registered"));
    }
}
