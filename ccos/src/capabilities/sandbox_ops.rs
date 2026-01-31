//! Sandboxed execution capabilities
//!
//! Provides RTFS-callable capabilities for sandboxed runtimes (Python, Node, etc.).

use crate::capability_marketplace::executors::{
    CapabilityExecutor, ExecutionContext, SandboxedExecutor,
};
use crate::capability_marketplace::types::{ProviderType, SandboxedCapability};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::utils::value_conversion::rtfs_value_to_json;
use futures::future::BoxFuture;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct SandboxPythonInput {
    code: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    allowed_hosts: Option<Vec<String>>,
    #[serde(default)]
    allowed_ports: Option<Vec<u16>>,
    #[serde(default)]
    secrets: Option<Vec<String>>,
    #[serde(default)]
    filesystem: Option<serde_json::Value>,
    #[serde(default)]
    resources: Option<serde_json::Value>,
}


pub async fn register_sandbox_ops_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
) -> RuntimeResult<()> {
    let handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync> =
        Arc::new(move |inputs: &Value| {
        let inputs = inputs.clone();
        Box::pin(async move {
            let payload: SandboxPythonInput = parse_payload("ccos.sandbox.python", &inputs)?;

            let mut metadata = HashMap::new();
            if let Some(hosts) = &payload.allowed_hosts {
                if !hosts.is_empty() {
                    metadata.insert("sandbox_allowed_hosts".to_string(), hosts.join(","));
                }
            }
            if let Some(ports) = &payload.allowed_ports {
                if !ports.is_empty() {
                    let csv = ports
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    metadata.insert("sandbox_allowed_ports".to_string(), csv);
                }
            }
            if let Some(secrets) = &payload.secrets {
                if !secrets.is_empty() {
                    metadata.insert("sandbox_required_secrets".to_string(), secrets.join(","));
                }
            }
            if let Some(fs) = &payload.filesystem {
                let json = serde_json::to_string(fs).map_err(|e| {
                    RuntimeError::Generic(format!("Invalid filesystem spec: {}", e))
                })?;
                metadata.insert("sandbox_filesystem".to_string(), json);
            }
            if let Some(resources) = &payload.resources {
                let json = serde_json::to_string(resources).map_err(|e| {
                    RuntimeError::Generic(format!("Invalid resources spec: {}", e))
                })?;
                metadata.insert("sandbox_resources".to_string(), json);
            }

            let provider = ProviderType::Sandboxed(SandboxedCapability {
                runtime: "python".to_string(),
                source: payload.code,
                entry_point: None,
                provider: payload.provider.or_else(|| Some("process".to_string())),
                runtime_spec: None,
                network_policy: None,
                filesystem: None,
                resources: None,
                secrets: Vec::new(),
            });

            let executor = SandboxedExecutor::new();
            let context = ExecutionContext::new("ccos.sandbox.python", &metadata, None);

            let result = executor.execute(&provider, &Value::Nil, &context).await?;
            Ok(Value::Map({
                let mut map = HashMap::new();
                map.insert(
                    rtfs::ast::MapKey::String("result".to_string()),
                    result,
                );
                map
            }))
        }) as BoxFuture<'static, RuntimeResult<Value>>
    });

    marketplace
        .register_native_capability(
            "ccos.sandbox.python".to_string(),
            "Sandbox / Python".to_string(),
            "Execute Python code in a sandboxed runtime".to_string(),
            handler,
            "default".to_string(),
        )
        .await
        .map_err(|e| RuntimeError::Generic(format!("Failed to register sandbox python: {}", e)))
}

fn parse_payload<T: serde::de::DeserializeOwned>(
    capability: &str,
    value: &Value,
) -> RuntimeResult<T> {
    let serialized = rtfs_value_to_json(value)?;
    serde_json::from_value(serialized).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: input payload does not match schema: {}",
            capability, err
        ))
    })
}

