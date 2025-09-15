use rtfs_compiler::{
    ccos::capabilities::providers::weather_mcp::WeatherMCPCapability,
    runtime::{
        security::RuntimeContext,
    },
    ccos::capabilities::provider::CapabilityProvider,
};

#[tokio::test] 
async fn test_weather_mcp_integration() {
    // Test Weather MCP capability directly without complex runtime setup
    let weather_capability = WeatherMCPCapability::new();
    
    // Test provider interface
    assert_eq!(weather_capability.provider_id(), "weather_mcp");
    
    let capabilities = weather_capability.list_capabilities();
    assert_eq!(capabilities.len(), 2);
    
    // Test MCP tools
    let tools = weather_capability.get_mcp_tools();
    assert_eq!(tools.len(), 2);
    
    // Check tool names
    let tool_names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
    assert!(tool_names.contains(&"get_current_weather"));
    assert!(tool_names.contains(&"get_weather_forecast"));
    
    // Test current weather tool directly
    let context = RuntimeContext::controlled(vec!["weather.query".to_string()]);
    let arguments = serde_json::json!({
        "city": "London",
        "units": "metric"
    });
    
    let mut capability = WeatherMCPCapability::new();
    let result = capability.call_tool("get_current_weather", &arguments, &context).await;
    assert!(result.is_ok(), "Failed to call current weather tool: {:?}", result.err());
    
    let weather_data: serde_json::Value = result.unwrap();
    assert!(weather_data.is_object());
    assert!(weather_data["location"].as_str().unwrap() == "London");
    assert!(weather_data["temperature"].is_number());
    
    // Test forecast tool
    let forecast_args = serde_json::json!({
        "city": "Paris", 
        "days": 3
    });
    
    let forecast_result = capability.call_tool("get_weather_forecast", &forecast_args, &context).await;
    assert!(forecast_result.is_ok(), "Failed to call forecast tool: {:?}", forecast_result.err());
    
    let forecast_data: serde_json::Value = forecast_result.unwrap();
    assert!(forecast_data.is_array());
    assert_eq!(forecast_data.as_array().unwrap().len(), 3);
    
    println!("✅ Weather MCP integration test passed!");
    println!("✅ Tool discovery working");
    println!("✅ Current weather API working");
    println!("✅ Weather forecast API working");
}

#[tokio::test]
async fn test_weather_mcp_security_validation() {
    // Test with pure security context (no permissions)
    let pure_context = RuntimeContext::pure();
    let arguments = serde_json::json!({
        "city": "London",
        "units": "metric"
    });
    
    let mut capability = WeatherMCPCapability::new();
    let result = capability.call_tool("get_current_weather", &arguments, &pure_context).await;
    
    // Should fail due to insufficient permissions
    assert!(result.is_err(), "Expected security violation, but call succeeded");
    
    println!("✅ Weather MCP security validation test passed!");
    println!("✅ Unauthorized access properly blocked");
}

#[tokio::test]
async fn test_weather_mcp_capability_metadata() {
    let capability = WeatherMCPCapability::new();
    
    // Test provider ID
    assert_eq!(capability.provider_id(), "weather_mcp");
    
    // Test capability list
    let capabilities = capability.list_capabilities();
    assert_eq!(capabilities.len(), 2);
    
    // Check first capability (current weather)
    let current_weather_cap = &capabilities[0];
    assert_eq!(current_weather_cap.id, "weather_mcp.get_current_weather");
    assert!(!current_weather_cap.security_requirements.permissions.is_empty());
    
    // Check second capability (forecast)
    let forecast_cap = &capabilities[1];
    assert_eq!(forecast_cap.id, "weather_mcp.get_weather_forecast");
    assert!(!forecast_cap.security_requirements.permissions.is_empty());
    
    // Test MCP tools
    let tools = capability.get_mcp_tools();
    assert_eq!(tools.len(), 2);
    
    // Check current weather tool
    let current_tool = &tools[0];
    assert_eq!(current_tool.name(), "get_current_weather");
    assert_eq!(current_tool.description(), "Get current weather information for a specific city");
    
    // Check forecast tool
    let forecast_tool = &tools[1];
    assert_eq!(forecast_tool.name(), "get_weather_forecast");
    assert_eq!(forecast_tool.description(), "Get weather forecast for a specific city");
    
    println!("✅ Weather MCP capability metadata test passed!");
    println!("✅ Provider ID correct");
    println!("✅ Capabilities properly structured");
    println!("✅ MCP tools properly defined");
    println!("✅ Security requirements correctly specified");
}
