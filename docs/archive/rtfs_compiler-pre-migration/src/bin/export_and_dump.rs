use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rtfs_compiler::ccos::capability_marketplace::types::{
    CapabilityManifest, HttpCapability, OpenApiAuth, OpenApiCapability, OpenApiOperation,
    ProviderType,
};
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Create capabilities similar to the test
    let mut github_metadata = HashMap::new();
    github_metadata.insert("source".to_string(), "export_and_dump".to_string());

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
                description: Some("Call the OpenWeather current weather endpoint with city and units".to_string()),
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

    // Persistent export dir under /tmp
    let mut dir = std::env::temp_dir();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let pid = std::process::id();
    dir.push(format!("ccos_exports_{}_{}", pid, ts));
    std::fs::create_dir_all(&dir)?;

    println!("Exporting capabilities to {}", dir.display());

    let exported_rtfs = marketplace.export_capabilities_to_rtfs_dir(&dir).await?;
    println!("Exported {} RTFS files", exported_rtfs);

    let json_file = dir.join("capabilities.json");
    let exported_json = marketplace.export_capabilities_to_file(&json_file).await?;
    println!(
        "Exported {} capabilities to JSON: {}",
        exported_json,
        json_file.display()
    );

    // List files
    for entry in std::fs::read_dir(&dir)? {
        let e = entry?;
        println!("- {}", e.path().display());
    }

    Ok(())
}
