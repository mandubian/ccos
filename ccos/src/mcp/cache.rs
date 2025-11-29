//! MCP Discovery Cache
//!
//! Provides caching layer for discovered MCP tools to avoid redundant
//! queries to MCP servers.
//!
//! Supports both in-memory and file-based caching with TTL support.

use crate::mcp::types::{MCPServerConfig, DiscoveredMCPTool};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Cache entry with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    tools: Vec<DiscoveredMCPTool>,
    cached_at: u64,
}

/// Cache for discovered MCP tools
pub struct MCPCache {
    /// In-memory cache: server_url → tools
    memory_cache: Mutex<HashMap<String, Vec<DiscoveredMCPTool>>>,
    /// Optional file cache directory
    cache_dir: Option<PathBuf>,
    /// Cache TTL in seconds (default: 24 hours)
    ttl_seconds: u64,
}

impl MCPCache {
    /// Create a new cache with default TTL of 24 hours
    pub fn new() -> Self {
        Self {
            memory_cache: Mutex::new(HashMap::new()),
            cache_dir: None,
            ttl_seconds: 86400, // 24 hours
        }
    }

    /// Enable file-based caching
    pub fn with_cache_dir(mut self, dir: PathBuf) -> Self {
        // Create directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("Warning: Failed to create cache directory: {}", e);
        }
        self.cache_dir = Some(dir);
        self
    }

    /// Set cache TTL in seconds
    pub fn with_ttl(mut self, ttl_seconds: u64) -> Self {
        self.ttl_seconds = ttl_seconds;
        self
    }

    /// Get cached tools for a server
    /// Checks memory cache first, then file cache if available
    pub fn get(&self, server_config: &MCPServerConfig) -> Option<Vec<DiscoveredMCPTool>> {
        // Check memory cache first
        {
            let cache = self.memory_cache.lock().unwrap();
            if let Some(tools) = cache.get(&server_config.endpoint) {
                return Some(tools.clone());
            }
        }

        // Check file cache if enabled
        if let Some(ref cache_dir) = self.cache_dir {
            if let Some(tools) = self.load_from_file_cache(cache_dir, server_config) {
                // Store in memory cache for faster access
                let mut cache = self.memory_cache.lock().unwrap();
                cache.insert(server_config.endpoint.clone(), tools.clone());
                return Some(tools);
            }
        }

        None
    }

    /// Store tools in cache (both memory and file if enabled)
    pub fn store(&self, server_config: &MCPServerConfig, tools: Vec<DiscoveredMCPTool>) {
        // Store in memory cache
        {
            let mut cache = self.memory_cache.lock().unwrap();
            cache.insert(server_config.endpoint.clone(), tools.clone());
        }

        // Store in file cache if enabled
        if let Some(ref cache_dir) = self.cache_dir {
            self.save_to_file_cache(cache_dir, server_config, &tools);
        }
    }

    /// Load tools from file cache
    fn load_from_file_cache(
        &self,
        cache_dir: &Path,
        server_config: &MCPServerConfig,
    ) -> Option<Vec<DiscoveredMCPTool>> {
        let cache_file = cache_dir.join(format!("{}_tools.json", sanitize_filename(&server_config.name)));

        if !cache_file.exists() {
            return None;
        }

        let content = match fs::read_to_string(&cache_file) {
            Ok(c) => c,
            Err(_) => return None,
        };

        let entry: CacheEntry = match serde_json::from_str(&content) {
            Ok(e) => e,
            Err(_) => return None,
        };

        // Check if cache is expired
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if now.saturating_sub(entry.cached_at) > self.ttl_seconds {
            // Cache expired, delete file
            let _ = fs::remove_file(&cache_file);
            return None;
        }

        Some(entry.tools)
    }

    /// Save tools to file cache
    fn save_to_file_cache(
        &self,
        cache_dir: &Path,
        server_config: &MCPServerConfig,
        tools: &[DiscoveredMCPTool],
    ) {
        let cache_file = cache_dir.join(format!("{}_tools.json", sanitize_filename(&server_config.name)));

        let entry = CacheEntry {
            tools: tools.to_vec(),
            cached_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        match serde_json::to_string_pretty(&entry) {
            Ok(json) => {
                if let Err(e) = fs::write(&cache_file, json) {
                    eprintln!("Warning: Failed to write cache file: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to serialize cache: {}", e);
            }
        }
    }

    /// Clear all caches (memory and file)
    pub fn clear(&self) -> RuntimeResult<()> {
        // Clear memory cache
        {
            let mut cache = self.memory_cache.lock().unwrap();
            cache.clear();
        }

        // Clear file cache if enabled
        if let Some(ref cache_dir) = self.cache_dir {
            if cache_dir.exists() {
                for entry in fs::read_dir(cache_dir).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to read cache directory: {}", e))
                })? {
                    let entry = entry.map_err(|e| {
                        RuntimeError::Generic(format!("Failed to read cache entry: {}", e))
                    })?;
                    if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        Ok(())
    }
}

/// Sanitize filename to be filesystem-safe
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' {
            c
        } else {
            '_'
        })
        .collect()
}

impl Default for MCPCache {
    fn default() -> Self {
        Self::new()
    }
}

