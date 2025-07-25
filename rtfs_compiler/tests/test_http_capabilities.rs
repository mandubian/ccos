// Import test helpers from the main tests directory
mod test_helpers;
use test_helpers::*;

#[cfg(test)]
mod http_capability_tests {
    use rtfs_compiler::runtime::Value;
    use super::*;

    #[tokio::test]
    async fn test_http_get_capability() {
        let marketplace = create_capability_marketplace();

        // Register an HTTP GET capability pointing to httpbin
        marketplace.register_http_capability(
            "http.get".to_string(),
            "HTTP GET Request".to_string(),
            "Performs HTTP GET request to httpbin".to_string(),
            "https://httpbin.org/get".to_string(),
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
                
                if let Some(Value::Integer(status)) = response.get(&status_key) {
                    assert_eq!(*status, 200, "HTTP status should be 200");
                }
                
                // Check that we got a body
                let body_key = rtfs_compiler::ast::MapKey::String("body".to_string());
                assert!(response.contains_key(&body_key), "Response should contain body");
                
                println!("✅ HTTP GET capability test passed!");
            }
            Ok(other) => panic!("Expected map response, got: {:?}", other),
            Err(e) => panic!("HTTP capability failed: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_http_post_capability() {
        let marketplace = create_capability_marketplace();

        // Register an HTTP POST capability
        marketplace.register_http_capability(
            "http.post".to_string(),
            "HTTP POST Request".to_string(),
            "Performs HTTP POST request to httpbin".to_string(),
            "https://httpbin.org/post".to_string(),
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
                if let Some(Value::Integer(status)) = response.get(&status_key) {
                    assert_eq!(*status, 200, "HTTP status should be 200");
                }
                
                println!("✅ HTTP POST capability test passed!");
            }
            Ok(other) => panic!("Expected map response, got: {:?}", other),
            Err(e) => panic!("HTTP POST capability failed: {:?}", e),
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
            "https://httpbin.org/bearer".to_string(),
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
                if let Some(Value::Integer(status)) = response.get(&status_key) {
                    assert_eq!(*status, 200, "HTTP status should be 200");
                }
                
                println!("✅ HTTP authentication capability test passed!");
            }
            Ok(other) => panic!("Expected map response, got: {:?}", other),
            Err(e) => panic!("HTTP auth capability failed: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_http_capability_in_rtfs() {
        // Test HTTP capability through RTFS call function
        use rtfs_compiler::parser;

        // Set up runtime with HTTP capability using helper
        let (_marketplace, evaluator) = create_http_test_setup().await;

        // Set up execution context (required for capability calls)
        setup_execution_context(evaluator.host.as_ref());

        // Parse and execute RTFS code that uses HTTP capability
        let rtfs_code = r#"(call :http.get [])"#;  // Use keyword syntax as originally intended
        let ast = parser::parse_expression(rtfs_code).expect("Failed to parse RTFS");
        
        let mut env = evaluator.env.clone();
        let result = evaluator.eval_expr(&ast, &mut env);
        
        match result {
            Ok(Value::Map(response)) => {
                println!("RTFS HTTP call result: {:?}", response);
                println!("✅ RTFS HTTP capability integration test passed!");
            }
            Ok(other) => panic!("Expected map response from RTFS, got: {:?}", other),
            Err(e) => panic!("RTFS HTTP capability call failed: {:?}", e),
        }
    }
}
