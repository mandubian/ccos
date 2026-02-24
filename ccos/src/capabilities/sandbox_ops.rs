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
struct SandboxInput {
    code: String,
    #[serde(default)]
    runtime: Option<String>,
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
    register_sandbox_primitive(Arc::clone(&marketplace), "ccos.sandbox.python", "python").await?;
    register_sandbox_primitive(
        Arc::clone(&marketplace),
        "ccos.sandbox.javascript",
        "javascript",
    )
    .await?;
    register_sandbox_primitive(Arc::clone(&marketplace), "ccos.sandbox.shell", "shell").await?;
    Ok(())
}

async fn register_sandbox_primitive(
    marketplace: Arc<CapabilityMarketplace>,
    capability_id: &'static str,
    default_runtime: &'static str,
) -> RuntimeResult<()> {
    let handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync> =
        Arc::new(move |inputs: &Value| {
            let inputs = inputs.clone();
            Box::pin(async move {
                let payload: SandboxInput = parse_payload(capability_id, &inputs)?;

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
                    runtime: payload
                        .runtime
                        .unwrap_or_else(|| default_runtime.to_string()),
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
                let context = ExecutionContext::new(capability_id, &metadata, None);

                let result = executor.execute(&provider, &Value::Nil, &context).await?;
                Ok(Value::Map({
                    let mut map = HashMap::new();
                    map.insert(rtfs::ast::MapKey::String("result".to_string()), result);
                    map
                }))
            }) as BoxFuture<'static, RuntimeResult<Value>>
        });

    let display_name = match default_runtime {
        "python" => "Sandbox / Python",
        "javascript" => "Sandbox / JavaScript",
        "shell" => "Sandbox / Shell",
        _ => "Sandbox / Runtime",
    };

    let description = format!("Execute {} code in a sandboxed runtime", default_runtime);

    marketplace
        .register_native_capability(
            capability_id.to_string(),
            display_name.to_string(),
            description,
            handler,
            "default".to_string(),
        )
        .await
        .map_err(|e| {
            RuntimeError::Generic(format!(
                "Failed to register sandbox primitive {}: {}",
                capability_id, e
            ))
        })
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
