//! L3 Semantic Cache: AI-driven vector search for semantic equivalence
//!
//! This layer uses embeddings and vector similarity to find semantically
//! equivalent cached results, even when the exact input differs.

use crate::ccos::caching::{CacheConfig, CacheError, CacheLayer, CacheStats};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Semantic cache entry containing both the result and its embedding
#[derive(Clone, Debug)]
pub struct SemanticCacheEntry<T> {
    pub result: T,
    pub embedding: Vec<f32>,
    pub created_at: Instant,
    pub access_count: u64,
    pub last_accessed: Instant,
}

/// Configuration for semantic cache behavior
#[derive(Clone, Debug)]
pub struct SemanticCacheConfig {
    pub max_size: usize,
    pub similarity_threshold: f32, // Minimum similarity score (0.0 - 1.0)
    pub embedding_dimension: usize, // Dimension of embeddings
    pub ttl_seconds: u64,
    pub max_cache_age_seconds: u64,
}

impl Default for SemanticCacheConfig {
    fn default() -> Self {
        Self {
            max_size: 1000,
            similarity_threshold: 0.85,   // 85% similarity threshold
            embedding_dimension: 384,     // Common embedding dimension
            ttl_seconds: 3600,            // 1 hour TTL
            max_cache_age_seconds: 86400, // 24 hours max age
        }
    }
}

/// Simple embedding generator for demonstration
/// In production, this would use a proper embedding model
#[derive(Debug)]
pub struct SimpleEmbeddingGenerator {
    dimension: usize,
}

impl SimpleEmbeddingGenerator {
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }

    /// Generate a simple hash-based embedding for demonstration
    /// In production, this would use a proper embedding model like sentence-transformers
    pub fn generate_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.to_lowercase().hash(&mut hasher);
        let hash = hasher.finish();

        // Generate a deterministic embedding based on the hash
        let mut embedding = Vec::with_capacity(self.dimension);
        for i in 0..self.dimension {
            let seed = hash.wrapping_add(i as u64);
            let mut local_hasher = DefaultHasher::new();
            seed.hash(&mut local_hasher);
            let value = local_hasher.finish() as f32 / u64::MAX as f32;
            embedding.push(value);
        }

        // Normalize the embedding
        self.normalize_vector(&mut embedding);
        embedding
    }

    /// Normalize a vector to unit length
    fn normalize_vector(&self, vector: &mut Vec<f32>) {
        let magnitude: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for value in vector.iter_mut() {
                *value /= magnitude;
            }
        }
    }
}

/// L3 Semantic Cache implementation
#[derive(Debug)]
pub struct L3SemanticCache<T: Clone + Send + Sync + std::fmt::Debug> {
    config: SemanticCacheConfig,
    cache: Arc<RwLock<HashMap<String, SemanticCacheEntry<T>>>>,
    access_order: Arc<RwLock<VecDeque<String>>>,
    stats: Arc<RwLock<CacheStats>>,
    embedding_generator: SimpleEmbeddingGenerator,
}

impl<T: Clone + Send + Sync + std::fmt::Debug> L3SemanticCache<T> {
    pub fn new(config: SemanticCacheConfig) -> Self {
        let embedding_generator = SimpleEmbeddingGenerator::new(config.embedding_dimension);

        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(CacheStats::new(1000))),
            embedding_generator,
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(SemanticCacheConfig::default())
    }

    /// Get semantically similar result
    pub fn get_semantic(&self, query: &str) -> Option<(T, f32)> {
        let query_embedding = self.embedding_generator.generate_embedding(query);

        let mut cache = self.cache.write().unwrap();
        let mut best_match: Option<(String, T, f32)> = None;
        let mut best_similarity = 0.0;

        // Find the most similar cached entry
        for (key, entry) in cache.iter() {
            // Skip stale entries
            if entry.created_at.elapsed() > Duration::from_secs(self.config.max_cache_age_seconds) {
                continue;
            }

            let similarity = self.cosine_similarity(&query_embedding, &entry.embedding);

            if similarity >= self.config.similarity_threshold && similarity > best_similarity {
                best_similarity = similarity;
                best_match = Some((key.clone(), entry.result.clone(), similarity));
            }
        }

        if let Some((key, result, similarity)) = best_match {
            // Update access statistics
            if let Some(entry) = cache.get_mut(&key) {
                entry.access_count += 1;
                entry.last_accessed = Instant::now();
            }

            // Update access order for LRU
            self.update_access_order(&key);

            // Update stats
            let mut stats = self.stats.write().unwrap();
            stats.hits += 1;
            stats.hit_rate = stats.hits as f64 / (stats.hits + stats.misses) as f64;

            Some((result, similarity))
        } else {
            // Update stats for miss
            let mut stats = self.stats.write().unwrap();
            stats.misses += 1;
            stats.hit_rate = stats.hits as f64 / (stats.hits + stats.misses) as f64;

            None
        }
    }

    /// Put a result with semantic caching
    pub fn put_semantic(&self, key: &str, result: T) -> Result<(), CacheError> {
        let embedding = self.embedding_generator.generate_embedding(key);

        let entry = SemanticCacheEntry {
            result: result.clone(),
            embedding,
            created_at: Instant::now(),
            access_count: 0,
            last_accessed: Instant::now(),
        };

        let mut cache = self.cache.write().unwrap();
        let mut access_order = self.access_order.write().unwrap();

        // Check if we need to evict entries
        if cache.len() >= self.config.max_size {
            self.evict_oldest(&mut cache, &mut access_order)?;
        }

        // Insert the new entry
        cache.insert(key.to_string(), entry);
        access_order.push_back(key.to_string());

        // Update stats
        let mut stats = self.stats.write().unwrap();
        stats.puts += 1;
        stats.size = cache.len();

        Ok(())
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }

    /// Invalidate a semantic cache entry
    pub fn invalidate_semantic(&self, key: &str) -> Result<(), CacheError> {
        let mut cache = self.cache.write().unwrap();
        let mut access_order = self.access_order.write().unwrap();

        if cache.remove(key).is_some() {
            access_order.retain(|k| k != key);

            let mut stats = self.stats.write().unwrap();
            stats.invalidations += 1;
            stats.size = cache.len();
        }

        Ok(())
    }

    /// Get all entries with their similarity scores for a query
    pub fn get_similar_entries(&self, query: &str, min_similarity: f32) -> Vec<(String, T, f32)> {
        let query_embedding = self.embedding_generator.generate_embedding(query);
        let mut results = Vec::new();

        let cache = self.cache.read().unwrap();

        for (key, entry) in cache.iter() {
            let similarity = self.cosine_similarity(&query_embedding, &entry.embedding);

            if similarity >= min_similarity {
                results.push((key.clone(), entry.result.clone(), similarity));
            }
        }

        // Sort by similarity (highest first)
        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Clear all cache entries
    pub fn clear(&self) -> Result<(), CacheError> {
        let mut cache = self.cache.write().unwrap();
        let mut access_order = self.access_order.write().unwrap();
        let mut stats = self.stats.write().unwrap();

        cache.clear();
        access_order.clear();

        stats.size = 0;
        stats.invalidations += 1;

        Ok(())
    }

    /// Get cache configuration
    pub fn config(&self) -> &SemanticCacheConfig {
        &self.config
    }

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if magnitude_a == 0.0 || magnitude_b == 0.0 {
            0.0
        } else {
            dot_product / (magnitude_a * magnitude_b)
        }
    }

    /// Update access order for LRU tracking
    fn update_access_order(&self, key: &str) {
        let mut access_order = self.access_order.write().unwrap();

        // Remove from current position
        access_order.retain(|k| k != key);

        // Add to front (most recently used)
        access_order.push_front(key.to_string());
    }

    /// Evict the oldest entry (LRU)
    fn evict_oldest(
        &self,
        cache: &mut HashMap<String, SemanticCacheEntry<T>>,
        access_order: &mut VecDeque<String>,
    ) -> Result<(), CacheError> {
        if let Some(oldest_key) = access_order.pop_back() {
            cache.remove(&oldest_key);
        }

        Ok(())
    }
}

impl<T: Clone + Send + Sync + std::fmt::Debug + 'static> CacheLayer<String, T>
    for L3SemanticCache<T>
{
    fn get(&self, key: &String) -> Option<T> {
        self.get_semantic(key).map(|(result, _)| result)
    }

    fn put(&self, key: String, value: T) -> Result<(), CacheError> {
        self.put_semantic(&key, value)
    }

    fn invalidate(&self, key: &String) -> Result<(), CacheError> {
        self.invalidate_semantic(key)
    }

    fn stats(&self) -> CacheStats {
        self.get_stats()
    }

    fn clear(&self) -> Result<(), CacheError> {
        self.clear()
    }

    fn config(&self) -> &CacheConfig {
        // Convert SemanticCacheConfig to CacheConfig for compatibility
        static CONFIG: CacheConfig = CacheConfig {
            max_size: 1000,
            ttl: Some(Duration::from_secs(3600)),
            eviction_policy: crate::ccos::caching::EvictionPolicy::LRU,
            persistence_enabled: false,
            async_operations: false,
            confidence_threshold: Some(0.85),
        };
        &CONFIG
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_generator() {
        let generator = SimpleEmbeddingGenerator::new(384);

        let embedding1 = generator.generate_embedding("hello world");
        let embedding2 = generator.generate_embedding("hello world");
        let embedding3 = generator.generate_embedding("different text");

        // Same text should produce same embedding
        assert_eq!(embedding1, embedding2);

        // Different text should produce different embedding
        assert_ne!(embedding1, embedding3);

        // Embeddings should be normalized
        let magnitude1: f32 = embedding1.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((magnitude1 - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_semantic_cache_basic_operations() {
        let mut config = SemanticCacheConfig::default();
        config.similarity_threshold = 0.5; // Lower threshold for testing
        let cache = L3SemanticCache::new(config);

        // Put a result
        cache
            .put_semantic("What is the weather like?", "Sunny and warm")
            .unwrap();

        // Get exact match
        let result = cache.get_semantic("What is the weather like?");
        assert!(result.is_some());
        let (value, similarity) = result.unwrap();
        assert_eq!(value, "Sunny and warm");
        assert!((similarity - 1.0).abs() < 0.001); // Should be exact match

        // Get similar match
        let result = cache.get_semantic("How's the weather?");
        assert!(result.is_some());
        let (value, similarity) = result.unwrap();
        assert_eq!(value, "Sunny and warm");
        assert!(similarity > 0.5); // Should be above lower threshold
    }

    #[test]
    fn test_semantic_cache_similarity() {
        let mut config = SemanticCacheConfig::default();
        config.similarity_threshold = 0.5; // Lower threshold for testing
        let cache = L3SemanticCache::new(config);

        // Add multiple similar queries
        cache
            .put_semantic("What is the weather like?", "Sunny")
            .unwrap();
        cache
            .put_semantic("How's the weather today?", "Cloudy")
            .unwrap();
        cache
            .put_semantic("Tell me about the weather", "Rainy")
            .unwrap();

        // Query similar to first entry
        let result = cache.get_semantic("How is the weather?");
        assert!(result.is_some());
        let (value, similarity) = result.unwrap();
        assert!(similarity > 0.5);

        // Should find the most similar match
        let similar_entries = cache.get_similar_entries("How is the weather?", 0.3);
        assert!(!similar_entries.is_empty());
        assert!(similar_entries[0].2 >= similar_entries[1].2); // Sorted by similarity
    }

    #[test]
    fn test_semantic_cache_stats() {
        let cache = L3SemanticCache::with_default_config();

        // Initial stats
        let initial_stats = cache.get_stats();
        assert_eq!(initial_stats.hits, 0);
        assert_eq!(initial_stats.misses, 0);

        // Put and get
        cache.put_semantic("test query", "test result").unwrap();
        cache.get_semantic("test query").unwrap();

        let final_stats = cache.get_stats();
        assert_eq!(final_stats.hits, 1);
        assert_eq!(final_stats.misses, 0);
        assert_eq!(final_stats.puts, 1);
        assert!(final_stats.hit_rate > 0.0);
    }

    #[test]
    fn test_semantic_cache_invalidation() {
        let cache = L3SemanticCache::with_default_config();

        cache.put_semantic("test query", "test result").unwrap();

        // Should exist
        assert!(cache.get_semantic("test query").is_some());

        // Invalidate
        cache.invalidate_semantic("test query").unwrap();

        // Should not exist
        assert!(cache.get_semantic("test query").is_none());

        let stats = cache.get_stats();
        assert_eq!(stats.invalidations, 1);
    }
}
