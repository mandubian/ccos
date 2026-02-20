//! Network capabilities for the CCOS marketplace
//!
//! Provides HTTP capabilities that can be discovered and executed
//! through the CapabilityMarketplace.

use crate::approval::{storage_file::FileApprovalStorage, UnifiedApprovalQueue};
use crate::capability_marketplace::CapabilityMarketplace;
use futures::future::BoxFuture;
use rtfs::ast::{Keyword, MapKey, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

fn get_map_string(map: &HashMap<MapKey, Value>, key: &str) -> Option<String> {
    map.get(&MapKey::Keyword(Keyword(key.to_string())))
        .or_else(|| map.get(&MapKey::String(key.to_string())))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            _ => Some(v.to_string()),
        })
}

fn get_map_value<'a>(map: &'a HashMap<MapKey, Value>, key: &str) -> Option<&'a Value> {
    map.get(&MapKey::Keyword(Keyword(key.to_string())))
        .or_else(|| map.get(&MapKey::String(key.to_string())))
}

/// Register network capabilities in the marketplace
pub async fn register_network_capabilities(
    marketplace: &CapabilityMarketplace,
    approval_queue: Option<UnifiedApprovalQueue<FileApprovalStorage>>,
    internal_secret: Option<String>,
) -> RuntimeResult<()> {
    // Input schema for http-fetch: {:url String :method? String :headers? Map :body? String :timeout_ms? Int}
    let input_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword("url".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("method".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("headers".to_string()),
                value_type: Box::new(TypeExpr::Map {
                    entries: vec![],
                    wildcard: Some(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
                }),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("body".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("timeout_ms".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: true,
            },
        ],
        wildcard: None,
    };

    // Output schema: {:status Int :body String :headers Map :usage? {:network_egress_bytes Int :network_ingress_bytes Int}}
    let output_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword("status".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("body".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("headers".to_string()),
                value_type: Box::new(TypeExpr::Map {
                    entries: vec![],
                    wildcard: Some(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
                }),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("usage".to_string()),
                value_type: Box::new(TypeExpr::Map {
                    entries: vec![
                        MapTypeEntry {
                            key: Keyword("network_egress_bytes".to_string()),
                            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                            optional: false,
                        },
                        MapTypeEntry {
                            key: Keyword("network_ingress_bytes".to_string()),
                            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                            optional: false,
                        },
                    ],
                    wildcard: None,
                }),
                optional: true,
            },
        ],
        wildcard: None,
    };

    let capability_registry = Arc::clone(&marketplace.capability_registry);

    // Register ccos.network.http-fetch - async HTTP capability
    let handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync> =
        Arc::new(move |input| {
            let input = input.clone();
            let capability_registry = Arc::clone(&capability_registry);
            let approval_queue = approval_queue.clone();
            let internal_secret = internal_secret.clone();
            Box::pin(async move {
                // Extract parameters from input
                let (url, method, headers_map, body, timeout_ms) = match &input {
                    Value::Map(map) => {
                        let url = get_map_string(map, "url").ok_or_else(|| {
                            RuntimeError::Generic("Missing required 'url' parameter".to_string())
                        })?;
                        let method =
                            get_map_string(map, "method").unwrap_or_else(|| "GET".to_string());
                        let headers = get_map_value(map, "headers");
                        let body = get_map_string(map, "body");
                        let timeout_ms = get_map_value(map, "timeout_ms").and_then(|v| match v {
                            Value::Integer(i) if *i > 0 => Some(*i as u64),
                            _ => None,
                        });
                        (url, method, headers.cloned(), body, timeout_ms)
                    }
                    Value::List(args) | Value::Vector(args) => {
                        // Positional: url, method?, headers?, body?
                        let url = args
                            .first()
                            .and_then(|v| v.as_string().map(|s| s.to_string()))
                            .ok_or_else(|| {
                                RuntimeError::Generic(
                                    "Missing required 'url' parameter".to_string(),
                                )
                            })?;
                        let method = args
                            .get(1)
                            .and_then(|v| v.as_string().map(|s| s.to_string()))
                            .unwrap_or_else(|| "GET".to_string());
                        let headers = args.get(2).cloned();
                        let body = args
                            .get(3)
                            .and_then(|v| v.as_string().map(|s| s.to_string()));
                        (url, method, headers, body, None)
                    }
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "map or list".to_string(),
                            actual: input.type_name().to_string(),
                            operation: "ccos.network.http-fetch".to_string(),
                        });
                    }
                };

                // Enforce allowlists from the registry (gateway-controlled egress boundary).
                let parsed = Url::parse(&url).map_err(|e| {
                    RuntimeError::NetworkError(format!("Invalid URL for http-fetch: {}", e))
                })?;
                let host = parsed
                    .host_str()
                    .ok_or_else(|| RuntimeError::NetworkError("URL missing host".to_string()))?
                    .to_lowercase();
                let port = parsed
                    .port_or_known_default()
                    .ok_or_else(|| RuntimeError::NetworkError("URL missing port".to_string()))?;

                log::debug!("[http-fetch] Checking access for {}:{} (url={})", host, port, url);

                {
                    let reg = capability_registry.read().await;
                    let host_allowed = reg.is_http_host_allowed(&host);
                    let port_allowed = reg.is_http_port_allowed(port);
                    
                    log::debug!("[http-fetch] Registry check: host_allowed={}, port_allowed={}", host_allowed, port_allowed);

                    // Also check if host has been explicitly approved via the approval system
                    let host_approved = if let Some(queue) = &approval_queue {
                        queue
                            .is_http_host_approved(&host, Some(port))
                            .await
                            .unwrap_or(false)
                    } else {
                        false
                    };

                    // Check for internal secret bypass
                    let mut internal_bypass = false;
                    if let Some(secret) = &internal_secret {
                        log::debug!("[http-fetch] Checking internal bypass for {}:{}, internal_secret present", host, port);
                        if let Some(Value::Map(hdrs)) = headers_map.as_ref() {
                            log::debug!("[http-fetch] Headers map present with {} entries", hdrs.len());
                            for (k, v) in hdrs.iter() {
                                log::debug!("[http-fetch] Header: {:?} = {:?}", k, v);
                            }
                            let provided_secret = get_map_string(hdrs, "X-Internal-Secret");
                            log::debug!("[http-fetch] Provided secret: {:?}", provided_secret);
                            if let Some(ps) = provided_secret {
                                if ps == *secret {
                                    log::info!(
                                        "[http-fetch] Internal secret bypass granted for {}:{}",
                                        host,
                                        port
                                    );
                                    internal_bypass = true;
                                } else {
                                    log::warn!("[http-fetch] Secret mismatch: expected len={}, got len={}", secret.len(), ps.len());
                                }
                            }
                        } else {
                            log::debug!("[http-fetch] No headers map found");
                        }
                    } else {
                        log::debug!("[http-fetch] No internal_secret configured");
                    }

                    if !host_allowed && !host_approved && !internal_bypass {
                        // Try to create an approval request instead of hard-failing
                        if let Some(queue) = &approval_queue {
                            let session_id = match &input {
                                Value::Map(m) => get_map_string(m, "session_id"),
                                _ => None,
                            };
                            let approval_id = queue
                                .add_http_host_approval(
                                    host.clone(),
                                    Some(port),
                                    url.clone(),
                                    "session".to_string(),
                                    Some("ccos.network.http-fetch".to_string()),
                                    format!(
                                        "HTTP fetch requested access to {}:{}",
                                        host, port
                                    ),
                                    24, // 24-hour expiry
                                    session_id,
                                )
                                .await?;
                            return Err(RuntimeError::Generic(format!(
                                "HTTP host '{}' requires approval. Approval ID: {}\n\nUse: /approve {}\n\nOr visit the approval UI to approve this host, then retry.",
                                host, approval_id, approval_id
                            )));
                        }
                        return Err(RuntimeError::SecurityViolation {
                            operation: "ccos.network.http-fetch".to_string(),
                            capability: "ccos.network.http-fetch".to_string(),
                            context: format!(
                                "Host '{}' not in HTTP allowlist and no approval queue configured. Add the host to the HTTP allowlist to proceed.",
                                host
                            ),
                        });
                    }
                    if !port_allowed && !internal_bypass {
                        return Err(RuntimeError::SecurityViolation {
                            operation: "ccos.network.http-fetch".to_string(),
                            capability: "ccos.network.http-fetch".to_string(),
                            context: format!("Port '{}' not in HTTP allowlist", port),
                        });
                    }
                }

                // Build reqwest client
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .map_err(|e| {
                        RuntimeError::Generic(format!("Failed to create HTTP client: {}", e))
                    })?;

                let method_enum =
                    reqwest::Method::from_bytes(method.as_bytes()).unwrap_or(reqwest::Method::GET);

                let mut request = client.request(method_enum, &url);

                // Add headers if provided
                let mut has_content_type = false;
                let mut has_user_agent = false;
                if let Some(Value::Map(hdrs)) = headers_map.as_ref() {
                    for (key, value) in hdrs.iter() {
                        let key_str = match key {
                            MapKey::String(s) => s.clone(),
                            MapKey::Keyword(k) => k.0.clone(),
                            MapKey::Integer(i) => i.to_string(),
                        };
                        if key_str.eq_ignore_ascii_case("content-type") {
                            has_content_type = true;
                        }
                        if key_str.eq_ignore_ascii_case("user-agent") {
                            has_user_agent = true;
                        }
                        if let Value::String(val_str) = value {
                            request = request.header(key_str, val_str);
                        }
                    }
                }
                if !has_user_agent {
                    request =
                        request.header("User-Agent", "ccos-chat-gateway/0.1 (ccos.http-fetch)");
                }

                // Add body if provided
                let body_len = body.as_ref().map(|b| b.len() as u64).unwrap_or(0);
                if let Some(body_str) = body {
                    let trimmed = body_str.trim();
                    let looks_json = trimmed.starts_with('{') || trimmed.starts_with('[');
                    if looks_json && !has_content_type {
                        request = request.header("Content-Type", "application/json");
                    }
                    request = request.body(body_str);
                }

                if let Some(ms) = timeout_ms {
                    request = request.timeout(std::time::Duration::from_millis(ms));
                }

                // Execute request
                let response = request
                    .send()
                    .await
                    .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

                let status = response.status().as_u16() as i64;
                let response_headers = response.headers().clone();
                let body_text = response.text().await.unwrap_or_default();

                // Best-effort usage accounting for budgeting/telemetry
                let request_body_len = body_len;
                let request_header_len: u64 = match &headers_map {
                    Some(Value::Map(hdrs)) => hdrs
                        .iter()
                        .map(|(k, v)| {
                            let ks = match k {
                                MapKey::String(s) => s.len(),
                                MapKey::Keyword(k) => k.0.len(),
                                MapKey::Integer(i) => i.to_string().len(),
                            };
                            let vs = match v {
                                Value::String(s) => s.len(),
                                other => other.to_string().len(),
                            };
                            (ks + vs) as u64
                        })
                        .sum(),
                    _ => 0,
                };
                let request_url_len = url.len() as u64;
                let network_egress_bytes = request_body_len + request_header_len + request_url_len;
                let network_ingress_bytes = body_text.len() as u64;

                // Build response map
                let mut result_map: HashMap<MapKey, Value> = HashMap::new();
                result_map.insert(MapKey::String("status".to_string()), Value::Integer(status));
                result_map.insert(MapKey::String("body".to_string()), Value::String(body_text));

                // Convert headers to RTFS map
                let mut headers_result: HashMap<MapKey, Value> = HashMap::new();
                for (key, value) in response_headers.iter() {
                    headers_result.insert(
                        MapKey::String(key.to_string()),
                        Value::String(value.to_str().unwrap_or("").to_string()),
                    );
                }
                result_map.insert(
                    MapKey::String("headers".to_string()),
                    Value::Map(headers_result),
                );

                let mut usage: HashMap<MapKey, Value> = HashMap::new();
                usage.insert(
                    MapKey::String("network_egress_bytes".to_string()),
                    Value::Integer(network_egress_bytes as i64),
                );
                usage.insert(
                    MapKey::String("network_ingress_bytes".to_string()),
                    Value::Integer(network_ingress_bytes as i64),
                );
                result_map.insert(MapKey::String("usage".to_string()), Value::Map(usage));

                Ok(Value::Map(result_map))
            })
        });

    marketplace
        .register_native_capability_with_schema(
            "ccos.network.http-fetch".to_string(),
            "HTTP Fetch".to_string(),
            "Perform HTTP requests (GET, POST, PUT, DELETE, etc.). Returns status code, body, and headers.".to_string(),
            handler,
            "network".to_string(),
            Some(input_schema),
            Some(output_schema),
        )
        .await
        .map_err(|e| {
            RuntimeError::Generic(format!("Failed to register ccos.network.http-fetch: {:?}", e))
        })?;

    Ok(())
}
