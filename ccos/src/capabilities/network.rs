//! Network capabilities for the CCOS marketplace
//!
//! Provides HTTP capabilities that can be discovered and executed
//! through the CapabilityMarketplace.

use crate::capability_marketplace::CapabilityMarketplace;
use futures::future::BoxFuture;
use rtfs::ast::{Keyword, MapKey, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;

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
) -> RuntimeResult<()> {
    // Input schema for http-fetch: {:url String :method? String :headers? Map :body? String}
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
        ],
        wildcard: None,
    };

    // Output schema: {:status Int :body String :headers Map}
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
        ],
        wildcard: None,
    };

    // Register ccos.network.http-fetch - async HTTP capability
    let handler: Arc<dyn Fn(&Value) -> BoxFuture<'static, RuntimeResult<Value>> + Send + Sync> =
        Arc::new(|input| {
            let input = input.clone();
            Box::pin(async move {
                // Extract parameters from input
                let (url, method, headers_map, body) = match &input {
                    Value::Map(map) => {
                        let url = get_map_string(map, "url").ok_or_else(|| {
                            RuntimeError::Generic("Missing required 'url' parameter".to_string())
                        })?;
                        let method =
                            get_map_string(map, "method").unwrap_or_else(|| "GET".to_string());
                        let headers = get_map_value(map, "headers");
                        let body = get_map_string(map, "body");
                        (url, method, headers.cloned(), body)
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
                        (url, method, headers, body)
                    }
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "map or list".to_string(),
                            actual: input.type_name().to_string(),
                            operation: "ccos.network.http-fetch".to_string(),
                        });
                    }
                };

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
                if let Some(Value::Map(hdrs)) = headers_map {
                    for (key, value) in hdrs.iter() {
                        let key_str = match key {
                            MapKey::String(s) => s.clone(),
                            MapKey::Keyword(k) => k.0.clone(),
                            MapKey::Integer(i) => i.to_string(),
                        };
                        if key_str.eq_ignore_ascii_case("content-type") {
                            has_content_type = true;
                        }
                        if let Value::String(val_str) = value {
                            request = request.header(key_str, val_str);
                        }
                    }
                }

                // Add body if provided
                if let Some(body_str) = body {
                    let trimmed = body_str.trim();
                    let looks_json = trimmed.starts_with('{') || trimmed.starts_with('[');
                    if looks_json && !has_content_type {
                        request = request.header("Content-Type", "application/json");
                    }
                    request = request.body(body_str);
                }

                // Execute request
                let response = request
                    .send()
                    .await
                    .map_err(|e| RuntimeError::Generic(format!("HTTP request failed: {}", e)))?;

                let status = response.status().as_u16() as i64;
                let response_headers = response.headers().clone();
                let body_text = response.text().await.unwrap_or_default();

                // Build response map
                let mut result_map: HashMap<MapKey, Value> = HashMap::new();
                result_map.insert(
                    MapKey::Keyword(Keyword("status".to_string())),
                    Value::Integer(status),
                );
                result_map.insert(
                    MapKey::Keyword(Keyword("body".to_string())),
                    Value::String(body_text),
                );

                // Convert headers to RTFS map
                let mut headers_result: HashMap<MapKey, Value> = HashMap::new();
                for (key, value) in response_headers.iter() {
                    headers_result.insert(
                        MapKey::String(key.to_string()),
                        Value::String(value.to_str().unwrap_or("").to_string()),
                    );
                }
                result_map.insert(
                    MapKey::Keyword(Keyword("headers".to_string())),
                    Value::Map(headers_result),
                );

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
