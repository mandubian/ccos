//! Browser-based API Discovery Service
//!
//! Uses headless Puppeteer (via MCP) to extract API information from
//! JavaScript-rendered pages (SPAs, Swagger UI, etc.)

use crate::capability_marketplace::CapabilityMarketplace;
use crate::mcp::discovery_session::MCPSessionManager;
use rtfs::runtime::error::RuntimeResult;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// Result of browser-based API discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserDiscoveryResult {
    pub success: bool,
    pub source_url: String,
    pub page_title: Option<String>,
    pub extracted_html: Option<String>,
    pub discovered_endpoints: Vec<DiscoveredEndpoint>,
    pub found_openapi_urls: Vec<String>,
    /// OpenAPI spec URL discovered from Swagger UI globals or script tags
    pub spec_url: Option<String>,
    pub error: Option<String>,
}

/// An endpoint discovered from browser extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredEndpoint {
    pub method: String,
    pub path: String,
    pub description: Option<String>,
}

/// Service for browser-based API discovery
pub struct BrowserDiscoveryService {
    puppeteer_endpoint: String,
    marketplace: Option<Arc<CapabilityMarketplace>>,
}

impl BrowserDiscoveryService {
    /// Create a new browser discovery service
    pub fn new() -> Self {
        Self {
            puppeteer_endpoint: "npx -y @modelcontextprotocol/server-puppeteer".to_string(),
            marketplace: None,
        }
    }

    /// Create with a specific Puppeteer endpoint
    pub fn with_endpoint(endpoint: String) -> Self {
        Self {
            puppeteer_endpoint: endpoint,
            marketplace: None,
        }
    }

    /// Set the marketplace for capability resolution
    pub fn with_marketplace(mut self, marketplace: Arc<CapabilityMarketplace>) -> Self {
        self.marketplace = Some(marketplace);
        self
    }

    /// Extract API information from a URL using headless browser
    pub async fn extract_from_url(&self, url: &str) -> RuntimeResult<BrowserDiscoveryResult> {
        use crate::mcp::discovery_session::MCPServerInfo;

        eprintln!("[BrowserDiscovery] Navigating to: {}", url);

        // Create MCP session manager (no auth needed for local Puppeteer)
        let session_manager = MCPSessionManager::new(None);

        // Create client info for MCP protocol
        let client_info = MCPServerInfo {
            name: "ccos-browser-discovery".to_string(),
            version: "1.0.0".to_string(),
        };

        // Initialize session with Puppeteer endpoint
        let session = session_manager
            .initialize_session(&self.puppeteer_endpoint, &client_info)
            .await?;

        // Navigate to URL with headless mode
        let _nav_result = session_manager
            .call_tool(
                &session,
                "puppeteer_navigate",
                json!({
                    "url": url,
                    "launchOptions": {
                        "headless": true
                    }
                }),
            )
            .await?;

        // Wait a bit for JavaScript to render (SPAs like Swagger UI take time)
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        // Extract page content using puppeteer_evaluate
        eprintln!("[BrowserDiscovery] Calling puppeteer_evaluate...");
        let eval_result = session_manager
            .call_tool(
                &session,
                "puppeteer_evaluate",
                json!({
                    "script": r#"
                        (function() {
                            const result = {
                                title: document.title,
                                html: document.body.innerHTML.substring(0, 50000),
                                links: [],
                                apiEndpoints: [],
                                swaggerSpec: null,
                                specUrl: null
                            };
                            
                            // 1. Check for Swagger UI window globals
                            if (window.ui) {
                                try {
                                    if (typeof window.ui.getConfigs === 'function') {
                                        const config = window.ui.getConfigs();
                                        if (config && config.url) {
                                            result.specUrl = config.url;
                                        }
                                        if (config && config.urls && config.urls.length > 0) {
                                            result.specUrl = config.urls[0].url;
                                        }
                                    } else if (window.ui.config && window.ui.config.url) {
                                        result.specUrl = window.ui.config.url;
                                    }
                                } catch (e) {}
                            }
                            
                            // 1b. Check for common Swagger UI element for the spec URL
                            if (!result.specUrl) {
                                const downloadInput = document.querySelector('input.download-url-input');
                                if (downloadInput && downloadInput.value) {
                                    result.specUrl = downloadInput.value;
                                }
                            }
                            
                            // 2. Check for Swagger spec in window globals
                            if (window.swaggerSpec) {
                                result.swaggerSpec = JSON.stringify(window.swaggerSpec).substring(0, 10000);
                            }
                            
                            // 3. Extract from rendered Swagger UI DOM
                            document.querySelectorAll('.opblock').forEach(block => {
                                const methodEl = block.querySelector('.opblock-summary-method');
                                const pathEl = block.querySelector('.opblock-summary-path, .opblock-summary-path__deprecated');
                                const descEl = block.querySelector('.opblock-summary-description');
                                
                                if (methodEl && pathEl) {
                                    result.apiEndpoints.push({
                                        method: methodEl.textContent.trim(),
                                        path: pathEl.textContent.trim().replace(/[{}]/g, ''),
                                        description: descEl ? descEl.textContent.trim() : ''
                                    });
                                }
                            });
                            
                            // 4. Also check for Redoc-style API docs
                            document.querySelectorAll('[data-section-id]').forEach(section => {
                                const methodEl = section.querySelector('.http-verb');
                                const pathEl = section.querySelector('.api-content path');
                                if (methodEl) {
                                    const method = methodEl.textContent.trim().toUpperCase();
                                    const pathText = pathEl ? pathEl.textContent.trim() : '';
                                    if (method && pathText) {
                                        result.apiEndpoints.push({ method, path: pathText, description: '' });
                                    }
                                }
                            });
                            
                            // 5. Find potential OpenAPI/Swagger links in <a> tags
                            document.querySelectorAll('a[href]').forEach(a => {
                                const href = a.href.toLowerCase();
                                if (href.includes('openapi') || 
                                    href.includes('swagger') || 
                                    href.endsWith('.json') || 
                                    href.endsWith('.yaml') ||
                                    href.endsWith('.yml')) {
                                    result.links.push(a.href);
                                }
                            });
                            
                            // 6. Look for spec URLs in script tags
                            document.querySelectorAll('script').forEach(script => {
                                const text = script.textContent || '';
                                const urlMatch = text.match(/url\s*:\s*["']([^"']+(?:\.json|\.yaml|\.yml|openapi|swagger)[^"']*)["']/i);
                                if (urlMatch && !result.specUrl) {
                                    result.specUrl = urlMatch[1];
                                }
                            });
                            
                            // 7. Fallback: Look for API patterns in rendered text
                            if (result.apiEndpoints.length === 0) {
                                const text = document.body.innerText;
                                const patterns = [
                                    /GET\s+\/[a-zA-Z0-9\/_{}.-]+/g,
                                    /POST\s+\/[a-zA-Z0-9\/_{}.-]+/g,
                                    /PUT\s+\/[a-zA-Z0-9\/_{}.-]+/g,
                                    /DELETE\s+\/[a-zA-Z0-9\/_{}.-]+/g,
                                    /PATCH\s+\/[a-zA-Z0-9\/_{}.-]+/g
                                ];
                                
                                patterns.forEach(pattern => {
                                    const matches = text.match(pattern) || [];
                                    matches.forEach(m => {
                                        const parts = m.split(/\s+/);
                                        if (parts.length >= 2) {
                                            result.apiEndpoints.push({
                                                method: parts[0],
                                                path: parts[1]
                                            });
                                        }
                                    });
                                });
                            }
                            
                            return result;
                        })()
                    "#
                }),
            )
            .await;

        match eval_result {
            Ok(eval_result) => {
                let content_str = eval_result
                    .content
                    .first()
                    .and_then(|c| c.text.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("");

                eprintln!(
                    "[BrowserDiscovery DEBUG] Raw content len: {}",
                    content_str.len()
                );

                // Robust extraction of JSON object
                let mut json_content = content_str;
                if let (Some(start), Some(end)) = (content_str.find('{'), content_str.rfind('}')) {
                    json_content = &content_str[start..=end];
                }

                // Try to parse
                let page_data =
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_content) {
                        Some(v)
                    } else if json_content.starts_with('"') {
                        // It might be a double-quoted JSON string
                        if let Ok(unescaped) = serde_json::from_str::<String>(json_content) {
                            serde_json::from_str::<serde_json::Value>(&unescaped).ok()
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                if let Some(v) = page_data {
                    eprintln!("[BrowserDiscovery DEBUG] Successfully parsed JSON result");
                    let title = v
                        .get("title")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string());
                    let html = v
                        .get("html")
                        .and_then(|h| h.as_str())
                        .map(|s| s.to_string());
                    let spec_url = v
                        .get("specUrl")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    let found_links: Vec<String> = v
                        .get("links")
                        .and_then(|l| l.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();

                    let endpoints: Vec<DiscoveredEndpoint> = v
                        .get("apiEndpoints")
                        .and_then(|e| e.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| {
                                    let method = v.get("method")?.as_str()?.to_string();
                                    let path = v.get("path")?.as_str()?.to_string();
                                    let description = v
                                        .get("description")
                                        .and_then(|d| d.as_str())
                                        .map(|s| s.to_string());
                                    Some(DiscoveredEndpoint {
                                        method,
                                        path,
                                        description,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    Ok(BrowserDiscoveryResult {
                        success: true,
                        source_url: url.to_string(),
                        page_title: title,
                        extracted_html: html,
                        discovered_endpoints: endpoints,
                        found_openapi_urls: found_links,
                        spec_url,
                        error: None,
                    })
                } else {
                    eprintln!(
                        "[BrowserDiscovery DEBUG] Failed to parse JSON. Content start: {}",
                        if json_content.len() > 100 {
                            &json_content[..100]
                        } else {
                            json_content
                        }
                    );
                    Ok(BrowserDiscoveryResult {
                        success: true,
                        source_url: url.to_string(),
                        page_title: None,
                        extracted_html: Some(content_str.to_string()),
                        discovered_endpoints: vec![],
                        found_openapi_urls: vec![],
                        spec_url: None,
                        error: None,
                    })
                }
            }
            Err(e) => {
                eprintln!("[BrowserDiscovery] Evaluation failed: {}", e);
                Ok(BrowserDiscoveryResult {
                    success: false,
                    source_url: url.to_string(),
                    page_title: None,
                    extracted_html: None,
                    discovered_endpoints: vec![],
                    found_openapi_urls: vec![],
                    spec_url: None,
                    error: Some(format!("Failed to evaluate page: {}", e)),
                })
            }
        }
    }

    /// Extract and analyze with LLM for deeper API understanding
    pub async fn extract_with_llm_analysis(
        &self,
        url: &str,
        llm_service: &crate::discovery::llm_discovery::LlmDiscoveryService,
    ) -> RuntimeResult<BrowserDiscoveryResult> {
        let result = self.extract_from_url(url).await?;

        // If we got HTML content, use LLM to analyze it
        if let Some(ref html) = result.extracted_html {
            if html.len() > 100 {
                match llm_service
                    .search_external_apis("api analysis", Some(url))
                    .await
                {
                    Ok(apis) => {
                        for api in apis {
                            eprintln!(
                                "[BrowserDiscovery] LLM found API: {} - {}",
                                api.name, api.description
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[BrowserDiscovery] LLM analysis failed: {}", e);
                    }
                }
            }
        }

        Ok(result)
    }
}

impl Default for BrowserDiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}
