use crate::capability_marketplace::types::{
    CapabilityManifest, EffectType, NativeCapability, ProviderType,
};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::mcp::discovery_session::{MCPServerInfo, MCPSessionManager};
use futures::future::FutureExt;
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::utils::value_conversion::rtfs_value_to_json;

/// Register MCP bridge capabilities (ecosystem capabilities)
pub async fn register_mcp_capabilities(marketplace: &CapabilityMarketplace) -> RuntimeResult<()> {
    let http_client = Arc::new(reqwest::Client::new());
    let session_manager = Arc::new(MCPSessionManager::with_client(http_client, None));

    // ccos.capabilities.mcp.call
    {
        let session_manager = session_manager.clone();
        let handler = Arc::new(
            move |inputs: &Value| -> futures::future::BoxFuture<'static, RuntimeResult<Value>> {
                let session_manager = session_manager.clone();
                let inputs = inputs.clone();
                async move {
                    // Extract arguments
                    let server_url = get_string_param(&inputs, "server-url")?;
                    let tool_name = get_string_param(&inputs, "tool-name")?;

                    // input can be a map or value.
                    // The generated code passes :input input
                    // If input is a Value, we use it.
                    let tool_input = match inputs {
                        Value::Map(m) => {
                            // We need to extract the "input" field from the inputs map
                            if let Some(val) = m.get(&rtfs::ast::MapKey::Keyword(
                                rtfs::ast::Keyword("input".to_string()),
                            )) {
                                val.clone()
                            } else if let Some(val) =
                                m.get(&rtfs::ast::MapKey::String("input".to_string()))
                            {
                                val.clone()
                            } else {
                                // If no "input" field, maybe the inputs ARE the tool inputs?
                                // But the signature is (fn [input] (call ... :input input))
                                // So it should be nested.
                                // However, if the caller passed inputs merged?
                                // Let's assume strict signature for now.
                                return Err(RuntimeError::Generic(
                                    "Missing 'input' parameter".to_string(),
                                ));
                            }
                        }
                        _ => return Err(RuntimeError::Generic("Expected map inputs".to_string())),
                    };

                    // Initialize session (or get existing from pool inside manager)
                    let client_info = MCPServerInfo {
                        name: "ccos-mcp-bridge".to_string(),
                        version: "1.0.0".to_string(),
                    };

                    // TODO: Auth headers?

                    let session = session_manager
                        .initialize_session(&server_url, &client_info)
                        .await?;

                    // Convert tool_input (RTFS Value) to JSON
                    // RTFS Value -> JSON conversion
                    let json_args = match &tool_input {
                        Value::Map(_) => {
                            // Use helper to convert RTFS value to JSON
                            rtfs_value_to_json(&tool_input)?
                        }
                        _ => rtfs_value_to_json(&tool_input)?, // Handle other inputs as well
                    };

                    // Call tool
                    let result = session_manager
                        .call_tool(&session, &tool_name, json_args)
                        .await?;

                    // Convert JSON result back to RTFS Value
                    // Result is McpToolCallResult content
                    // We return output as string usually
                    // Or map?
                    // The CCOS capability usually returns a string or map.
                    // Let's verify what `call_tool` returns. It returns `McpToolCallResult`.

                    // Flatten content to string for now (simple implementation)
                    let mut output = String::new();
                    for content in result.content {
                        if let Some(text) = content.text {
                            output.push_str(&text);
                        }
                    }

                    Ok(Value::String(output))
                }
                .boxed()
            },
        );

        let native_cap = NativeCapability {
            handler,
            security_level: "high".to_string(), // Network access
            metadata: HashMap::new(),
        };

        let manifest = CapabilityManifest {
            id: "ccos.capabilities.mcp.call".to_string(),
            name: "MCP Tool Call Bridge".to_string(),
            description: "Bridge to call MCP tools from RTFS".to_string(),
            provider: ProviderType::Native(native_cap),
            version: "1.0.0".to_string(),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec!["network".to_string()],
            metadata: HashMap::new(),
            agent_metadata: None,
            domains: vec!["mcp".to_string()],
            categories: vec!["bridge".to_string()],
            effect_type: EffectType::Effectful,
            approval_status: crate::capability_marketplace::types::ApprovalStatus::Approved,
        };

        marketplace.register_capability_manifest(manifest).await?;
    }

    Ok(())
}

fn get_string_param(inputs: &Value, name: &str) -> RuntimeResult<String> {
    match inputs {
        Value::Map(m) => {
            // Check for string key
            if let Some(val) = m.get(&MapKey::String(name.to_string())) {
                match val {
                    Value::String(s) => Ok(s.clone()),
                    _ => Err(RuntimeError::Generic(format!(
                        "Parameter '{}' must be a string",
                        name
                    ))),
                }
            } else if let Some(val) = m.get(&MapKey::Keyword(Keyword(name.to_string()))) {
                // Check for keyword key
                match val {
                    Value::String(s) => Ok(s.clone()),
                    _ => Err(RuntimeError::Generic(format!(
                        "Parameter '{}' must be a string",
                        name
                    ))),
                }
            } else {
                Err(RuntimeError::Generic(format!(
                    "Missing parameter '{}'",
                    name
                )))
            }
        }
        _ => Err(RuntimeError::Generic("Expected map input".to_string())),
    }
}
