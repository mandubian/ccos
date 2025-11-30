//! APIs.guru OpenAPI Directory Integration
//!
//! This module queries the APIs.guru openapi-directory for community-maintained
//! OpenAPI specifications when local discovery fails.
//!
//! Repository: https://github.com/APIs-guru/openapi-directory

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::Deserialize;
use std::collections::HashMap;

/// APIs.guru API entry
#[derive(Debug, Clone, Deserialize)]
pub struct ApisGuruEntry {
    pub added: Option<String>,
    pub preferred: Option<String>,
    pub versions: HashMap<String, ApisGuruVersion>,
}

/// APIs.guru API version info
#[derive(Debug, Clone, Deserialize)]
pub struct ApisGuruVersion {
    pub added: Option<String>,
    pub info: ApisGuruInfo,
    #[serde(rename = "swaggerUrl")]
    pub swagger_url: Option<String>,
    #[serde(rename = "openapiVer")]
    pub openapi_ver: Option<String>,
    pub link: Option<String>,
}

/// APIs.guru API info
#[derive(Debug, Clone, Deserialize)]
pub struct ApisGuruInfo {
    pub title: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "x-logo")]
    pub logo: Option<serde_json::Value>,
}

/// Search result from APIs.guru
#[derive(Debug, Clone)]
pub struct ApisGuruSearchResult {
    pub api_id: String,
    pub title: String,
    pub description: String,
    pub version: String,
    pub openapi_url: String,
}

/// APIs.guru client for querying the OpenAPI directory
pub struct ApisGuruClient {
    /// Base URL for the APIs.guru API
    base_url: String,
    /// HTTP client
    client: reqwest::Client,
}

impl ApisGuruClient {
    /// Create a new APIs.guru client
    pub fn new() -> Self {
        Self {
            base_url: "https://api.apis.guru/v2".to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Get the list of all APIs
    pub async fn list_apis(&self) -> RuntimeResult<HashMap<String, ApisGuruEntry>> {
        let url = format!("{}/list.json", self.base_url);

        let response = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to fetch APIs.guru list: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::Generic(format!(
                "APIs.guru returned HTTP {}",
                response.status()
            )));
        }

        let apis: HashMap<String, ApisGuruEntry> = response
            .json()
            .await
            .map_err(|e| RuntimeError::Generic(format!("Failed to parse APIs.guru response: {}", e)))?;

        Ok(apis)
    }

    /// Search for APIs by keyword
    pub async fn search(&self, query: &str) -> RuntimeResult<Vec<ApisGuruSearchResult>> {
        let apis = self.list_apis().await?;
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for (api_id, entry) in apis {
            // Check if query matches API ID or any version info
            let id_matches = api_id.to_lowercase().contains(&query_lower);

            // Get the preferred version or the first available
            let version_key = entry
                .preferred
                .clone()
                .or_else(|| entry.versions.keys().next().cloned());

            if let Some(version_key) = version_key {
                if let Some(version) = entry.versions.get(&version_key) {
                    let title = version.info.title.clone().unwrap_or_default();
                    let description = version.info.description.clone().unwrap_or_default();

                    let title_matches = title.to_lowercase().contains(&query_lower);
                    let desc_matches = description.to_lowercase().contains(&query_lower);

                    if id_matches || title_matches || desc_matches {
                        if let Some(openapi_url) = &version.swagger_url {
                            results.push(ApisGuruSearchResult {
                                api_id: api_id.clone(),
                                title,
                                description,
                                version: version.info.version.clone().unwrap_or_default(),
                                openapi_url: openapi_url.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Sort by relevance (exact matches first)
        results.sort_by(|a, b| {
            let a_exact = a.api_id.to_lowercase() == query_lower
                || a.title.to_lowercase() == query_lower;
            let b_exact = b.api_id.to_lowercase() == query_lower
                || b.title.to_lowercase() == query_lower;

            match (a_exact, b_exact) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.api_id.cmp(&b.api_id),
            }
        });

        Ok(results)
    }

    /// Get OpenAPI spec URL for a specific API
    pub async fn get_spec_url(&self, api_id: &str) -> RuntimeResult<Option<String>> {
        let apis = self.list_apis().await?;

        // Try exact match first
        if let Some(entry) = apis.get(api_id) {
            let version_key = entry
                .preferred
                .clone()
                .or_else(|| entry.versions.keys().next().cloned());

            if let Some(version_key) = version_key {
                if let Some(version) = entry.versions.get(&version_key) {
                    return Ok(version.swagger_url.clone());
                }
            }
        }

        // Try partial match
        let api_id_lower = api_id.to_lowercase();
        for (key, entry) in &apis {
            if key.to_lowercase().contains(&api_id_lower) {
                let version_key = entry
                    .preferred
                    .clone()
                    .or_else(|| entry.versions.keys().next().cloned());

                if let Some(version_key) = version_key {
                    if let Some(version) = entry.versions.get(&version_key) {
                        return Ok(version.swagger_url.clone());
                    }
                }
            }
        }

        Ok(None)
    }
}

impl Default for ApisGuruClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_apis() {
        let client = ApisGuruClient::new();
        
        // This test requires network access
        if let Ok(results) = client.search("github").await {
            println!("Found {} APIs matching 'github'", results.len());
            for result in results.iter().take(5) {
                println!("  - {}: {}", result.api_id, result.title);
            }
        }
    }
}
