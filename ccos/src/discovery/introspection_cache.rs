//! Introspection cache for MCP and OpenAPI discovery results
//!
//! This module provides a simple file-based cache for introspection results
//! to speed up subsequent discovery runs by avoiding repeated network calls.

use crate::synthesis::mcp_introspector::MCPIntrospectionResult;
use crate::synthesis::api_introspector::APIIntrospectionResult;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};

/// Cache entry with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry<T> {
    data: T,
    cached_at: u64,
}

/// Introspection cache for storing and retrieving discovery results
pub struct IntrospectionCache {
    cache_dir: PathBuf,
    ttl: Duration,
}

impl IntrospectionCache {
    /// Create a new introspection cache with default TTL of 24 hours
    pub fn new(cache_dir: impl AsRef<Path>) -> RuntimeResult<Self> {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        
        // Create cache directory if it doesn't exist
        fs::create_dir_all(&cache_dir)
            .map_err(|e| RuntimeError::Generic(format!("Failed to create cache directory: {}", e)))?;
        
        Ok(Self {
            cache_dir,
            ttl: Duration::from_secs(24 * 60 * 60), // 24 hours
        })
    }
    
    /// Create a cache with a custom TTL
    pub fn with_ttl(cache_dir: impl AsRef<Path>, ttl: Duration) -> RuntimeResult<Self> {
        let cache_dir = cache_dir.as_ref().to_path_buf();
        
        fs::create_dir_all(&cache_dir)
            .map_err(|e| RuntimeError::Generic(format!("Failed to create cache directory: {}", e)))?;
        
        Ok(Self {
            cache_dir,
            ttl,
        })
    }
    
    /// Get cached MCP introspection data for a URL
    pub fn get_mcp(&self, url: &str) -> RuntimeResult<Option<MCPIntrospectionResult>> {
        let key = self.url_to_key(url);
        let cache_file = self.cache_dir.join(key);
        
        if !cache_file.exists() {
            return Ok(None);
        }
        
        let content = fs::read_to_string(&cache_file)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read cache file: {}", e)))?;
        
        let entry: CacheEntry<MCPIntrospectionResult> = serde_json::from_str(&content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to deserialize cache entry: {}", e)))?;
        
        // Check if entry is expired
        if self.is_expired(&entry) {
            eprintln!("  ⏰ Cache entry expired for: {}", url);
            // Delete expired cache file
            let _ = fs::remove_file(&cache_file);
            return Ok(None);
        }
        
        eprintln!("  📦 Using cached MCP introspection result for: {}", url);
        Ok(Some(entry.data))
    }
    
    /// Store MCP introspection data for a URL
    pub fn put_mcp(&self, url: &str, data: &MCPIntrospectionResult) -> RuntimeResult<()> {
        let key = self.url_to_key(url);
        let cache_file = self.cache_dir.join(key);
        
        let entry = CacheEntry {
            data: data.clone(),
            cached_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        let json = serde_json::to_string_pretty(&entry)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize cache entry: {}", e)))?;
        
        fs::write(&cache_file, json)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write cache file: {}", e)))?;
        
        eprintln!("  💾 Cached MCP introspection result for: {}", url);
        Ok(())
    }
    
    /// Get cached OpenAPI introspection data for a URL
    pub fn get_openapi(&self, url: &str) -> RuntimeResult<Option<APIIntrospectionResult>> {
        let key = self.url_to_key(url);
        let cache_file = self.cache_dir.join(key);
        
        if !cache_file.exists() {
            return Ok(None);
        }
        
        let content = fs::read_to_string(&cache_file)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read cache file: {}", e)))?;
        
        let entry: CacheEntry<APIIntrospectionResult> = serde_json::from_str(&content)
            .map_err(|e| RuntimeError::Generic(format!("Failed to deserialize cache entry: {}", e)))?;
        
        // Check if entry is expired
        if self.is_expired(&entry) {
            eprintln!("  ⏰ Cache entry expired for: {}", url);
            // Delete expired cache file
            let _ = fs::remove_file(&cache_file);
            return Ok(None);
        }
        
        eprintln!("  📦 Using cached OpenAPI introspection result for: {}", url);
        Ok(Some(entry.data))
    }
    
    /// Store OpenAPI introspection data for a URL
    pub fn put_openapi(&self, url: &str, data: &APIIntrospectionResult) -> RuntimeResult<()> {
        let key = self.url_to_key(url);
        let cache_file = self.cache_dir.join(key);
        
        let entry = CacheEntry {
            data: data.clone(),
            cached_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        let json = serde_json::to_string_pretty(&entry)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize cache entry: {}", e)))?;
        
        fs::write(&cache_file, json)
            .map_err(|e| RuntimeError::Generic(format!("Failed to write cache file: {}", e)))?;
        
        eprintln!("  💾 Cached OpenAPI introspection result for: {}", url);
        Ok(())
    }
    
    /// Clear all cached entries
    pub fn clear(&self) -> RuntimeResult<()> {
        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir)
                .map_err(|e| RuntimeError::Generic(format!("Failed to read cache directory: {}", e)))?
            {
                let entry = entry
                    .map_err(|e| RuntimeError::Generic(format!("Failed to read cache entry: {}", e)))?;
                let path = entry.path();
                if path.is_file() {
                    fs::remove_file(&path)
                        .map_err(|e| RuntimeError::Generic(format!("Failed to remove cache file: {}", e)))?;
                }
            }
        }
        Ok(())
    }
    
    /// Get cache statistics
    pub fn stats(&self) -> RuntimeResult<CacheStats> {
        let mut stats = CacheStats {
            total_entries: 0,
            expired_entries: 0,
            total_size: 0,
        };
        
        if !self.cache_dir.exists() {
            return Ok(stats);
        }
        
        for entry in fs::read_dir(&self.cache_dir)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read cache directory: {}", e)))?
        {
            let entry = entry
                .map_err(|e| RuntimeError::Generic(format!("Failed to read cache entry: {}", e)))?;
            let path = entry.path();
            
            if path.is_file() {
                stats.total_entries += 1;
                
                // Get file size
                if let Ok(metadata) = fs::metadata(&path) {
                    stats.total_size += metadata.len();
                }
                
                // Check if expired - try both MCP and OpenAPI types
                if let Ok(content) = fs::read_to_string(&path) {
                    // Try MCP type first
                    if let Ok(cache_entry) = serde_json::from_str::<CacheEntry<MCPIntrospectionResult>>(&content) {
                        if self.is_expired(&cache_entry) {
                            stats.expired_entries += 1;
                        }
                    }
                    // Try OpenAPI type if MCP failed
                    else if let Ok(cache_entry) = serde_json::from_str::<CacheEntry<APIIntrospectionResult>>(&content) {
                        if self.is_expired(&cache_entry) {
                            stats.expired_entries += 1;
                        }
                    }
                }
            }
        }
        
        Ok(stats)
    }
    
    /// Convert URL to a safe cache key (filename)
    fn url_to_key(&self, url: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        format!("{:x}.json", hasher.finish())
    }
    
    /// Check if a cache entry is expired
    fn is_expired<T>(&self, entry: &CacheEntry<T>) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let age = now - entry.cached_at;
        age > self.ttl.as_secs()
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub total_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_get_put_mcp() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IntrospectionCache::new(temp_dir.path()).unwrap();
        
        let url = "https://api.example.com";
        let data = MCPIntrospectionResult {
            server_url: url.to_string(),
            server_name: "test-server".to_string(),
            protocol_version: "1.0".to_string(),
            tools: vec![],
        };
        
        // Nothing in cache yet
        assert!(cache.get_mcp(url).unwrap().is_none());
        
        // Put data in cache
        cache.put_mcp(url, &data).unwrap();
        
        // Should be able to retrieve it
        let cached = cache.get_mcp(url).unwrap();
        assert!(cached.is_some());
        let cached_data = cached.unwrap();
        assert_eq!(cached_data.server_name, "test-server");
    }
    
    #[test]
    fn test_cache_expiration() {
        let temp_dir = TempDir::new().unwrap();
        // Create cache with very short TTL
        let cache = IntrospectionCache::with_ttl(temp_dir.path(), Duration::from_secs(1)).unwrap();
        
        let url = "https://api.example.com";
        let data = MCPIntrospectionResult {
            server_url: url.to_string(),
            server_name: "test-server".to_string(),
            protocol_version: "1.0".to_string(),
            tools: vec![],
        };
        
        cache.put_mcp(url, &data).unwrap();
        assert!(cache.get_mcp(url).unwrap().is_some());
        
        // Wait for expiration
        std::thread::sleep(Duration::from_secs(2));
        
        // Should be expired now
        assert!(cache.get_mcp(url).unwrap().is_none());
    }
    
    #[test]
    fn test_cache_clear() {
        let temp_dir = TempDir::new().unwrap();
        let cache = IntrospectionCache::new(temp_dir.path()).unwrap();
        
        let data1 = MCPIntrospectionResult {
            server_url: "url1".to_string(),
            server_name: "server1".to_string(),
            protocol_version: "1.0".to_string(),
            tools: vec![],
        };
        let data2 = MCPIntrospectionResult {
            server_url: "url2".to_string(),
            server_name: "server2".to_string(),
            protocol_version: "1.0".to_string(),
            tools: vec![],
        };
        
        cache.put_mcp("url1", &data1).unwrap();
        cache.put_mcp("url2", &data2).unwrap();
        
        assert_eq!(cache.stats().unwrap().total_entries, 2);
        
        cache.clear().unwrap();
        
        assert_eq!(cache.stats().unwrap().total_entries, 0);
    }
}
