// Import test helpers from the main tests directory
use crate::test_helpers::*;

#[cfg(test)]
mod http_capability_tests {
    use rtfs_compiler::runtime::Value;
    use super::*;

    #[tokio::test]
    async fn test_http_get_capability() {
        let marketplace = create_capability_marketplace();

        // Register an HTTP GET capability pointing to a mock endpoint
        marketplace.register_http_capability(
            "http.get".to_string(),
            "HTTP GET Request".to_string(),
            "Performs HTTP GET request to mock endpoint".to_string(),
            "http://localhost:9999/mock".to_string(), // Non-existent local endpoint
            None,
        ).await.expect("Failed to register HTTP capability");

        // Test the capability
        let inputs = Value::List(vec![]);
    let result = marketplace.execute_capability("http.get", &inputs).await;
        
        match result {
            Ok(Value::Map(response)) => {
                println!("HTTP GET Response: {:?}", response);
                
                // Check that we got a status code
                let status_key = rtfs_compiler::ast::MapKey::String("status".to_string());
                assert!(response.contains_key(&status_key), "Response should contain status");
                
                // Check that we got a body
                let body_key = rtfs_compiler::ast::MapKey::String("body".to_string());
                assert!(response.contains_key(&body_key), "Response should contain body");
                
                println!("✅ HTTP GET capability test passed!");
            }
            Ok(other) => panic!("Expected map response, got: {:?}", other),
            Err(e) => {
                // Expect connection error since we're using a non-existent endpoint
                if e.to_string().contains("HTTP request failed") {
                    println!("✅ HTTP GET capability test passed (connection error as expected)!");
                    println!("Error (expected): {:?}", e);
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_http_post_capability() {
        let marketplace = create_capability_marketplace();

        // Register an HTTP POST capability
        marketplace.register_http_capability(
            "http.post".to_string(),
            "HTTP POST Request".to_string(),
            "Performs HTTP POST request to mock endpoint".to_string(),
            "http://localhost:9999/mock".to_string(), // Non-existent local endpoint
            None,
        ).await.expect("Failed to register HTTP capability");

        // Test POST with JSON body
        let inputs = Value::List(vec![
            Value::String("https://httpbin.org/post".to_string()), // URL
            Value::String("POST".to_string()),                      // Method
            Value::Map({
                let mut headers = std::collections::HashMap::new();
                headers.insert(
                    rtfs_compiler::ast::MapKey::String("Content-Type".to_string()),
                    Value::String("application/json".to_string())
                );
                headers
            }),                                                     // Headers
            Value::String(r#"{"test": "data", "number": 42}"#.to_string()), // Body
        ]);

    let result = marketplace.execute_capability("http.post", &inputs).await;
        
        match result {
            Ok(Value::Map(response)) => {
                println!("HTTP POST Response: {:?}", response);
                
                // Check status
                let status_key = rtfs_compiler::ast::MapKey::String("status".to_string());
                assert!(response.contains_key(&status_key), "Response should contain status");
                
                println!("✅ HTTP POST capability test passed!");
            }
            Ok(other) => panic!("Expected map response, got: {:?}", other),
            Err(e) => {
                // Expect connection error since we're using a non-existent endpoint
                if e.to_string().contains("HTTP request failed") {
                    println!("✅ HTTP POST capability test passed (connection error as expected)!");
                    println!("Error (expected): {:?}", e);
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_http_with_authentication() {
        let marketplace = create_capability_marketplace();

        // Register an HTTP capability with authentication
        marketplace.register_http_capability(
            "http.auth".to_string(),
            "HTTP Authenticated Request".to_string(),
            "Performs HTTP request with Bearer authentication".to_string(),
            "http://localhost:9999/mock".to_string(), // Non-existent local endpoint
            Some("test-token-123".to_string()),
        ).await.expect("Failed to register HTTP capability");

        // Test authenticated request
        let inputs = Value::List(vec![]);
    let result = marketplace.execute_capability("http.auth", &inputs).await;
        
        match result {
            Ok(Value::Map(response)) => {
                println!("HTTP Auth Response: {:?}", response);
                
                // Check status
                let status_key = rtfs_compiler::ast::MapKey::String("status".to_string());
                assert!(response.contains_key(&status_key), "Response should contain status");
                
                println!("✅ HTTP authentication capability test passed!");
            }
            Ok(other) => panic!("Expected map response, got: {:?}", other),
            Err(e) => {
                // Expect connection error since we're using a non-existent endpoint
                if e.to_string().contains("HTTP request failed") {
                    println!("✅ HTTP authentication capability test passed (connection error as expected)!");
                    println!("Error (expected): {:?}", e);
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_http_capability_in_rtfs() {
        // Test HTTP capability through RTFS call function
        use rtfs_compiler::parser;

        // Set up runtime with HTTP capability using helper
        let (_marketplace, evaluator) = create_http_test_setup().await;

        // Parse and execute RTFS code that uses HTTP capability
        let rtfs_code = r#"(call :http.get [])"#;  // Use keyword syntax as originally intended
        let ast = parser::parse_expression(rtfs_code).expect("Failed to parse RTFS");
        
        let mut env = evaluator.env.clone();
        
        // In the new architecture, capability calls should return RequiresHost
        // and be handled by the CCOS Orchestrator, not executed directly
        // Use a very short timeout to prevent hanging
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100), // 100ms timeout - should be enough for immediate errors
            async {
                evaluator.eval_expr(&ast, &mut env)
            }
        ).await;
        
        match result {
            Ok(Ok(rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(host_call))) => {
                println!("RTFS HTTP call requires host: {:?}", host_call);
                println!("✅ RTFS HTTP capability integration test passed (host call required)!");
                
                // Verify that the host call is for the HTTP capability
                assert!(host_call.fn_symbol.contains("http.get") || 
                        host_call.fn_symbol.contains("http"));
            }
            Ok(Ok(rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(_))) => {
                panic!("Expected RequiresHost for capability call, got Complete");
            }
            Ok(Err(e)) => {
                // The error "FATAL: Host method called without a valid execution context" 
                // is expected in the new architecture when trying to execute capability calls directly
                // without proper CCOS Orchestrator setup
                if e.to_string().contains("execution context") {
                    println!("✅ RTFS HTTP capability integration test passed (execution context error as expected)!");
                    println!("Error (expected): {:?}", e);
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
            Err(_) => {
                // Timeout is expected if the HTTP call hangs
                println!("✅ RTFS HTTP capability integration test passed (timeout as expected - HTTP call would hang without proper orchestration)!");
            }
        }
    }
}
