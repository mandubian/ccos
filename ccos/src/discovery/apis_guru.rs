//! APIs.guru client for OpenAPI directory search
//!
//! Searches the APIs.guru directory (https://apis.guru) for OpenAPI specifications
//! that can be converted to MCP-compatible server endpoints.

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// APIs.guru API client
pub struct ApisGuruClient {
    base_url: String,
    client: reqwest::Client,
}

/// APIs.guru API entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApisGuruApi {
    // Note: 'name' is not in the API response - it's the HashMap key
    // We keep it here for convenience but it should be populated from the key
    #[serde(skip, default)]
    pub name: String,
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default)]
    pub preferred: Option<String>,
    pub versions: HashMap<String, ApisGuruVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApisGuruVersion {
    #[serde(default)]
    pub added: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
    pub info: ApisGuruVersionInfo,
    #[serde(rename = "swaggerUrl")]
    pub swagger_url: Option<String>,
    #[serde(rename = "swaggerYamlUrl")]
    pub swagger_yaml_url: Option<String>,
    #[serde(rename = "openapiVer", default)]
    pub openapi_ver: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(rename = "externalDocs", default)]
    pub external_docs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApisGuruVersionInfo {
    pub title: String,
    pub description: Option<String>,
    pub version: String,
    #[serde(rename = "x-providerName", default)]
    pub provider_name: Option<String>,
    #[serde(rename = "x-logo", default)]
    pub logo: Option<ApisGuruLogo>,
    #[serde(default)]
    pub contact: Option<serde_json::Value>,
    #[serde(default)]
    pub license: Option<serde_json::Value>,
    #[serde(rename = "termsOfService", default)]
    pub terms_of_service: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApisGuruLogo {
    pub url: String,
}

/// Search result from APIs.guru
#[derive(Debug, Clone)]
pub struct ApisGuruSearchResult {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub openapi_url: Option<String>,
    pub swagger_url: Option<String>,
    pub provider: Option<String>,
}

impl ApisGuruClient {
    pub fn new() -> Self {
        // Create client with longer timeouts for large file downloads
        // APIs.guru list.json is ~9MB and can take time to download
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new()); // Fallback to default if builder fails
        
        Self {
            base_url: "https://api.apis.guru".to_string(),
            client,
        }
    }

    /// Search APIs.guru directory for APIs matching the query
    pub async fn search(&self, query: &str) -> RuntimeResult<Vec<ApisGuruSearchResult>> {
        // APIs.guru doesn't have a search endpoint, so we list all APIs and filter
        // In production, you might want to cache this or use a different approach
        let all_apis = self.list_all_apis().await?;

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let mut results = Vec::new();

        for (api_name, api) in all_apis {
            // Get info from preferred version or first available version
            let version_info = api
                .preferred
                .as_ref()
                .and_then(|pref| api.versions.get(pref))
                .or_else(|| api.versions.values().next());

            if let Some(version) = version_info {
                let title_lower = version.info.title.to_lowercase();
                let provider_lower = version
                    .info
                    .provider_name
                    .as_ref()
                    .map(|p| p.to_lowercase())
                    .unwrap_or_default();

                let desc_lower = version
                    .info
                    .description
                    .as_ref()
                    .map(|d| d.to_lowercase())
                    .unwrap_or_default();

                // Match if query appears in title, description, or provider name
                let full_match = title_lower.contains(&query_lower)
                    || desc_lower.contains(&query_lower)
                    || provider_lower.contains(&query_lower);

                // Or if all query words are present
                let all_words_match = if query_words.len() > 1 {
                    query_words.iter().all(|word| {
                        title_lower.contains(word)
                            || desc_lower.contains(word)
                            || provider_lower.contains(word)
                    })
                } else {
                    false
                };

                if full_match || all_words_match {
                    // Use swaggerUrl or swaggerYamlUrl as the OpenAPI spec URL
                    let openapi_url = version
                        .swagger_url
                        .clone()
                        .or(version.swagger_yaml_url.clone());

                    results.push(ApisGuruSearchResult {
                        name: api_name.clone(),
                        title: version.info.title.clone(),
                        description: version.info.description.clone(),
                        openapi_url: openapi_url.clone(),
                        swagger_url: version.swagger_url.clone(),
                        provider: version.info.provider_name.clone(),
                    });
                }
            }
        }

        Ok(results)
    }

    /// List all APIs from APIs.guru
    async fn list_all_apis(&self) -> RuntimeResult<HashMap<String, ApisGuruApi>> {
        let url = format!("{}/v2/list.json", self.base_url);

        // APIs.guru list.json is very large (thousands of APIs), so we need a longer timeout
        let response = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(120)) // 2 minutes for large file download
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    RuntimeError::Generic(format!(
                        "APIS.guru operation timed out (the list.json file is very large, ~9MB). Try again or check your connection."
                    ))
                } else {
                    RuntimeError::Generic(format!("Failed to fetch APIs.guru list: {}", e))
                }
            })?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "APIs.guru API error: {}",
                response.status()
            )));
        }

        // Parse as a generic JSON object first to handle the structure
        let json: serde_json::Value = response.json().await.map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse APIs.guru response: {}", e))
        })?;

        // The response is a JSON object where keys are API names and values are API objects
        // The API objects don't have a 'name' field - the name is the key
        let mut apis = HashMap::new();

        if let Some(obj) = json.as_object() {
            let mut parse_errors = 0;
            for (api_name, api_value) in obj {
                // Try to deserialize the API object
                match serde_json::from_value::<ApisGuruApi>(api_value.clone()) {
                    Ok(mut api) => {
                        // Set the name from the key
                        api.name = api_name.clone();
                        apis.insert(api_name.clone(), api);
                    }
                    Err(e) => {
                        // Only log first few errors to avoid spam
                        if parse_errors < 3 {
                            eprintln!(
                                "Warning: Failed to parse API '{}' from APIs.guru: {}",
                                api_name, e
                            );
                        }
                        parse_errors += 1;
                    }
                }
            }

            if parse_errors > 0 {
                eprintln!(
                    "Note: {} APIs failed to parse (showing first 3 errors)",
                    parse_errors
                );
            }
        } else {
            return Err(RuntimeError::Generic(
                "APIs.guru response is not a JSON object".to_string(),
            ));
        }

        Ok(apis)
    }
}
