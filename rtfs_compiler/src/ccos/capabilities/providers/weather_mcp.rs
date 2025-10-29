//! Weather MCP Capability Implementation (moved to runtime::capabilities::providers)

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

use crate::ast::{PrimitiveType, TypeExpr};
use crate::runtime::capabilities::provider::{
    CapabilityDescriptor, CapabilityProvider, ExecutionContext, HealthStatus, NetworkAccess,
    Permission, ProviderConfig, ProviderMetadata, ResourceLimits, SecurityRequirements,
};
use crate::runtime::security::RuntimeContext;
use crate::runtime::{RuntimeError, RuntimeResult, Value as RuntimeValue};

/// Weather MCP Server implementation
/// Provides weather information tools following MCP protocol standards
#[derive(Debug, Clone)]
pub struct WeatherMCPCapability {
    /// API key for OpenWeatherMap (in real implementation)
    api_key: Option<String>,
    /// Base URL for weather API
    base_url: String,
    /// Cache for recent weather queries
    cache: HashMap<String, CachedWeatherData>,
}

/// Cached weather data to reduce API calls
#[derive(Debug, Clone)]
struct CachedWeatherData {
    data: WeatherResponse,
    timestamp: std::time::SystemTime,
}

/// MCP Tool definition for weather queries
#[derive(Debug, Serialize, Deserialize)]
pub struct MCPTool {
    name: String,
    description: String,
    input_schema: Value,
    output_schema: Option<Value>,
}

impl MCPTool {
    pub fn new(
        name: String,
        description: String,
        input_schema: Value,
        output_schema: Option<Value>,
    ) -> Self {
        Self {
            name,
            description,
            input_schema,
            output_schema,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    pub fn output_schema(&self) -> Option<&Value> {
        self.output_schema.as_ref()
    }
}

/// Weather API response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherResponse {
    pub location: String,
    pub temperature: f64,
    pub description: String,
    pub humidity: i32,
    pub wind_speed: f64,
    pub pressure: f64,
    pub timestamp: String,
}

/// Current weather tool input schema
#[derive(Debug, Serialize, Deserialize)]
pub struct CurrentWeatherInput {
    pub city: String,
    pub country: Option<String>,
    pub units: Option<String>, // metric, imperial, kelvin
}

/// Weather forecast input schema
#[derive(Debug, Serialize, Deserialize)]
pub struct WeatherForecastInput {
    pub city: String,
    pub country: Option<String>,
    pub days: Option<i32>, // 1-5 days
    pub units: Option<String>,
}

impl WeatherMCPCapability {
    /// Create a new Weather MCP capability
    pub fn new() -> Self {
        Self {
            api_key: std::env::var("OPENWEATHER_API_KEY").ok(),
            base_url: "https://api.openweathermap.org/data/2.5".to_string(),
            cache: HashMap::new(),
        }
    }

    /// Get available MCP tools for weather functionality
    pub fn get_mcp_tools(&self) -> Vec<MCPTool> {
        vec![
            MCPTool {
                name: "get_current_weather".to_string(),
                description: "Get current weather information for a specific city".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "The city name to get weather for"
                        },
                        "country": {
                            "type": "string",
                            "description": "Optional country code (e.g., 'US', 'UK')"
                        },
                        "units": {
                            "type": "string",
                            "enum": ["metric", "imperial", "kelvin"],
                            "description": "Temperature units (default: metric)"
                        }
                    },
                    "required": ["city"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"},
                        "temperature": {"type": "number"},
                        "description": {"type": "string"},
                        "humidity": {"type": "integer"},
                        "wind_speed": {"type": "number"},
                        "pressure": {"type": "number"},
                        "timestamp": {"type": "string"}
                    }
                })),
            },
            MCPTool {
                name: "get_weather_forecast".to_string(),
                description: "Get weather forecast for a specific city".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "city": {
                            "type": "string",
                            "description": "The city name to get forecast for"
                        },
                        "country": {
                            "type": "string",
                            "description": "Optional country code"
                        },
                        "days": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 5,
                            "description": "Number of forecast days (1-5)"
                        },
                        "units": {
                            "type": "string",
                            "enum": ["metric", "imperial", "kelvin"],
                            "description": "Temperature units"
                        }
                    },
                    "required": ["city"]
                }),
                output_schema: Some(json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "date": {"type": "string"},
                            "temperature_min": {"type": "number"},
                            "temperature_max": {"type": "number"},
                            "description": {"type": "string"},
                            "humidity": {"type": "integer"},
                            "wind_speed": {"type": "number"}
                        }
                    }
                })),
            },
        ]
    }

    /// Execute MCP tool call following JSON-RPC protocol
    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: &Value,
        context: &RuntimeContext,
    ) -> RuntimeResult<Value> {
        // Validate security context - check if weather queries are allowed
        if !context.allowed_capabilities.contains("weather.query") {
            return Err(RuntimeError::SecurityViolation {
                operation: "weather query".to_string(),
                capability: tool_name.to_string(),
                context: format!("{:?}", context.security_level),
            });
        }

        match tool_name {
            "get_current_weather" => self.get_current_weather(arguments).await,
            "get_weather_forecast" => self.get_weather_forecast(arguments).await,
            _ => Err(RuntimeError::UnknownCapability(format!(
                "Unknown weather tool: {}",
                tool_name
            ))),
        }
    }

    /// Get current weather for a city
    async fn get_current_weather(&mut self, arguments: &Value) -> RuntimeResult<Value> {
        // Parse input according to MCP schema
        let input: CurrentWeatherInput =
            serde_json::from_value(arguments.clone()).map_err(|e| {
                RuntimeError::Generic(format!("Invalid input for get_current_weather: {}", e))
            })?;

        let cache_key = format!(
            "current_{}_{}",
            input.city,
            input.country.as_deref().unwrap_or("")
        );

        // Check cache first (5 minute TTL)
        if let Some(cached) = self.cache.get(&cache_key) {
            if cached
                .timestamp
                .elapsed()
                .unwrap_or(Duration::from_secs(999))
                < Duration::from_secs(300)
            {
                return Ok(serde_json::to_value(&cached.data).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to serialize cached weather data: {}", e))
                })?);
            }
        }

        // Simulate API call (in real implementation, this would be actual HTTP request)
        let weather_data = self.fetch_current_weather(&input).await?;

        // Cache the result
        self.cache.insert(
            cache_key,
            CachedWeatherData {
                data: weather_data.clone(),
                timestamp: std::time::SystemTime::now(),
            },
        );

        // Return MCP-compliant response
        Ok(serde_json::to_value(&weather_data).map_err(|e| {
            RuntimeError::Generic(format!("Failed to serialize weather data: {}", e))
        })?)
    }

    /// Get weather forecast for a city
    async fn get_weather_forecast(&mut self, arguments: &Value) -> RuntimeResult<Value> {
        let input: WeatherForecastInput =
            serde_json::from_value(arguments.clone()).map_err(|e| {
                RuntimeError::Generic(format!("Invalid input for get_weather_forecast: {}", e))
            })?;

        // Simulate API call delay
        sleep(Duration::from_millis(100)).await;

        let days = input.days.unwrap_or(3).min(5).max(1);
        let mut forecast = Vec::new();

        for day in 1..=days {
            forecast.push(json!({
                "date": format!("2024-01-{:02}", day),
                "temperature_min": 15.0 + (day as f64 * 2.0),
                "temperature_max": 25.0 + (day as f64 * 1.5),
                "description": format!("Day {} forecast for {}", day, input.city),
                "humidity": 65 + (day * 5),
                "wind_speed": 12.5 + (day as f64 * 0.5)
            }));
        }

        Ok(json!(forecast))
    }

    /// Simulate fetching current weather data
    /// In a real implementation, this would make HTTP requests to OpenWeatherMap API
    async fn fetch_current_weather(
        &self,
        input: &CurrentWeatherInput,
    ) -> RuntimeResult<WeatherResponse> {
        // Simulate API call delay
        sleep(Duration::from_millis(150)).await;

        // Simulate weather data based on city
        let temp_base = match input.city.to_lowercase().as_str() {
            "london" => 12.0,
            "paris" => 16.0,
            "tokyo" => 20.0,
            "new york" => 18.0,
            "sydney" => 22.0,
            _ => 15.0,
        };

        let units = input.units.as_deref().unwrap_or("metric");
        let temperature = match units {
            "imperial" => temp_base * 9.0 / 5.0 + 32.0, // Convert to Fahrenheit
            "kelvin" => temp_base + 273.15,             // Convert to Kelvin
            _ => temp_base,                             // Celsius (metric)
        };

        Ok(WeatherResponse {
            location: format!(
                "{}{}",
                input.city,
                input
                    .country
                    .as_ref()
                    .map(|c| format!(", {}", c))
                    .unwrap_or_default()
            ),
            temperature,
            description: format!("Simulated weather for {}", input.city),
            humidity: 65,
            wind_speed: 12.5,
            pressure: 1013.25,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
    }
}

impl CapabilityProvider for WeatherMCPCapability {
    fn provider_id(&self) -> &str {
        "weather_mcp"
    }

    fn list_capabilities(&self) -> Vec<CapabilityDescriptor> {
        self.get_mcp_tools()
            .into_iter()
            .map(|tool| {
                let mut metadata = HashMap::new();
                metadata.insert("provider".to_string(), "weather_mcp".to_string());
                metadata.insert("category".to_string(), "weather".to_string());
                metadata.insert("tool_name".to_string(), tool.name.clone());
                if let Some(output_schema) = tool.output_schema {
                    metadata.insert("output_schema".to_string(), output_schema.to_string());
                }

                CapabilityDescriptor {
                    id: format!("weather_mcp.{}", tool.name),
                    description: tool.description.clone(),
                    capability_type: TypeExpr::Primitive(PrimitiveType::String), // Simplified for now
                    security_requirements: SecurityRequirements {
                        permissions: vec![Permission::NetworkAccess("weather.api".to_string())],
                        requires_microvm: false,
                        resource_limits: ResourceLimits {
                            max_memory: Some(16 * 1024 * 1024), // 16MB
                            max_cpu_time: Some(Duration::from_secs(5)),
                            max_disk_space: None,
                        },
                        network_access: NetworkAccess::Limited(vec![
                            "api.openweathermap.org".to_string()
                        ]),
                    },
                    metadata,
                }
            })
            .collect()
    }

    fn execute_capability(
        &self,
        capability_id: &str,
        inputs: &RuntimeValue,
        context: &ExecutionContext,
    ) -> RuntimeResult<RuntimeValue> {
        // Extract tool name from capability ID
        let tool_name = capability_id.strip_prefix("weather_mcp.").ok_or_else(|| {
            RuntimeError::UnknownCapability(format!(
                "Invalid weather MCP capability ID: {}",
                capability_id
            ))
        })?;

        // Convert RuntimeValue to serde_json::Value for MCP processing
        let arguments = serde_json::to_value(inputs).map_err(|e| {
            RuntimeError::Generic(format!("Failed to convert inputs to JSON: {}", e))
        })?;

        // Create a RuntimeContext from ExecutionContext (simplified)
        let runtime_context = RuntimeContext::controlled(vec!["weather.query".to_string()]);

        // Execute the tool without creating a nested Tokio runtime.
        let result = tokio::task::block_in_place(|| {
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                // Need to clone self for the async call since we can't move
                let mut capability = self.clone();
                capability
                    .call_tool(tool_name, &arguments, &runtime_context)
                    .await
            })
        })?;

        // Convert back to RuntimeValue
        serde_json::from_value(result).map_err(|e| {
            RuntimeError::Generic(format!("Failed to convert result to RuntimeValue: {}", e))
        })
    }

    fn initialize(&mut self, _config: &ProviderConfig) -> Result<(), String> {
        // Initialize any resources needed for the weather service
        Ok(())
    }

    fn health_check(&self) -> HealthStatus {
        // Simple health check - in real implementation, could ping weather API
        HealthStatus::Healthy
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: "Weather MCP Provider".to_string(),
            description: "Provides weather information via Model Context Protocol".to_string(),
            version: "1.0.0".to_string(),
            author: "RTFS Compiler".to_string(),
            license: Some("MIT".to_string()),
            dependencies: vec!["tokio".to_string(), "serde_json".to_string()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::security::RuntimeContext;
    use serde_json::json;

    #[tokio::test]
    async fn test_weather_mcp_tools() {
        let capability = WeatherMCPCapability::new();
        let tools = capability.get_mcp_tools();

        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "get_current_weather");
        assert_eq!(tools[1].name, "get_weather_forecast");
    }

    #[tokio::test]
    async fn test_current_weather_call() {
        let mut capability = WeatherMCPCapability::new();
        let context = RuntimeContext::controlled(vec!["weather.query".to_string()]);

        let arguments = json!({
            "city": "London",
            "units": "metric"
        });

        let result = capability
            .call_tool("get_current_weather", &arguments, &context)
            .await;
        assert!(result.is_ok());

        let weather_data: WeatherResponse = serde_json::from_value(result.unwrap()).unwrap();
        assert_eq!(weather_data.location, "London");
        assert!(weather_data.temperature > 0.0);
    }

    #[tokio::test]
    async fn test_weather_forecast_call() {
        let mut capability = WeatherMCPCapability::new();
        let context = RuntimeContext::controlled(vec!["weather.query".to_string()]);

        let arguments = json!({
            "city": "Paris",
            "days": 3
        });

        let result = capability
            .call_tool("get_weather_forecast", &arguments, &context)
            .await;
        assert!(result.is_ok());

        let forecast: Vec<Value> = serde_json::from_value(result.unwrap()).unwrap();
        assert_eq!(forecast.len(), 3);
    }

    #[tokio::test]
    async fn test_security_context_validation() {
        let mut capability = WeatherMCPCapability::new();
        let context = RuntimeContext::pure(); // No permissions

        let arguments = json!({"city": "Tokyo"});

        let result = capability
            .call_tool("get_current_weather", &arguments, &context)
            .await;
        assert!(result.is_err());

        if let Err(RuntimeError::SecurityViolation { .. }) = result {
            // Expected
        } else {
            panic!("Expected SecurityViolation error");
        }
    }

    #[test]
    fn test_capability_provider_interface() {
        let capability = WeatherMCPCapability::new();

        assert_eq!(capability.provider_id(), "weather_mcp");

        let capabilities = capability.list_capabilities();
        assert_eq!(capabilities.len(), 2);

        // Check first capability
        let weather_cap = &capabilities[0];
        assert_eq!(weather_cap.id, "weather_mcp.get_current_weather");
        assert!(weather_cap.security_requirements.permissions.len() > 0);
    }
}
