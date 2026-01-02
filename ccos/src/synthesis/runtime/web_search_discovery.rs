use rtfs::runtime::error::RuntimeResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Web search result for API discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    /// URL of the result
    pub url: String,
    /// Title of the result
    pub title: String,
    /// Snippet/description
    pub snippet: String,
    /// Relevance score (0.0 to 1.0)
    pub relevance_score: f64,
    /// Result type (openapi_spec, graphql_schema, github_repo, api_docs, etc.)
    pub result_type: String,
}

impl PartialEq for WebSearchResult {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
    }
}

impl Eq for WebSearchResult {}

impl PartialOrd for WebSearchResult {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        other.relevance_score.partial_cmp(&self.relevance_score)
    }
}

impl Ord for WebSearchResult {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Rate limiter for web requests
#[derive(Debug, Clone)]
struct RateLimiter {
    requests_per_minute: u32,
    last_request: Instant,
    request_count: u32,
    window_start: Instant,
}

impl RateLimiter {
    fn new(requests_per_minute: u32) -> Self {
        Self {
            requests_per_minute,
            last_request: Instant::now(),
            request_count: 0,
            window_start: Instant::now(),
        }
    }

    async fn wait_if_needed(&mut self) {
        let now = Instant::now();

        // Reset counter if window has passed
        if now.duration_since(self.window_start) >= Duration::from_secs(60) {
            self.request_count = 0;
            self.window_start = now;
        }

        // Check if we need to wait
        if self.request_count >= self.requests_per_minute {
            let wait_time = Duration::from_secs(60) - now.duration_since(self.window_start);
            if wait_time > Duration::from_secs(0) {
                eprintln!("⏳ Rate limiting: waiting {}ms", wait_time.as_millis());
                sleep(wait_time).await;
                self.request_count = 0;
                self.window_start = Instant::now();
            }
        }

        self.request_count += 1;
        self.last_request = Instant::now();
    }
}

/// Persistent file-based cache for web search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchCacheEntry {
    /// The cached results
    pub results: Vec<WebSearchResult>,
    /// When this entry was created
    pub created_at: i64, // Unix timestamp
    /// TTL in seconds
    pub ttl_secs: u64,
}

impl SearchCacheEntry {
    /// Check if this cache entry is still valid
    pub fn is_valid(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        (now - self.created_at) < self.ttl_secs as i64
    }
}

/// Search cache with file persistence
#[derive(Debug)]
pub struct SearchCache {
    /// Cache file path
    cache_file: std::path::PathBuf,
    /// In-memory cache
    cache: HashMap<String, SearchCacheEntry>,
    /// Default TTL in seconds (1 day = 86400)
    default_ttl: u64,
}

impl SearchCache {
    /// Create a new search cache with default settings
    pub fn new() -> Self {
        // Get cache directory (use $HOME/.cache/ccos or current dir)
        let cache_dir = std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".cache").join("ccos"))
            .unwrap_or_else(|_| std::path::PathBuf::from(".ccos_cache"));

        // Create cache directory if it doesn't exist
        let _ = std::fs::create_dir_all(&cache_dir);

        let cache_file = cache_dir.join("web_search_cache.json");
        let cache = Self::load_from_file(&cache_file);

        Self {
            cache_file,
            cache,
            default_ttl: 86400, // 1 day
        }
    }

    /// Create with custom cache path
    pub fn with_path(path: &std::path::Path) -> Self {
        let cache = Self::load_from_file(path);
        Self {
            cache_file: path.to_path_buf(),
            cache,
            default_ttl: 86400,
        }
    }

    /// Set custom TTL in seconds
    pub fn with_ttl(mut self, ttl_secs: u64) -> Self {
        self.default_ttl = ttl_secs;
        self
    }

    /// Load cache from file
    fn load_from_file(path: &std::path::Path) -> HashMap<String, SearchCacheEntry> {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(cache) = serde_json::from_str(&content) {
                return cache;
            }
        }
        HashMap::new()
    }

    /// Save cache to file
    fn save_to_file(&self) {
        if let Ok(content) = serde_json::to_string_pretty(&self.cache) {
            let _ = std::fs::write(&self.cache_file, content);
        }
    }

    /// Get cached results for a query
    pub fn get(&self, query: &str) -> Option<Vec<WebSearchResult>> {
        let key = Self::normalize_query(query);
        if let Some(entry) = self.cache.get(&key) {
            if entry.is_valid() {
                return Some(entry.results.clone());
            }
        }
        None
    }

    /// Cache results for a query
    pub fn set(&mut self, query: &str, results: Vec<WebSearchResult>) {
        let key = Self::normalize_query(query);
        let entry = SearchCacheEntry {
            results,
            created_at: chrono::Utc::now().timestamp(),
            ttl_secs: self.default_ttl,
        };
        self.cache.insert(key, entry);
        self.save_to_file();
    }

    /// Normalize query for cache key (lowercase, trimmed)
    fn normalize_query(query: &str) -> String {
        query.trim().to_lowercase()
    }

    /// Clear the entire cache
    pub fn clear(&mut self) {
        self.cache.clear();
        self.save_to_file();
        eprintln!("🗑️  Search cache cleared");
    }

    /// Remove expired entries
    pub fn cleanup_expired(&mut self) {
        let before_count = self.cache.len();
        self.cache.retain(|_, entry| entry.is_valid());
        let removed = before_count - self.cache.len();
        if removed > 0 {
            self.save_to_file();
            eprintln!("🧹 Cleaned up {} expired cache entries", removed);
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let total = self.cache.len();
        let valid = self.cache.values().filter(|e| e.is_valid()).count();
        (valid, total)
    }

    /// Get cache file path
    pub fn cache_path(&self) -> &std::path::Path {
        &self.cache_file
    }
}

impl Default for SearchCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Web Search Discovery Provider
pub struct WebSearchDiscovery {
    /// Web search engine provider (google, duckduckgo, bing, scraping)
    pub provider: String,
    /// Mock mode for testing
    mock_mode: bool,
    /// Rate limiter for web requests
    rate_limiter: RateLimiter,
    /// API keys for different search providers
    api_keys: HashMap<String, String>,
    /// User agent for web scraping
    user_agent: String,
    /// Persistent search cache
    cache: Option<SearchCache>,
}

impl WebSearchDiscovery {
    /// Helper to convert reqwest errors to RuntimeError
    fn handle_reqwest_error(e: reqwest::Error) -> rtfs::runtime::error::RuntimeError {
        rtfs::runtime::error::RuntimeError::Generic(format!("HTTP request failed: {}", e))
    }

    /// Create a new web search discovery provider
    pub fn new(provider: String) -> Self {
        let mut api_keys = HashMap::new();

        // Load API keys from environment variables
        if let Ok(google_key) = std::env::var("GOOGLE_SEARCH_API_KEY") {
            api_keys.insert("google".to_string(), google_key);
        }
        if let Ok(google_cx) = std::env::var("GOOGLE_SEARCH_CX") {
            api_keys.insert("google_cx".to_string(), google_cx);
        }
        if let Ok(bing_key) = std::env::var("BING_SEARCH_API_KEY") {
            api_keys.insert("bing".to_string(), bing_key);
        }

        Self {
            provider,
            mock_mode: false,
            rate_limiter: RateLimiter::new(10), // Conservative rate limit
            api_keys,
            user_agent: "CCOS-WebSearch/1.0 (Capability Discovery Bot)".to_string(),
            cache: Some(SearchCache::new()),
        }
    }

    /// Create in mock mode for testing
    pub fn mock() -> Self {
        Self {
            provider: "mock".to_string(),
            mock_mode: true,
            rate_limiter: RateLimiter::new(1000), // No rate limiting in mock mode
            api_keys: HashMap::new(),
            user_agent: "CCOS-WebSearch-Mock/1.0".to_string(),
            cache: None, // No caching in mock mode
        }
    }

    /// Create with custom rate limiting
    pub fn with_rate_limit(provider: String, requests_per_minute: u32) -> Self {
        let mut instance = Self::new(provider);
        instance.rate_limiter = RateLimiter::new(requests_per_minute);
        instance
    }

    /// Enable or disable caching
    pub fn with_cache(mut self, enabled: bool) -> Self {
        if enabled {
            if self.cache.is_none() {
                self.cache = Some(SearchCache::new());
            }
        } else {
            self.cache = None;
        }
        self
    }

    /// Clear the search cache
    pub fn clear_cache(&mut self) {
        if let Some(ref mut cache) = self.cache {
            cache.clear();
        } else {
            eprintln!("ℹ️  No cache to clear (caching disabled)");
        }
    }

    /// Get cache statistics: (valid_entries, total_entries)
    pub fn cache_stats(&self) -> Option<(usize, usize)> {
        self.cache.as_ref().map(|c| c.stats())
    }

    /// Get the cache file path
    pub fn cache_path(&self) -> Option<&std::path::Path> {
        self.cache.as_ref().map(|c| c.cache_path())
    }

    /// Cleanup expired cache entries
    pub fn cleanup_expired_cache(&mut self) {
        if let Some(ref mut cache) = self.cache {
            cache.cleanup_expired();
        }
    }

    /// Search for API specs and documentation
    ///
    /// Generates multiple search strategies:
    /// 1. General API searches (most useful for finding actual APIs)
    /// 2. MCP server searches (for direct tool integration)
    /// 3. OpenAPI/REST spec searches (for automatic client generation)
    pub async fn search_for_api_specs(
        &mut self,
        capability_name: &str,
    ) -> RuntimeResult<Vec<WebSearchResult>> {
        if self.mock_mode {
            return self.get_mock_results(capability_name);
        }

        // Build search queries with multiple strategies
        // Priority: General APIs > MCP Servers > OpenAPI Specs
        let queries = vec![
            // Strategy 1: General API searches (most likely to find useful results)
            format!("{} API", capability_name),
            format!("{} free API service", capability_name),
            // Strategy 2: MCP server searches (for direct integration)
            format!("{} MCP server", capability_name),
            // Strategy 3: Spec-based searches (for automatic generation)
            format!("{} REST API documentation", capability_name),
            format!("{} OpenAPI specification", capability_name),
        ];

        let mut all_results = Vec::new();

        for query in queries {
            match self.perform_search(&query).await {
                Ok(mut results) => all_results.append(&mut results),
                Err(e) => {
                    eprintln!("Search error for query '{}': {}", query, e);
                }
            }
        }

        // Deduplicate and sort by relevance
        all_results.sort();
        all_results.dedup();
        all_results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(all_results.into_iter().take(10).collect())
    }

    /// Perform actual web search with multiple fallback strategies
    async fn perform_search(&mut self, query: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        // Check cache first
        if let Some(ref cache) = self.cache {
            if let Some(cached_results) = cache.get(query) {
                eprintln!("📦 CACHE HIT: Using cached results for '{}'", query);
                return Ok(cached_results);
            }
        }

        eprintln!(
            "🔍 WEB SEARCH: Searching for '{}'{}",
            query,
            if self.cache.is_some() {
                " (will cache)"
            } else {
                ""
            }
        );

        // Helper to cache results before returning
        let cache_results =
            |s: &mut Self, q: &str, results: Vec<WebSearchResult>| -> Vec<WebSearchResult> {
                if let Some(ref mut cache) = s.cache {
                    cache.set(q, results.clone());
                    eprintln!("💾 Cached {} results for '{}'", results.len(), q);
                }
                results
            };

        // Try different search methods in order of preference (free first)
        // DuckDuckGo first (free, no API key)
        match self.search_duckduckgo_api(query).await {
            Ok(results) if !results.is_empty() => {
                eprintln!("✅ Found {} results via DuckDuckGo", results.len());
                let cached = cache_results(self, query, results);
                return Ok(cached);
            }
            Ok(_) => {
                // Silently continue to next search method
            }
            Err(e) => {
                // Only log if it's not a timeout (timeouts are expected and we'll try other methods)
                if !e.to_string().contains("timeout") && !e.to_string().contains("timed out") {
                    eprintln!("⚠️ DuckDuckGo search failed: {}", e);
                }
            }
        }

        // Google Custom Search (free tier: 100 queries/day)
        match self.search_google_api(query).await {
            Ok(results) if !results.is_empty() => {
                eprintln!("✅ Found {} results via Google", results.len());
                let cached = cache_results(self, query, results);
                return Ok(cached);
            }
            Ok(_) => eprintln!("⚠️ No results from Google"),
            Err(e) => eprintln!("❌ Google failed: {}", e),
        }

        // Bing Search (free tier: 1000 queries/month)
        match self.search_bing_api(query).await {
            Ok(results) if !results.is_empty() => {
                eprintln!("✅ Found {} results via Bing", results.len());
                let cached = cache_results(self, query, results);
                return Ok(cached);
            }
            Ok(_) => eprintln!("⚠️ No results from Bing"),
            Err(e) => eprintln!("❌ Bing failed: {}", e),
        }

        // Web scraping fallback (free but slower)
        match self.search_via_scraping(query).await {
            Ok(results) if !results.is_empty() => {
                eprintln!("✅ Found {} results via scraping", results.len());
                let cached = cache_results(self, query, results);
                return Ok(cached);
            }
            Ok(_) => eprintln!("⚠️ No results from scraping"),
            Err(e) => eprintln!("❌ Scraping failed: {}", e),
        }

        eprintln!("❌ All search methods failed for query: {}", query);
        Ok(Vec::new())
    }

    /// Search using DuckDuckGo Instant Answer API (free, no API key required)
    async fn search_duckduckgo_api(&mut self, query: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        self.rate_limiter.wait_if_needed().await;

        let client = reqwest::Client::new();
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencoding::encode(query)
        );

        let response = client
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .timeout(Duration::from_secs(5)) // Reduced timeout - DuckDuckGo can be slow, we have fallbacks
            .send()
            .await
            .map_err(Self::handle_reqwest_error)?;

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "DuckDuckGo API returned status: {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response.json().await.map_err(Self::handle_reqwest_error)?;
        let mut results = Vec::new();

        // Parse DuckDuckGo response
        if let Some(abstract_text) = json.get("AbstractText").and_then(|v| v.as_str()) {
            if !abstract_text.is_empty() {
                results.push(WebSearchResult {
                    url: json
                        .get("AbstractURL")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    title: json
                        .get("Heading")
                        .and_then(|v| v.as_str())
                        .unwrap_or("DuckDuckGo Result")
                        .to_string(),
                    snippet: abstract_text.to_string(),
                    relevance_score: 0.8,
                    result_type: "api_docs".to_string(),
                });
            }
        }

        // Parse related topics
        if let Some(related_topics) = json.get("RelatedTopics").and_then(|v| v.as_array()) {
            for topic in related_topics.iter().take(3) {
                if let Some(text) = topic.get("Text").and_then(|v| v.as_str()) {
                    if let Some(url) = topic.get("FirstURL").and_then(|v| v.as_str()) {
                        results.push(WebSearchResult {
                            url: url.to_string(),
                            title: text.chars().take(100).collect(),
                            snippet: text.to_string(),
                            relevance_score: 0.6,
                            result_type: "related".to_string(),
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// Search using Google Custom Search API (free tier: 100 queries/day)
    async fn search_google_api(&mut self, query: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        let api_key = self.api_keys.get("google");
        let cx = self.api_keys.get("google_cx");

        if api_key.is_none() || cx.is_none() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Google API key or CX not configured".to_string(),
            ));
        }

        self.rate_limiter.wait_if_needed().await;

        let client = reqwest::Client::new();
        let url = format!(
            "https://www.googleapis.com/customsearch/v1?key={}&cx={}&q={}&num=10",
            api_key.unwrap(),
            cx.unwrap(),
            urlencoding::encode(query)
        );

        let response = client
            .get(&url)
            .header("User-Agent", &self.user_agent)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(Self::handle_reqwest_error)?;

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "Google API returned status: {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response.json().await.map_err(Self::handle_reqwest_error)?;
        let mut results = Vec::new();

        if let Some(items) = json.get("items").and_then(|v| v.as_array()) {
            for item in items {
                if let (Some(title), Some(link), Some(snippet)) = (
                    item.get("title").and_then(|v| v.as_str()),
                    item.get("link").and_then(|v| v.as_str()),
                    item.get("snippet").and_then(|v| v.as_str()),
                ) {
                    let result_type = if link.contains("openapi") || link.contains("swagger") {
                        "openapi_spec"
                    } else if link.contains("github.com") {
                        "github_repo"
                    } else if link.contains("docs") || link.contains("api") {
                        "api_docs"
                    } else {
                        "general"
                    };

                    results.push(WebSearchResult {
                        url: link.to_string(),
                        title: title.to_string(),
                        snippet: snippet.to_string(),
                        relevance_score: self.score_relevance(&WebSearchResult {
                            url: link.to_string(),
                            title: title.to_string(),
                            snippet: snippet.to_string(),
                            relevance_score: 0.5,
                            result_type: result_type.to_string(),
                        }),
                        result_type: result_type.to_string(),
                    });
                }
            }
        }

        Ok(results)
    }

    /// Search using Bing Search API (free tier: 1000 queries/month)
    async fn search_bing_api(&mut self, query: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        let api_key = self.api_keys.get("bing");

        if api_key.is_none() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(
                "Bing API key not configured".to_string(),
            ));
        }

        self.rate_limiter.wait_if_needed().await;

        let client = reqwest::Client::new();
        let url = format!(
            "https://api.bing.microsoft.com/v7.0/search?q={}&count=10",
            urlencoding::encode(query)
        );

        let response = client
            .get(&url)
            .header("Ocp-Apim-Subscription-Key", api_key.unwrap())
            .header("User-Agent", &self.user_agent)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(Self::handle_reqwest_error)?;

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "Bing API returned status: {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response.json().await.map_err(Self::handle_reqwest_error)?;
        let mut results = Vec::new();

        if let Some(web_pages) = json
            .get("webPages")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_array())
        {
            for page in web_pages {
                if let (Some(name), Some(url), Some(snippet)) = (
                    page.get("name").and_then(|v| v.as_str()),
                    page.get("url").and_then(|v| v.as_str()),
                    page.get("snippet").and_then(|v| v.as_str()),
                ) {
                    let result_type = if url.contains("openapi") || url.contains("swagger") {
                        "openapi_spec"
                    } else if url.contains("github.com") {
                        "github_repo"
                    } else if url.contains("docs") || url.contains("api") {
                        "api_docs"
                    } else {
                        "general"
                    };

                    results.push(WebSearchResult {
                        url: url.to_string(),
                        title: name.to_string(),
                        snippet: snippet.to_string(),
                        relevance_score: self.score_relevance(&WebSearchResult {
                            url: url.to_string(),
                            title: name.to_string(),
                            snippet: snippet.to_string(),
                            relevance_score: 0.5,
                            result_type: result_type.to_string(),
                        }),
                        result_type: result_type.to_string(),
                    });
                }
            }
        }

        Ok(results)
    }

    /// Fallback web scraping search (free but slower, with rate limiting)
    async fn search_via_scraping(&mut self, query: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        self.rate_limiter.wait_if_needed().await;

        // Try multiple search engines via scraping
        let search_engines = vec![
            ("https://html.duckduckgo.com/html/?q=", "DuckDuckGo"),
            ("https://www.startpage.com/sp/search?query=", "Startpage"),
        ];

        for (base_url, engine_name) in search_engines {
            match self
                .scrape_search_engine(&format!("{}{}", base_url, urlencoding::encode(query)))
                .await
            {
                Ok(results) if !results.is_empty() => {
                    eprintln!(
                        "✅ Scraping {} found {} results",
                        engine_name,
                        results.len()
                    );
                    return Ok(results);
                }
                Ok(_) => {
                    eprintln!("⚠️ No results from scraping {}", engine_name);
                }
                Err(e) => {
                    eprintln!("❌ Scraping {} failed: {}", engine_name, e);
                }
            }
        }

        Ok(Vec::new())
    }

    /// Scrape search results from a search engine
    async fn scrape_search_engine(&self, url: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(&self.user_agent)
            .build()
            .map_err(Self::handle_reqwest_error)?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(Self::handle_reqwest_error)?;

        if !response.status().is_success() {
            return Err(rtfs::runtime::error::RuntimeError::Generic(format!(
                "Scraping failed with status: {}",
                response.status()
            )));
        }

        let html = response.text().await.map_err(Self::handle_reqwest_error)?;
        let mut results = Vec::new();

        // Simple HTML parsing for search results
        // This is a basic implementation - in production you'd want a proper HTML parser
        let lines: Vec<&str> = html.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            if line.contains("href=\"http") && line.contains("class=\"result") {
                // Extract URL and title from search result
                if let Some(url_start) = line.find("href=\"") {
                    let url_part = &line[url_start + 6..];
                    if let Some(url_end) = url_part.find("\"") {
                        let result_url = &url_part[..url_end];

                        // Look for title in nearby lines
                        let title = lines
                            .get(i + 1)
                            .or_else(|| lines.get(i + 2))
                            .unwrap_or(&"Search Result")
                            .trim();

                        if !result_url.is_empty() && !title.is_empty() {
                            let result_type = if result_url.contains("openapi")
                                || result_url.contains("swagger")
                            {
                                "openapi_spec"
                            } else if result_url.contains("github.com") {
                                "github_repo"
                            } else if result_url.contains("docs") || result_url.contains("api") {
                                "api_docs"
                            } else {
                                "general"
                            };

                            results.push(WebSearchResult {
                                url: result_url.to_string(),
                                title: title.to_string(),
                                snippet: "Scraped search result".to_string(),
                                relevance_score: self.score_relevance(&WebSearchResult {
                                    url: result_url.to_string(),
                                    title: title.to_string(),
                                    snippet: "Scraped search result".to_string(),
                                    relevance_score: 0.5,
                                    result_type: result_type.to_string(),
                                }),
                                result_type: result_type.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(results.into_iter().take(5).collect()) // Limit scraped results
    }

    /// Score result relevance based on URL and content patterns
    fn score_relevance(&self, result: &WebSearchResult) -> f64 {
        let mut score = 0.5_f64; // Base score

        // Boost for official sources
        if result.url.contains("github.com") {
            score += 0.3;
            if result.url.contains("openapi") || result.url.contains("specification") {
                score += 0.1;
            }
        }

        if result.url.contains("openapis.org") {
            score += 0.25;
        }

        if result.url.contains("/api") || result.url.contains("-api") {
            score += 0.15;
        }

        // Penalize for low-quality sources
        if result.url.contains("stackoverflow") {
            score -= 0.1;
        }

        if result.url.contains("medium.com") {
            score -= 0.05;
        }

        // Boost for YAML/JSON file indicators
        if result.title.contains(".yaml") || result.title.contains(".json") {
            score += 0.2;
        }

        score.min(1.0).max(0.0)
    }

    /// Get mock results for testing
    fn get_mock_results(&self, capability_name: &str) -> RuntimeResult<Vec<WebSearchResult>> {
        let results = match capability_name.to_lowercase().as_str() {
            "github" => vec![
                WebSearchResult {
                    url: "https://github.com/octocat/Hello-World".to_string(),
                    title: "GitHub API v3 OpenAPI Specification".to_string(),
                    snippet: "The official GitHub API specification in OpenAPI 3.0 format"
                        .to_string(),
                    relevance_score: 0.95,
                    result_type: "openapi_spec".to_string(),
                },
                WebSearchResult {
                    url: "https://docs.github.com/en/rest".to_string(),
                    title: "GitHub REST API Documentation".to_string(),
                    snippet:
                        "Complete reference for GitHub's REST API endpoints and authentication"
                            .to_string(),
                    relevance_score: 0.90,
                    result_type: "api_docs".to_string(),
                },
                WebSearchResult {
                    url: "https://github.com/github/rest-api-description".to_string(),
                    title: "GitHub REST API Description Repository".to_string(),
                    snippet: "Official OpenAPI specification for GitHub REST API".to_string(),
                    relevance_score: 0.92,
                    result_type: "github_repo".to_string(),
                },
            ],
            "stripe" => vec![
                WebSearchResult {
                    url: "https://github.com/stripe/openapi".to_string(),
                    title: "Stripe OpenAPI Specification".to_string(),
                    snippet: "Official Stripe OpenAPI 3.0 specification".to_string(),
                    relevance_score: 0.98,
                    result_type: "openapi_spec".to_string(),
                },
                WebSearchResult {
                    url: "https://stripe.com/docs/api".to_string(),
                    title: "Stripe API Reference".to_string(),
                    snippet: "Complete Stripe API documentation with examples".to_string(),
                    relevance_score: 0.85,
                    result_type: "api_docs".to_string(),
                },
            ],
            "openai" => vec![
                WebSearchResult {
                    url: "https://github.com/openai/openai-openapi".to_string(),
                    title: "OpenAI OpenAPI Specification".to_string(),
                    snippet: "Official OpenAPI spec for OpenAI API".to_string(),
                    relevance_score: 0.97,
                    result_type: "openapi_spec".to_string(),
                },
                WebSearchResult {
                    url: "https://platform.openai.com/docs/api-reference".to_string(),
                    title: "OpenAI API Reference".to_string(),
                    snippet: "Complete OpenAI API documentation".to_string(),
                    relevance_score: 0.88,
                    result_type: "api_docs".to_string(),
                },
            ],
            _ => vec![
                WebSearchResult {
                    url: format!("https://openapis.org/spec/{}", capability_name),
                    title: format!("{} OpenAPI Specification", capability_name),
                    snippet: format!("OpenAPI specification for {}", capability_name),
                    relevance_score: 0.7,
                    result_type: "openapi_spec".to_string(),
                },
                WebSearchResult {
                    url: format!("https://github.com/search?q={}", capability_name),
                    title: format!("GitHub search results for {}", capability_name),
                    snippet: format!("Find {} API specifications on GitHub", capability_name),
                    relevance_score: 0.6,
                    result_type: "github_repo".to_string(),
                },
            ],
        };

        Ok(results)
    }

    /// Format results for display
    pub fn format_results_for_display(results: &[WebSearchResult]) -> String {
        let mut output = String::new();
        output.push_str("📄 Found API candidates:\n");

        for (i, result) in results.iter().take(10).enumerate() {
            let stars = match (result.relevance_score * 3.0) as u32 {
                3 => "⭐⭐⭐",
                2 => "⭐⭐",
                1 => "⭐",
                _ => "",
            };

            output.push_str(&format!("  {}. {} {}\n", i + 1, result.url, stars));
            output.push_str(&format!("     Type: {}\n", result.result_type));
            output.push_str(&format!("     {}\n", result.snippet));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_search_discovery_mock() {
        let discovery = WebSearchDiscovery::mock();
        assert_eq!(discovery.mock_mode, true);
    }

    #[tokio::test]
    async fn test_search_for_github_api() {
        let mut discovery = WebSearchDiscovery::mock();
        let results = discovery.search_for_api_specs("github").await.unwrap();

        assert!(!results.is_empty());
        assert!(results[0].url.contains("github.com"));
        assert!(results[0].relevance_score > 0.9);
    }

    #[tokio::test]
    async fn test_search_for_stripe_api() {
        let mut discovery = WebSearchDiscovery::mock();
        let results = discovery.search_for_api_specs("stripe").await.unwrap();

        assert!(!results.is_empty());
        assert!(results[0].relevance_score > 0.95);
    }

    #[test]
    fn test_format_results_for_display() {
        let results = vec![WebSearchResult {
            url: "https://github.com/example/openapi".to_string(),
            title: "Example API Spec".to_string(),
            snippet: "An example API specification".to_string(),
            relevance_score: 0.95,
            result_type: "openapi_spec".to_string(),
        }];

        let formatted = WebSearchDiscovery::format_results_for_display(&results);
        assert!(formatted.contains("github.com"));
        assert!(formatted.contains("openapi_spec"));
    }

    #[test]
    fn test_relevance_scoring() {
        let discovery = WebSearchDiscovery::mock();

        let github_result = WebSearchResult {
            url: "https://github.com/example/openapi.yaml".to_string(),
            title: "openapi.yaml".to_string(),
            snippet: "API spec".to_string(),
            relevance_score: 0.5,
            result_type: "openapi_spec".to_string(),
        };

        let scored = discovery.score_relevance(&github_result);
        assert!(scored > 0.8); // Should have high score for github + openapi + .yaml

        let low_result = WebSearchResult {
            url: "https://stackoverflow.com/questions/123".to_string(),
            title: "SO Question".to_string(),
            snippet: "Question about API".to_string(),
            relevance_score: 0.5,
            result_type: "qa".to_string(),
        };

        let scored_low = discovery.score_relevance(&low_result);
        assert!(scored_low < 0.5); // Should be penalized
    }
}
