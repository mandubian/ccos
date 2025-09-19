// Minimal test helpers for RTFS-only tests
// This provides only the essential functions needed for pure RTFS testing

use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::ccos::host::RuntimeHost;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::causal_chain::CausalChain;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;
use std::net::SocketAddr;

/// Creates a minimal host for pure RTFS tests
fn create_minimal_host() -> Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let security_context = RuntimeContext::pure();
    
    Arc::new(RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context,
    ))
}

/// Creates a pure RTFS evaluator without CCOS dependencies
pub fn create_pure_evaluator() -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::pure();
    let host = create_minimal_host();
    let mut evaluator = Evaluator::new(module_registry, security_context, host);
    
    // Load the standard library
    let stdlib_env = StandardLibrary::create_global_environment();
    evaluator.env = stdlib_env;
    
    evaluator
}

/// Creates a pure RTFS evaluator with custom security context
pub fn create_pure_evaluator_with_context(security_context: RuntimeContext) -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let host = create_minimal_host();
    let mut evaluator = Evaluator::new(module_registry, security_context, host);
    
    // Load the standard library
    let stdlib_env = StandardLibrary::create_global_environment();
    evaluator.env = stdlib_env;
    
    evaluator
}

/// Creates a full RTFS evaluator with all capabilities allowed
pub fn create_full_evaluator() -> Evaluator {
    let module_registry = Arc::new(ModuleRegistry::new());
    let security_context = RuntimeContext::full();
    let host = create_minimal_host();
    let mut evaluator = Evaluator::new(module_registry, security_context, host);
    
    // Load the standard library
    let stdlib_env = StandardLibrary::create_global_environment();
    evaluator.env = stdlib_env;
    
    evaluator
}

/// Mock HTTP server for testing HTTP capabilities
/// This is a simple mock that doesn't actually start a server,
/// but provides mock responses for testing
pub struct MockHttpServer {
    _addr: SocketAddr,
}

impl MockHttpServer {
    /// Create a mock HTTP server (no actual server started)
    pub async fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = SocketAddr::from(([127, 0, 0, 1], 9999));
        
        // For now, just return a mock server struct
        // In a real implementation, this would start an actual HTTP server
        Ok(MockHttpServer { _addr: addr })
    }
    
    pub fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 9999))
    }
}

/// Mock HTTP response generator for testing
pub fn get_mock_response(path: &str, method: &str) -> String {
    match (method, path) {
        ("GET", "/mock") => r#"{"message": "Mock GET response", "status": "ok"}"#.to_string(),
        ("GET", "/mock/get") => r#"{"method": "GET", "url": "/mock/get", "status": "success"}"#.to_string(),
        ("GET", "/mock/headers") => r#"{"headers": "received", "method": "GET", "status": "success"}"#.to_string(),
        ("POST", "/mock/post") => r#"{"method": "POST", "status": "success", "data": "received"}"#.to_string(),
        ("PUT", "/mock/put") => r#"{"method": "PUT", "status": "success", "data": "updated"}"#.to_string(),
        ("DELETE", "/mock/delete") => r#"{"method": "DELETE", "status": "success", "data": "deleted"}"#.to_string(),
        ("PATCH", "/mock/patch") => r#"{"method": "PATCH", "status": "success", "data": "patched"}"#.to_string(),
        ("HEAD", "/mock/headers") => r#"{"headers": "received", "method": "HEAD", "status": "success"}"#.to_string(),
        ("GET", "/mock/json") => r#"{"name": "test", "value": 42, "active": true}"#.to_string(),
        _ => format!(r#"{{"error": "Not found", "path": "{}", "method": "{}"}}"#, path, method),
    }
}