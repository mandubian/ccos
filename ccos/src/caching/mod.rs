//! Multi-Layered Cognitive Caching System
//!
//! This module implements the advanced caching architecture for RTFS 2.0,
//! providing four layers of intelligent caching:
//!
//! - **L1 Delegation Cache**: `(Agent, Task) -> Plan` memoization
//! - **L2 Inference Cache**: Hybrid storage for LLM inference results
//! - **L3 Semantic Cache**: AI-driven vector search for semantic equivalence
//! - **L4 Content-Addressable RTFS**: Bytecode-level caching and reuse
//!
//! This transforms caching from a simple performance optimization into a core
//! feature of the system's intelligence.

pub mod l1_delegation;
pub mod l2_inference;
pub mod l3_semantic;
pub mod l4_content_addressable;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Core trait for all cache layers in the multi-layered architecture
pub trait CacheLayer<K, V>: Send + Sync + std::fmt::Debug
where
    K: Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// Retrieve a value from the cache
    fn get(&self, key: &K) -> Option<V>;

    /// Store a value in the cache
    fn put(&self, key: K, value: V) -> Result<(), CacheError>;

    /// Remove a value from the cache
    fn invalidate(&self, key: &K) -> Result<(), CacheError>;

    /// Get cache statistics
    fn stats(&self) -> CacheStats;

    /// Clear all entries from the cache
    fn clear(&self) -> Result<(), CacheError>;

    /// Get cache configuration
    fn config(&self) -> &CacheConfig;
}

/// Cache statistics for monitoring and observability
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub puts: u64,
    pub invalidations: u64,
    pub size: usize,
    pub capacity: usize,
    pub hit_rate: f64,
    pub last_updated: Instant,
}

impl CacheStats {
    pub fn new(capacity: usize) -> Self {
        Self {
            hits: 0,
            misses: 0,
            puts: 0,
            invalidations: 0,
            size: 0,
            capacity,
            hit_rate: 0.0,
            last_updated: Instant::now(),
        }
    }

    pub fn update_hit_rate(&mut self) {
        let total_requests = self.hits + self.misses;
        if total_requests > 0 {
            self.hit_rate = self.hits as f64 / total_requests as f64;
        }
        self.last_updated = Instant::now();
    }

    pub fn record_hit(&mut self) {
        self.hits += 1;
        self.update_hit_rate();
    }

    pub fn record_miss(&mut self) {
        self.misses += 1;
        self.update_hit_rate();
    }

    pub fn record_put(&mut self) {
        self.puts += 1;
    }

    pub fn record_invalidation(&mut self) {
        self.invalidations += 1;
    }
}

/// Cache configuration for different layers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub max_size: usize,
    pub ttl: Option<Duration>,
    pub eviction_policy: EvictionPolicy,
    pub persistence_enabled: bool,
    pub async_operations: bool,
    pub confidence_threshold: Option<f64>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size: 1000,
            ttl: None,
            eviction_policy: EvictionPolicy::LRU,
            persistence_enabled: false,
            async_operations: false,
            confidence_threshold: None,
        }
    }
}

/// Cache eviction policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvictionPolicy {
    LRU,    // Least Recently Used
    LFU,    // Least Frequently Used
    FIFO,   // First In, First Out
    Random, // Random eviction
    TTL,    // Time To Live based
}

/// Cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry<V> {
    pub value: V,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub access_count: u64,
}

impl<V> CacheEntry<V> {
    pub fn new(value: V) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            access_count: 1,
        }
    }

    pub fn access(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.last_accessed.elapsed() > ttl
    }
}

/// Cache errors
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Cache is full")]
    CacheFull,

    #[error("Key not found")]
    KeyNotFound,

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Async operation failed: {0}")]
    AsyncError(String),
}

/// Global cache manager for coordinating all layers
#[derive(Debug)]
pub struct CacheManager {
    l1_cache: Option<Arc<dyn CacheLayer<String, String>>>,
    l2_cache: Option<Arc<dyn CacheLayer<String, String>>>,
    l3_cache: Option<Arc<dyn CacheLayer<String, String>>>,
    l4_cache: Option<Arc<dyn CacheLayer<String, String>>>,
    stats: Arc<RwLock<HashMap<String, CacheStats>>>,
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            l1_cache: None,
            l2_cache: None,
            l3_cache: None,
            l4_cache: None,
            stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_l1_cache(mut self, cache: Arc<dyn CacheLayer<String, String>>) -> Self {
        self.l1_cache = Some(cache);
        self
    }

    pub fn with_l2_cache(mut self, cache: Arc<dyn CacheLayer<String, String>>) -> Self {
        self.l2_cache = Some(cache);
        self
    }

    pub fn with_l3_cache(mut self, cache: Arc<dyn CacheLayer<String, String>>) -> Self {
        self.l3_cache = Some(cache);
        self
    }

    pub fn with_l4_cache(mut self, cache: Arc<dyn CacheLayer<String, String>>) -> Self {
        self.l4_cache = Some(cache);
        self
    }

    pub fn get_l1_cache(&self) -> Option<&Arc<dyn CacheLayer<String, String>>> {
        self.l1_cache.as_ref()
    }

    pub fn get_l2_cache(&self) -> Option<&Arc<dyn CacheLayer<String, String>>> {
        self.l2_cache.as_ref()
    }

    pub fn get_l3_cache(&self) -> Option<&Arc<dyn CacheLayer<String, String>>> {
        self.l3_cache.as_ref()
    }

    pub fn get_l4_cache(&self) -> Option<&Arc<dyn CacheLayer<String, String>>> {
        self.l4_cache.as_ref()
    }

    pub fn get_stats(&self, layer: &str) -> Option<CacheStats> {
        self.stats.read().unwrap().get(layer).cloned()
    }

    pub fn update_stats(&self, layer: &str, stats: CacheStats) {
        self.stats.write().unwrap().insert(layer.to_string(), stats);
    }

    pub fn get_all_stats(&self) -> HashMap<String, CacheStats> {
        self.stats.read().unwrap().clone()
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache key generation utilities
pub mod keygen {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    /// Generate a hash-based cache key from any hashable type
    pub fn hash_key<T: Hash>(value: &T) -> String {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Generate a composite key from multiple components
    pub fn composite_key(components: &[&str]) -> String {
        components.join("::")
    }

    /// Generate a delegation cache key: (Agent, Task) -> Plan
    pub fn delegation_key(agent: &str, task: &str) -> String {
        composite_key(&[agent, task])
    }

    /// Generate an inference cache key for model calls
    pub fn inference_key(model_id: &str, input: &str) -> String {
        let input_hash = hash_input(input);
        composite_key(&[model_id, &input_hash])
    }

    /// Hash an input string for cache key generation
    pub fn hash_input(input: &str) -> String {
        hash_key(&input)
    }

    /// Generate a semantic cache key
    pub fn semantic_key(embedding_hash: &str) -> String {
        format!("semantic:{}", embedding_hash)
    }

    /// Generate a content-addressable RTFS key
    pub fn content_addressable_key(bytecode_hash: &str) -> String {
        format!("rtfs:{}", bytecode_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_stats() {
        let mut stats = CacheStats::new(100);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.hit_rate, 0.0);

        stats.record_hit();
        stats.record_miss();
        stats.record_hit();

        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate - 2.0 / 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cache_entry() {
        let entry = CacheEntry::new("test_value");
        assert_eq!(entry.access_count, 1);

        let mut entry = entry;
        entry.access();
        assert_eq!(entry.access_count, 2);
    }

    #[test]
    fn test_key_generation() {
        assert_eq!(keygen::composite_key(&["a", "b", "c"]), "a::b::c");
        assert_eq!(keygen::delegation_key("agent1", "task1"), "agent1::task1");
        let expected = format!("model1::{}", keygen::hash_input("hash123"));
        assert_eq!(keygen::inference_key("model1", "hash123"), expected);
        assert_eq!(keygen::semantic_key("emb123"), "semantic:emb123");
        assert_eq!(keygen::content_addressable_key("bc123"), "rtfs:bc123");
    }
}
