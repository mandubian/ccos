// RTFS Agent Communication Client - Handles agent-to-agent communication
// Implements communication with agents discovered through the registry

use std::collections::HashMap;
use std::time::Duration;
use reqwest::Client;
use serde_json::{json, Value as JsonValue};

use crate::runtime::{RuntimeResult, RuntimeError, Value};
use super::types::*;

/// Client for communicating with agents
pub struct AgentCommunicationClient {
    http_client: Client,
    default_timeout: Duration,
}

/// Request to invoke an agent capability
#[derive(Debug, Clone)]
pub struct AgentInvocationRequest {
    pub agent_id: String,
    pub capability_id: String,
    pub input_data: Value,
    pub timeout_ms: Option<u64>,
    pub options: Option<HashMap<String, JsonValue>>,
}

/// Response from agent invocation
#[derive(Debug, Clone)]
pub struct AgentInvocationResponse {
    pub success: bool,
    pub output_data: Option<Value>,
    pub error_message: Option<String>,
    pub execution_time_ms: Option<u64>,
    pub metadata: Option<HashMap<String, JsonValue>>,
}

impl AgentCommunicationClient {
    /// Create a new communication client
    pub fn new() -> Self {
        Self {
            http_client: Client::new(),
            default_timeout: Duration::from_secs(30),
        }
    }
    
    /// Set the default timeout for agent communications
    pub fn set_default_timeout(&mut self, timeout: Duration) {
        self.default_timeout = timeout;
    }
    
    /// Invoke an agent capability
    pub async fn invoke_agent(
        &self,
        agent_card: &AgentCard,
        request: AgentInvocationRequest
    ) -> RuntimeResult<AgentInvocationResponse> {
        // Find appropriate endpoint for HTTP communication
        let endpoint = self.find_http_endpoint(agent_card)?;
        
        let timeout = request.timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(self.default_timeout);
            
        // Prepare the request payload
        let payload = self.prepare_invocation_payload(&request)?;
        
        // Send the request
        let response = self.http_client
            .post(&endpoint.uri)
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| RuntimeError::AgentCommunicationError {
                message: format!("Failed to connect to agent: {}", e),
                agent_id: request.agent_id.clone(),
                endpoint: endpoint.uri.clone(),
            })?;            
        // Check response status
        if !response.status().is_success() {
            return Err(RuntimeError::AgentCommunicationError {
                message: format!("Agent returned error status: {}", response.status()),
                agent_id: request.agent_id,
                endpoint: endpoint.uri.clone(),
            });
        }
        
        // Parse response
        let response_body: JsonValue = response.json().await
            .map_err(|e| RuntimeError::AgentCommunicationError {
                message: format!("Failed to parse agent response: {}", e),
                agent_id: request.agent_id.clone(),
                endpoint: endpoint.uri.clone(),
            })?;
            
        self.parse_invocation_response(response_body, &request.agent_id, &endpoint.uri)
    }
    
    /// Invoke an agent capability with simple parameters
    pub async fn invoke_simple(
        &self,
        agent_card: &AgentCard,
        capability_id: String,
        input_data: Value
    ) -> RuntimeResult<Value> {
        let request = AgentInvocationRequest {
            agent_id: agent_card.agent_id.clone(),
            capability_id,
            input_data,
            timeout_ms: None,
            options: None,
        };
        
        let response = self.invoke_agent(agent_card, request).await?;
        
        if response.success {
            response.output_data.ok_or_else(|| RuntimeError::AgentCommunicationError {
                message: "Agent returned success but no output data".to_string(),
                agent_id: agent_card.agent_id.clone(),
                endpoint: "unknown".to_string(),
            })
        } else {
            Err(RuntimeError::AgentCommunicationError {
                message: response.error_message.unwrap_or("Unknown agent error".to_string()),
                agent_id: agent_card.agent_id.clone(),
                endpoint: "unknown".to_string(),
            })
        }
    }
    
    /// Health check for an agent
    pub async fn health_check(&self, agent_card: &AgentCard) -> RuntimeResult<bool> {
        let endpoint = self.find_http_endpoint(agent_card)?;
        
        // Try to connect with a short timeout
        let health_url = if endpoint.uri.ends_with('/') {
            format!("{}health", endpoint.uri)
        } else {
            format!("{}/health", endpoint.uri)
        };
        
        let response = self.http_client
            .get(&health_url)
            .timeout(Duration::from_secs(5))
            .send()
            .await;
            
        match response {
            Ok(resp) if resp.status().is_success() => Ok(true),
            _ => Ok(false),
        }
    }
    
    /// Get agent capabilities info
    pub async fn get_capabilities(&self, agent_card: &AgentCard) -> RuntimeResult<Vec<AgentCapability>> {
        let endpoint = self.find_http_endpoint(agent_card)?;
        
        let capabilities_url = if endpoint.uri.ends_with('/') {
            format!("{}capabilities", endpoint.uri)
        } else {
            format!("{}/capabilities", endpoint.uri)
        };
        
        let response = self.http_client
            .get(&capabilities_url)
            .timeout(self.default_timeout)
            .send()
            .await
            .map_err(|e| RuntimeError::AgentCommunicationError {
                message: format!("Failed to get capabilities: {}", e),
                agent_id: agent_card.agent_id.clone(),
                endpoint: endpoint.uri.clone(),
            })?;
            
        if !response.status().is_success() {            return Err(RuntimeError::AgentCommunicationError {
                message: format!("Agent returned error status: {}", response.status()),
                agent_id: agent_card.agent_id.clone(),
                endpoint: endpoint.uri.clone(),
            });
        }
        
        let capabilities: Vec<AgentCapability> = response.json().await
            .map_err(|e| RuntimeError::AgentCommunicationError {
                message: format!("Failed to parse capabilities response: {}", e),
                agent_id: agent_card.agent_id.clone(),
                endpoint: endpoint.uri.clone(),
            })?;
            
        Ok(capabilities)
    }
    
    // Helper methods
    
    fn find_http_endpoint<'a>(&self, agent_card: &'a AgentCard) -> RuntimeResult<&'a AgentEndpoint> {
        agent_card.communication.endpoints
            .iter()
            .find(|endpoint| endpoint.protocol == "http" || endpoint.protocol == "https")
            .ok_or_else(|| RuntimeError::AgentCommunicationError {
                message: "No HTTP endpoint found for agent".to_string(),
                agent_id: agent_card.agent_id.clone(),
                endpoint: "none".to_string(),
            })
    }
    
    fn prepare_invocation_payload(&self, request: &AgentInvocationRequest) -> RuntimeResult<JsonValue> {
        // Convert RTFS Value to JSON for transmission
        let input_json = self.value_to_json(&request.input_data)?;
        
        let mut payload = json!({
            "capability_id": request.capability_id,
            "input": input_json,
            "metadata": {
                "client": "rtfs_compiler",
                "version": "0.1.0"
            }
        });
        
        if let Some(options) = &request.options {
            if let JsonValue::Object(ref mut map) = payload {
                map.insert("options".to_string(), JsonValue::Object(
                    options.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                ));
            }
        }
        
        Ok(payload)
    }
    
    fn parse_invocation_response(
        &self, 
        response: JsonValue, 
        agent_id: &str, 
        endpoint: &str
    ) -> RuntimeResult<AgentInvocationResponse> {
        let success = response.get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
            
        let output_data = if success {
            response.get("output")
                .map(|v| self.json_to_value(v))
                .transpose()?
        } else {
            None
        };
        
        let error_message = if !success {
            response.get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            None
        };
        
        let execution_time_ms = response.get("execution_time_ms")
            .and_then(|v| v.as_u64());
              let metadata = response.get("metadata")
            .and_then(|v| v.as_object())
            .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<HashMap<String, JsonValue>>());
            
        Ok(AgentInvocationResponse {
            success,
            output_data,
            error_message,
            execution_time_ms,
            metadata,
        })
    }
    
    fn value_to_json(&self, value: &Value) -> RuntimeResult<JsonValue> {
        match value {
            Value::Nil => Ok(JsonValue::Null),
            Value::Boolean(b) => Ok(JsonValue::Bool(*b)),
            Value::Integer(i) => Ok(JsonValue::Number((*i).into())),
            Value::Float(f) => {
                serde_json::Number::from_f64(*f)
                    .map(JsonValue::Number)
                    .ok_or_else(|| RuntimeError::JsonError("Invalid float value".to_string()))
            },
            Value::String(s) => Ok(JsonValue::String(s.clone())),
            Value::Keyword(k) => Ok(JsonValue::String(format!(":{}", k.0))),
            Value::Symbol(s) => Ok(JsonValue::String(s.0.clone())),
            Value::Vector(v) => {
                let json_vec: Result<Vec<JsonValue>, RuntimeError> = v.iter()
                    .map(|item| self.value_to_json(item))
                    .collect();
                Ok(JsonValue::Array(json_vec?))
            },
            Value::Map(m) => {
                let mut json_obj = serde_json::Map::new();
                for (key, value) in m {
                    let key_str = match key {
                        crate::ast::MapKey::Keyword(k) => format!(":{}", k.0),
                        crate::ast::MapKey::String(s) => s.clone(),
                        crate::ast::MapKey::Integer(i) => i.to_string(),
                    };
                    json_obj.insert(key_str, self.value_to_json(value)?);
                }
                Ok(JsonValue::Object(json_obj))
            },
            _ => Err(RuntimeError::JsonError(
                format!("Cannot convert {:?} to JSON for agent communication", value)
            )),
        }
    }
    
    fn json_to_value(&self, json: &JsonValue) -> RuntimeResult<Value> {
        match json {
            JsonValue::Null => Ok(Value::Nil),
            JsonValue::Bool(b) => Ok(Value::Boolean(*b)),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Integer(i))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Float(f))
                } else {
                    Err(RuntimeError::JsonError("Invalid number format".to_string()))
                }
            },
            JsonValue::String(s) => {
                if s.starts_with(':') {
                    Ok(Value::Keyword(crate::ast::Keyword(s[1..].to_string())))
                } else {
                    Ok(Value::String(s.clone()))
                }
            },
            JsonValue::Array(arr) => {
                let vec_result: Result<Vec<Value>, RuntimeError> = arr.iter()
                    .map(|item| self.json_to_value(item))
                    .collect();
                Ok(Value::Vector(vec_result?))
            },
            JsonValue::Object(obj) => {
                let mut map = std::collections::HashMap::new();
                for (key, value) in obj {
                    let map_key = if key.starts_with(':') {
                        crate::ast::MapKey::Keyword(crate::ast::Keyword(key[1..].to_string()))
                    } else if let Ok(i) = key.parse::<i64>() {
                        crate::ast::MapKey::Integer(i)
                    } else {
                        crate::ast::MapKey::String(key.clone())
                    };
                    map.insert(map_key, self.json_to_value(value)?);
                }
                Ok(Value::Map(map))
            },
        }
    }
}

impl Default for AgentCommunicationClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Keyword;
    
    #[test]
    fn test_value_to_json_conversion() {
        let client = AgentCommunicationClient::new();
        
        // Test basic types
        assert_eq!(client.value_to_json(&Value::Nil).unwrap(), JsonValue::Null);
        assert_eq!(client.value_to_json(&Value::Boolean(true)).unwrap(), JsonValue::Bool(true));
        assert_eq!(client.value_to_json(&Value::Integer(42)).unwrap(), JsonValue::Number(42.into()));
        assert_eq!(client.value_to_json(&Value::String("test".to_string())).unwrap(), JsonValue::String("test".to_string()));
        
        // Test keyword conversion
        let keyword_value = Value::Keyword(Keyword("test".to_string()));
        assert_eq!(client.value_to_json(&keyword_value).unwrap(), JsonValue::String(":test".to_string()));
    }
    
    #[test]
    fn test_json_to_value_conversion() {
        let client = AgentCommunicationClient::new();
        
        // Test basic types
        assert_eq!(client.json_to_value(&JsonValue::Null).unwrap(), Value::Nil);
        assert_eq!(client.json_to_value(&JsonValue::Bool(true)).unwrap(), Value::Boolean(true));
        assert_eq!(client.json_to_value(&JsonValue::Number(42.into())).unwrap(), Value::Integer(42));
        assert_eq!(client.json_to_value(&JsonValue::String("test".to_string())).unwrap(), Value::String("test".to_string()));
        
        // Test keyword conversion
        let keyword_json = JsonValue::String(":test".to_string());
        assert_eq!(client.json_to_value(&keyword_json).unwrap(), Value::Keyword(Keyword("test".to_string())));
    }
    
    #[test]
    fn test_find_http_endpoint() {
        let client = AgentCommunicationClient::new();
        
        let mut agent_card = AgentCard::new(
            "test-agent".to_string(),
            "Test Agent".to_string(),
            "1.0.0".to_string(),
            "Test agent".to_string()
        );
        
        agent_card.add_endpoint(AgentEndpoint::new(
            "http".to_string(),
            "http://localhost:8080".to_string()
        ));
        
        let endpoint = client.find_http_endpoint(&agent_card).unwrap();
        assert_eq!(endpoint.protocol, "http");
        assert_eq!(endpoint.uri, "http://localhost:8080");
    }
}
