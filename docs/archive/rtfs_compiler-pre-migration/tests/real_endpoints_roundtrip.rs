use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rtfs_compiler::ast::MapKey;
use rtfs_compiler::ccos::capabilities::SessionPoolManager;
use rtfs_compiler::ccos::capability_marketplace::types::{
    CapabilityManifest, HttpCapability, OpenApiAuth, OpenApiCapability, OpenApiOperation,
    ProviderType,
};
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::error::{RuntimeError, RuntimeResult};
use rtfs_compiler::runtime::values::Value;
use tempfile::TempDir;
use tokio::sync::RwLock;

async fn setup_marketplace_with_sessions() -> RuntimeResult<(CapabilityMarketplace, TempDir)> {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    let session_pool = Arc::new(SessionPoolManager::new());
    marketplace.set_session_pool(session_pool).await;
    let temp_dir = TempDir::new()
        .map_err(|e| RuntimeError::Generic(format!("Failed to create temporary directory: {e}")))?;
    Ok((marketplace, temp_dir))
}

#[tokio::test]
async fn test_real_endpoints_roundtrip_export_import_and_execute() -> Result<(), RuntimeError> {
    let _openweather_api_key = match std::env::var("OPENWEATHER_API_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            eprintln!(
                "Skipping real endpoint roundtrip test – OPENWEATHER_API_KEY environment variable is not set"
            );
            return Ok(());
        }
    };

    let (marketplace, tmpdir) = setup_marketplace_with_sessions().await?;

    let mut github_metadata = HashMap::new();
    github_metadata.insert("source".to_string(), "integration_test".to_string());

    let github_capability = CapabilityManifest {
        id: "real.http.github.list_issues".to_string(),
        name: "GitHub Issues (HTTP)".to_string(),
        description: "Fetch issues for a repository using the GitHub REST API".to_string(),
        provider: ProviderType::Http(HttpCapability {
            base_url: "https://api.github.com/repos/octocat/hello-world/issues".to_string(),
            auth_token: None,
            timeout_ms: 15_000,
        }),
        version: "1.0.0".to_string(),
        input_schema: None,
        output_schema: None,
        attestation: None,
        provenance: None,
        permissions: vec!["http:request".to_string()],
        effects: vec![":network".to_string()],
        metadata: github_metadata,
        agent_metadata: None,
    };

    marketplace
        .register_capability_manifest(github_capability.clone())
        .await?;

    let mut weather_metadata = HashMap::new();
    weather_metadata.insert(
        "openapi_provider_kind".to_string(),
        "weather_api".to_string(),
    );

    let openweather_capability = CapabilityManifest {
        id: "real.openapi.openweather.current_weather".to_string(),
        name: "OpenWeather Current Weather".to_string(),
        description: "Fetch current weather conditions from OpenWeather".to_string(),
        provider: ProviderType::OpenApi(OpenApiCapability {
            base_url: "https://api.openweathermap.org".to_string(),
            spec_url: Some(
                "https://raw.githubusercontent.com/APIs-guru/openapi-directory/main/APIs/openweathermap.org/data/2.5/weather/openapi.yaml"
                    .to_string(),
            ),
            operations: vec![OpenApiOperation {
                operation_id: Some("getCurrentWeather".to_string()),
                path: "/data/2.5/weather".to_string(),
                method: "GET".to_string(),
                summary: Some("Fetch current weather".to_string()),
                description: Some(
                    "Call the OpenWeather current weather endpoint with city and units".to_string(),
                ),
            }],
            auth: Some(OpenApiAuth {
                auth_type: "api_key".to_string(),
                location: "query".to_string(),
                parameter_name: "appid".to_string(),
                env_var_name: Some("OPENWEATHER_API_KEY".to_string()),
                required: true,
            }),
            timeout_ms: 20_000,
        }),
        version: "1.0.0".to_string(),
        input_schema: None,
        output_schema: None,
        attestation: None,
        provenance: None,
        permissions: vec!["http:request".to_string()],
        effects: vec![":network".to_string()],
        metadata: weather_metadata,
        agent_metadata: None,
    };

    marketplace
        .register_capability_manifest(openweather_capability.clone())
        .await?;

    let export_dir = tmpdir.path().join("rtfs_export");
    std::fs::create_dir_all(&export_dir)
        .map_err(|e| RuntimeError::Generic(format!("Failed to create export directory: {e}")))?;

    let exported_rtfs = marketplace
        .export_capabilities_to_rtfs_dir(&export_dir)
        .await?;
    assert_eq!(
        exported_rtfs, 2,
        "Expected to export two capabilities to RTFS files"
    );

    let json_file = tmpdir.path().join("capabilities.json");
    let exported_json = marketplace.export_capabilities_to_file(&json_file).await?;
    assert_eq!(
        exported_json, 2,
        "Expected to export two capabilities to JSON"
    );

    let rtfs_files: Vec<PathBuf> = std::fs::read_dir(&export_dir)
        .map_err(|e| RuntimeError::Generic(format!("Failed to read RTFS export directory: {e}")))?
        .filter_map(|entry| match entry {
            Ok(dir_entry) => {
                let path = dir_entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("rtfs") {
                    Some(path)
                } else {
                    None
                }
            }
            Err(_) => None,
        })
        .collect();
    assert_eq!(
        rtfs_files.len(),
        2,
        "Should have two RTFS manifests on disk"
    );

    let (marketplace_from_rtfs, _tmpdir_rtfs) = setup_marketplace_with_sessions().await?;
    let imported_count = marketplace_from_rtfs
        .import_capabilities_from_rtfs_dir(&export_dir)
        .await?;
    assert_eq!(
        imported_count, 2,
        "Should import the two exported capabilities"
    );

    let github_headers = {
        let mut headers = HashMap::with_capacity(3);
        headers.insert(
            MapKey::String("User-Agent".to_string()),
            Value::String("ccos-integration-test/1.0".to_string()),
        );
        headers.insert(
            MapKey::String("Accept".to_string()),
            Value::String("application/vnd.github+json".to_string()),
        );
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.trim().is_empty() {
                headers.insert(
                    MapKey::String("Authorization".to_string()),
                    Value::String(format!("Bearer {token}")),
                );
            }
        }
        Value::Map(headers)
    };

    let github_url = "https://api.github.com/repos/octocat/hello-world/issues?per_page=1";
    let github_inputs = Value::Vector(vec![
        Value::String(github_url.to_string()),
        Value::String("GET".to_string()),
        github_headers,
    ]);

    let github_result = marketplace_from_rtfs
        .execute_capability(&github_capability.id, &github_inputs)
        .await?;

    let github_map = match github_result {
        Value::Map(map) => map,
        other => {
            return Err(RuntimeError::Generic(format!(
                "GitHub execution returned non-map value: {}",
                other.type_name()
            )))
        }
    };

    let github_status_value = github_map
        .get(&MapKey::String("status".to_string()))
        .ok_or_else(|| RuntimeError::Generic("Missing status from GitHub response".to_string()))?;
    let github_status = match github_status_value {
        Value::Integer(code) => *code,
        other => {
            return Err(RuntimeError::Generic(format!(
                "Unexpected status value type from GitHub response: {}",
                other.type_name()
            )))
        }
    };
    assert_eq!(
        github_status, 200,
        "GitHub API should respond with HTTP 200"
    );

    let github_body_value = github_map
        .get(&MapKey::String("body".to_string()))
        .ok_or_else(|| RuntimeError::Generic("Missing body from GitHub response".to_string()))?;
    let github_body = match github_body_value {
        Value::String(body) => body,
        other => {
            return Err(RuntimeError::Generic(format!(
                "GitHub response body has unexpected type: {}",
                other.type_name()
            )))
        }
    };

    let github_json: serde_json::Value = serde_json::from_str(github_body)
        .map_err(|e| RuntimeError::Generic(format!("Failed to parse GitHub JSON response: {e}")))?;
    let issues = github_json.as_array().ok_or_else(|| {
        RuntimeError::Generic("GitHub issues response is not an array".to_string())
    })?;
    if let Some(first_issue) = issues.first() {
        let has_title = first_issue
            .as_object()
            .map(|issue| issue.contains_key("title"))
            .unwrap_or(false);
        assert!(
            has_title,
            "GitHub issue entries should include a title field"
        );
    }

    let mut weather_params = HashMap::with_capacity(2);
    weather_params.insert(
        MapKey::String("q".to_string()),
        Value::String("London".to_string()),
    );
    weather_params.insert(
        MapKey::String("units".to_string()),
        Value::String("metric".to_string()),
    );

    let mut weather_input_map = HashMap::with_capacity(2);
    weather_input_map.insert(
        MapKey::String("operation".to_string()),
        Value::String("getCurrentWeather".to_string()),
    );
    weather_input_map.insert(
        MapKey::String("params".to_string()),
        Value::Map(weather_params),
    );
    let weather_inputs = Value::Map(weather_input_map);

    let weather_result = marketplace_from_rtfs
        .execute_capability(&openweather_capability.id, &weather_inputs)
        .await?;
    let weather_map = match weather_result {
        Value::Map(map) => map,
        other => {
            return Err(RuntimeError::Generic(format!(
                "OpenWeather execution returned non-map value: {}",
                other.type_name()
            )))
        }
    };

    let weather_status_value = weather_map
        .get(&MapKey::String("status".to_string()))
        .ok_or_else(|| {
            RuntimeError::Generic("Missing status from OpenWeather response".to_string())
        })?;
    let weather_status = match weather_status_value {
        Value::Integer(code) => *code,
        other => {
            return Err(RuntimeError::Generic(format!(
                "Unexpected status value type from OpenWeather response: {}",
                other.type_name()
            )))
        }
    };
    assert_eq!(
        weather_status, 200,
        "OpenWeather API should respond with HTTP 200"
    );

    let weather_json_value = weather_map
        .get(&MapKey::String("json".to_string()))
        .ok_or_else(|| {
            RuntimeError::Generic("Missing JSON payload from OpenWeather response".to_string())
        })?;
    let weather_json_map = match weather_json_value {
        Value::Map(map) => map,
        other => {
            return Err(RuntimeError::Generic(format!(
                "OpenWeather JSON payload has unexpected type: {}",
                other.type_name()
            )))
        }
    };

    let optional_city = weather_json_map
        .get(&MapKey::String("name".to_string()))
        .and_then(|value| match value {
            Value::String(city) => Some(city.clone()),
            _ => None,
        });
    assert!(
        optional_city.is_some(),
        "OpenWeather response should include a city name"
    );

    let weather_vector = weather_json_map
        .get(&MapKey::String("weather".to_string()))
        .ok_or_else(|| {
            RuntimeError::Generic("Missing weather array in OpenWeather response".to_string())
        })?;
    match weather_vector {
        Value::Vector(entries) => {
            assert!(!entries.is_empty(), "Weather array should not be empty");
        }
        other => {
            return Err(RuntimeError::Generic(format!(
                "OpenWeather weather field has unexpected type: {}",
                other.type_name()
            )))
        }
    }

    Ok(())
}
