use super::types::*;
use super::executors::{ExecutorVariant, HttpExecutor, LocalExecutor, MCPExecutor, A2AExecutor};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::values::Value;
use crate::ast::{MapKey, TypeExpr};
use crate::runtime::type_validator::{TypeValidator, TypeCheckingConfig, VerificationContext};
use crate::runtime::streaming::{StreamType, StreamingProvider};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::Utc;

impl CapabilityMarketplace {
    pub fn new(capability_registry: Arc<RwLock<crate::runtime::capability_registry::CapabilityRegistry>>) -> Self {
        let mut marketplace = Self {
            capabilities: Arc::new(RwLock::new(HashMap::new())),
            discovery_agents: Vec::new(),
            capability_registry,
            network_registry: None,
            type_validator: Arc::new(TypeValidator::new()),
            executor_registry: HashMap::new(),
        };
        marketplace.executor_registry.insert(TypeId::of::<MCPCapability>(), ExecutorVariant::MCP(MCPExecutor));
        marketplace.executor_registry.insert(TypeId::of::<A2ACapability>(), ExecutorVariant::A2A(A2AExecutor));
        marketplace.executor_registry.insert(TypeId::of::<LocalCapability>(), ExecutorVariant::Local(LocalExecutor));
        marketplace.executor_registry.insert(TypeId::of::<HttpCapability>(), ExecutorVariant::Http(HttpExecutor));
        marketplace
    }

    fn compute_content_hash(&self, content: &str) -> String { super::discovery::compute_content_hash(content) }

    pub async fn register_streaming_capability(
        &self,
        id: String,
        name: String,
        description: String,
        stream_type: StreamType,
        provider: StreamingProvider,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: "streaming".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["streaming_registration".to_string()],
            registered_at: Utc::now(),
        };
        let stream_impl = StreamCapabilityImpl {
            provider,
            stream_type,
            input_schema: None,
            output_schema: None,
            supports_progress: true,
            supports_cancellation: true,
            bidirectional_config: None,
            duplex_config: None,
            stream_config: None,
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Stream(stream_impl),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_local_capability(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: "local".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["local_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Local(LocalCapability { handler }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_local_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: "local".to_string(),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}", id, name, description)),
            custody_chain: vec!["local_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Local(LocalCapability { handler }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await;
        caps.insert(id, capability);
        Ok(())
    }

    pub async fn register_http_capability(
        &self,
        id: String,
        name: String,
        description: String,
        base_url: String,
        auth_token: Option<String>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("http:{}", base_url),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}", id, name, description, base_url)),
            custody_chain: vec!["http_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Http(HttpCapability { base_url, auth_token, timeout_ms: 5000 }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_http_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        base_url: String,
        auth_token: Option<String>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("http:{}", base_url),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}", id, name, description, base_url)),
            custody_chain: vec!["http_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Http(HttpCapability { base_url, auth_token, timeout_ms: 5000 }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_mcp_capability(
        &self,
        id: String,
        name: String,
        description: String,
        server_url: String,
        tool_name: String,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("mcp:{}/{}", server_url, tool_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}{}", id, name, description, server_url, tool_name, timeout_ms)),
            custody_chain: vec!["mcp_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::MCP(MCPCapability { server_url, tool_name, timeout_ms }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_mcp_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        server_url: String,
        tool_name: String,
        timeout_ms: u64,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("mcp:{}/{}", server_url, tool_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}{}", id, name, description, server_url, tool_name, timeout_ms)),
            custody_chain: vec!["mcp_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::MCP(MCPCapability { server_url, tool_name, timeout_ms }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_a2a_capability(
        &self,
        id: String,
        name: String,
        description: String,
        agent_id: String,
        endpoint: String,
        protocol: String,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("a2a:{}@{}", agent_id, endpoint),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}{}{}", id, name, description, agent_id, endpoint, protocol, timeout_ms)),
            custody_chain: vec!["a2a_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::A2A(A2ACapability { agent_id, endpoint, protocol, timeout_ms }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_a2a_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        agent_id: String,
        endpoint: String,
        protocol: String,
        timeout_ms: u64,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("a2a:{}@{}", agent_id, endpoint),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}{}{}", id, name, description, agent_id, endpoint, protocol, timeout_ms)),
            custody_chain: vec!["a2a_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::A2A(A2ACapability { agent_id, endpoint, protocol, timeout_ms }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_plugin_capability(
        &self,
        id: String,
        name: String,
        description: String,
        plugin_path: String,
        function_name: String,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("plugin:{}#{}", plugin_path, function_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}", id, name, description, plugin_path, function_name)),
            custody_chain: vec!["plugin_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Plugin(PluginCapability { plugin_path, function_name }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_plugin_capability_with_schema(
        &self,
        id: String,
        name: String,
        description: String,
        plugin_path: String,
        function_name: String,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("plugin:{}#{}", plugin_path, function_name),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}", id, name, description, plugin_path, function_name)),
            custody_chain: vec!["plugin_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::Plugin(PluginCapability { plugin_path, function_name }),
            version: "1.0.0".to_string(),
            input_schema,
            output_schema,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn register_remote_rtfs_capability(
        &self,
        id: String,
        name: String,
        description: String,
        endpoint: String,
        auth_token: Option<String>,
        timeout_ms: u64,
    ) -> Result<(), RuntimeError> {
        let provenance = CapabilityProvenance {
            source: format!("remote-rtfs:{}", endpoint),
            version: Some("1.0.0".to_string()),
            content_hash: self.compute_content_hash(&format!("{}{}{}{}{}", id, name, description, endpoint, timeout_ms)),
            custody_chain: vec!["remote_rtfs_registration".to_string()],
            registered_at: chrono::Utc::now(),
        };
        let capability = CapabilityManifest {
            id: id.clone(),
            name,
            description,
            provider: ProviderType::RemoteRTFS(RemoteRTFSCapability { endpoint, timeout_ms, auth_token }),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: Some(provenance),
            permissions: vec![],
            metadata: HashMap::new(),
        };
        let mut caps = self.capabilities.write().await; caps.insert(id, capability); Ok(())
    }

    pub async fn start_stream_with_config(&self, capability_id: &str, params: &Value, config: &crate::runtime::streaming::StreamConfig) -> RuntimeResult<crate::runtime::streaming::StreamHandle> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;
        if let ProviderType::Stream(stream_impl) = &capability.provider {
            if config.callbacks.is_some() { stream_impl.provider.start_stream_with_config(params, config).await }
            else { let handle = stream_impl.provider.start_stream(params)?; Ok(handle) }
        } else { Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id))) }
    }

    pub async fn start_bidirectional_stream_with_config(&self, capability_id: &str, params: &Value, config: &crate::runtime::streaming::StreamConfig) -> RuntimeResult<crate::runtime::streaming::StreamHandle> {
        let capability = self.get_capability(capability_id).await
            .ok_or_else(|| RuntimeError::Generic(format!("Capability '{}' not found", capability_id)))?;
        if let ProviderType::Stream(stream_impl) = &capability.provider {
            if !matches!(stream_impl.stream_type, StreamType::Bidirectional) { return Err(RuntimeError::Generic(format!("Capability '{}' is not bidirectional", capability_id))); }
            if config.callbacks.is_some() { stream_impl.provider.start_bidirectional_stream_with_config(params, config).await }
            else { let handle = stream_impl.provider.start_bidirectional_stream(params)?; Ok(handle) }
        } else { Err(RuntimeError::Generic(format!("Capability '{}' is not a stream capability", capability_id))) }
    }

    pub async fn get_capability(&self, id: &str) -> Option<CapabilityManifest> {
        let capabilities = self.capabilities.read().await; capabilities.get(id).cloned()
    }

    pub async fn list_capabilities(&self) -> Vec<CapabilityManifest> {
        let capabilities = self.capabilities.read().await; capabilities.values().cloned().collect()
    }

    pub async fn execute_capability(&self, id: &str, inputs: &Value) -> RuntimeResult<Value> {
        // Fetch manifest or fall back to registry execution
        let manifest_opt = { self.capabilities.read().await.get(id).cloned() };
        let manifest = if let Some(m) = manifest_opt {
            m
        } else {
            let registry = self.capability_registry.read().await;
            let args = vec![inputs.clone()];
            return registry.execute_capability_with_microvm(id, args);
        };

        // Prepare boundary verification context
        let boundary_context = VerificationContext::capability_boundary(id);
        let type_config = TypeCheckingConfig::default();

        // Validate inputs if a schema is provided
        if let Some(input_schema) = &manifest.input_schema {
            self.type_validator
                .validate_with_config(inputs, input_schema, &type_config, &boundary_context)
                .map_err(|e| RuntimeError::Generic(format!("Type validation failed: {}", e)))?;
        }

        // Execute via executor registry or provider fallback
        let exec_result = if let Some(executor) = self
            .executor_registry
            .get(&match &manifest.provider {
                ProviderType::Local(_) => std::any::TypeId::of::<LocalCapability>(),
                ProviderType::Http(_) => std::any::TypeId::of::<HttpCapability>(),
                ProviderType::MCP(_) => std::any::TypeId::of::<MCPCapability>(),
                ProviderType::A2A(_) => std::any::TypeId::of::<A2ACapability>(),
                ProviderType::Plugin(_) => std::any::TypeId::of::<PluginCapability>(),
                ProviderType::RemoteRTFS(_) => std::any::TypeId::of::<RemoteRTFSCapability>(),
                ProviderType::Stream(_) => std::any::TypeId::of::<StreamCapabilityImpl>(),
            })
        {
            executor.execute(&manifest.provider, inputs).await
        } else {
            match &manifest.provider {
                ProviderType::Local(local) => (local.handler)(inputs),
                ProviderType::Http(http) => self.execute_http_capability(http, inputs).await,
                ProviderType::MCP(_mcp) => Err(RuntimeError::Generic("MCP not configured".to_string())),
                ProviderType::A2A(_a2a) => Err(RuntimeError::Generic("A2A not configured".to_string())),
                ProviderType::Plugin(_p) => Err(RuntimeError::Generic("Plugin not configured".to_string())),
                ProviderType::RemoteRTFS(_r) => Err(RuntimeError::Generic("Remote RTFS not configured".to_string())),
                ProviderType::Stream(stream_impl) => self.execute_stream_capability(stream_impl, inputs).await,
            }
        }?;

        // Validate outputs if a schema is provided
        if let Some(output_schema) = &manifest.output_schema {
            self.type_validator
                .validate_with_config(&exec_result, output_schema, &type_config, &boundary_context)
                .map_err(|e| RuntimeError::Generic(format!("Type validation failed: {}", e)))?;
        }

        Ok(exec_result)
    }

    async fn execute_stream_capability(&self, stream_impl: &StreamCapabilityImpl, inputs: &Value) -> RuntimeResult<Value> {
        let handle = stream_impl.provider.start_stream(inputs)?; Ok(Value::String(format!("Stream started with ID: {}", handle.stream_id)))
    }

    async fn execute_http_capability(&self, http: &HttpCapability, inputs: &Value) -> RuntimeResult<Value> {
        let args = match inputs { Value::List(list) => list.clone(), Value::Vector(vec) => vec.clone(), v => vec![v.clone()] };
        let url = args.get(0).and_then(|v| v.as_string()).unwrap_or(&http.base_url);
        let method = args.get(1).and_then(|v| v.as_string()).unwrap_or("GET");
        let default_headers = std::collections::HashMap::new();
        let headers = args.get(2).and_then(|v| match v { Value::Map(m) => Some(m), _ => None }).unwrap_or(&default_headers);
        let body = args.get(3).and_then(|v| v.as_string()).unwrap_or("").to_string();
        let client = reqwest::Client::new();
        let method_enum = reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);
        let mut req = client.request(method_enum, url);
        if let Some(token) = &http.auth_token { req = req.bearer_auth(token); }
        for (k,v) in headers.iter() { if let MapKey::String(ref key) = k { if let Value::String(ref val) = v { req = req.header(key, val); } } }
        if !body.is_empty() { req = req.body(body); }
        let response = req.timeout(std::time::Duration::from_millis(http.timeout_ms)).send().await
            .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;
        let status = response.status().as_u16() as i64; let response_headers = response.headers().clone(); let resp_body = response.text().await.unwrap_or_default();
        let mut response_map = std::collections::HashMap::new();
        response_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
        response_map.insert(MapKey::String("body".to_string()), Value::String(resp_body));
        let mut headers_map = std::collections::HashMap::new();
        for (key, value) in response_headers.iter() { headers_map.insert(MapKey::String(key.to_string()), Value::String(value.to_str().unwrap_or("").to_string())); }
        response_map.insert(MapKey::String("headers".to_string()), Value::Map(headers_map));
        Ok(Value::Map(response_map))
    }

    pub async fn execute_with_validation(&self, capability_id: &str, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        let config = TypeCheckingConfig::default(); self.execute_with_validation_config(capability_id, params, &config).await
    }

    pub async fn execute_with_validation_config(&self, capability_id: &str, params: &HashMap<String, Value>, config: &TypeCheckingConfig) -> Result<Value, RuntimeError> {
        let capability = { let capabilities = self.capabilities.read().await; capabilities.get(capability_id).cloned().ok_or_else(|| RuntimeError::Generic(format!("Capability not found: {}", capability_id)))? };
        let boundary_context = VerificationContext::capability_boundary(capability_id);
        if let Some(input_schema) = &capability.input_schema { self.validate_input_schema_optimized(params, input_schema, config, &boundary_context).await?; }
        let inputs_value = self.params_to_value(params)?;
        let result = self.execute_capability(capability_id, &inputs_value).await?;
        if let Some(output_schema) = &capability.output_schema { self.validate_output_schema_optimized(&result, output_schema, config, &boundary_context).await?; }
        Ok(result)
    }

    async fn validate_input_schema_optimized(&self, params: &HashMap<String, Value>, schema_expr: &TypeExpr, config: &TypeCheckingConfig, context: &VerificationContext) -> Result<(), RuntimeError> {
        let params_value = self.params_to_value(params)?; self.type_validator.validate_with_config(&params_value, schema_expr, config, context).map_err(|e| RuntimeError::Generic(format!("Input validation failed: {}", e)))?; Ok(())
    }

    async fn validate_output_schema_optimized(&self, result: &Value, schema_expr: &TypeExpr, config: &TypeCheckingConfig, context: &VerificationContext) -> Result<(), RuntimeError> {
        self.type_validator.validate_with_config(result, schema_expr, config, context).map_err(|e| RuntimeError::Generic(format!("Output validation failed: {}", e)))?; Ok(())
    }

    fn params_to_value(&self, params: &HashMap<String, Value>) -> Result<Value, RuntimeError> {
        let mut map = HashMap::new();
        for (key, value) in params { let map_key = if key.starts_with(':') { MapKey::Keyword(crate::ast::Keyword(key[1..].to_string())) } else { MapKey::String(key.clone()) }; map.insert(map_key, value.clone()); }
        Ok(Value::Map(map))
    }

    pub fn json_to_rtfs_value(json: &serde_json::Value) -> RuntimeResult<Value> {
        match json {
            serde_json::Value::String(s) => Ok(Value::String(s.clone())),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() { Ok(Value::Integer(i)) }
                else if let Some(f) = n.as_f64() { Ok(Value::Float(f)) }
                else { Err(RuntimeError::Generic("Invalid number format".to_string())) }
            }
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
            serde_json::Value::Array(arr) => {
                let values: Result<Vec<Value>, RuntimeError> = arr.iter().map(Self::json_to_rtfs_value).collect();
                Ok(Value::Vector(values?))
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new(); for (key, value) in obj { let rtfs_key = MapKey::String(key.clone()); let rtfs_value = Self::json_to_rtfs_value(value)?; map.insert(rtfs_key, rtfs_value); } Ok(Value::Map(map))
            }
            serde_json::Value::Null => Ok(Value::Nil),
        }
    }
}
