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
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::io::{Read, Write};
use std::thread;
use std::sync::Once;

/// Creates a minimal host for RTFS tests with specified security context
fn create_minimal_host_with_context(security_context: RuntimeContext) -> Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry));
    let causal_chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    
    Arc::new(RuntimeHost::new(
        causal_chain,
        capability_marketplace,
        security_context,
    ))
}

/// Creates a minimal host for pure RTFS tests (with Pure security context)
fn create_minimal_host() -> Arc<dyn rtfs_compiler::runtime::host_interface::HostInterface> {
    create_minimal_host_with_context(RuntimeContext::pure())
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
    let host = create_minimal_host_with_context(security_context.clone());
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
    let host = create_minimal_host_with_context(security_context.clone());
    let mut evaluator = Evaluator::new(module_registry, security_context, host);
    
    // Load the standard library
    let stdlib_env = StandardLibrary::create_global_environment();
    evaluator.env = stdlib_env;
    
    evaluator
}

/// Mock HTTP server for testing HTTP capabilities
pub struct MockHttpServer {
    addr: SocketAddr,
    _handle: thread::JoinHandle<()>,
}

impl MockHttpServer {
    /// Start a real mock HTTP server on localhost:9999
    pub fn start() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let addr = SocketAddr::from(([127, 0, 0, 1], 9999));
        let listener = TcpListener::bind(addr)?;
        
        // Start the server in a background thread
        let handle = thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        // Handle each connection in a separate thread
                        thread::spawn(move || {
                            handle_connection(stream);
                        });
                    }
                    Err(e) => {
                        eprintln!("Error accepting connection: {}", e);
                    }
                }
            }
        });
        
        // Give the server a moment to start
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        Ok(MockHttpServer { 
            addr, 
            _handle: handle 
        })
    }
    
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

/// Handle a single HTTP connection
fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0; 1024];
    
    if let Ok(size) = stream.read(&mut buffer) {
        let request = String::from_utf8_lossy(&buffer[..size]);
        
        // Parse the request to extract method and path
        let (method, path) = parse_request(&request);
        let response_body = get_mock_response(&path, &method);
        
        // Create HTTP response
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n\
             {}",
            response_body.len(),
            response_body
        );
        
        if let Err(e) = stream.write_all(response.as_bytes()) {
            eprintln!("Error writing response: {}", e);
        }
    }
}

/// Parse HTTP request to extract method and path
fn parse_request(request: &str) -> (String, String) {
    let lines: Vec<&str> = request.lines().collect();
    if let Some(first_line) = lines.first() {
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        if parts.len() >= 2 {
            let method = parts[0].to_string();
            let path = parts[1].to_string();
            return (method, path);
        }
    }
    ("GET".to_string(), "/mock".to_string())
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

/// Global mock server that starts once and is available for all tests
static MOCK_SERVER_INIT: Once = Once::new();
static mut MOCK_SERVER_STARTED: bool = false;

/// Ensure the global mock server is started
pub fn ensure_mock_server() {
    MOCK_SERVER_INIT.call_once(|| {
        // Try to start the mock server
        if let Ok(_server) = MockHttpServer::start() {
            unsafe {
                MOCK_SERVER_STARTED = true;
            }
            println!("Global mock HTTP server started on localhost:9999");
        } else {
            eprintln!("Warning: Failed to start global mock HTTP server");
        }
    });
}

/// Check if the mock server is available
pub fn is_mock_server_available() -> bool {
    unsafe { MOCK_SERVER_STARTED }
}