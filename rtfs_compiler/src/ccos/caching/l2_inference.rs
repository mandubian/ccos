//! L2 Inference Cache: (Model, Input) -> Output memoization
//! 
//! This layer provides fast lookup for model inference results, caching the mapping
//! from model-input pairs to outputs with confidence scores. This is the second
//! layer that operates on actual inference data rather than delegation decisions.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use super::{CacheLayer, CacheStats, CacheConfig, CacheEntry, CacheError, EvictionPolicy, keygen};

/// Inference result with metadata
#[derive(Debug, Clone)]
pub struct InferenceResult {
    pub output: String,              // Model output
    pub confidence: f64,             // Confidence score (0.0 - 1.0)
    pub model_version: String,       // Model version/identifier
    pub inference_time_ms: u64,      // Time taken for inference
    pub created_at: Instant,         // When this result was created
    pub metadata: HashMap<String, String>, // Additional metadata
}

impl InferenceResult {
    pub fn new(output: String, confidence: f64, model_version: String, inference_time_ms: u64) -> Self {
        Self {
            output,
            confidence,
            model_version,
            inference_time_ms,
            created_at: Instant::now(),
            metadata: HashMap::new(),
        }
    }
    
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
    
    pub fn is_stale(&self, max_age: Duration) -> bool {
        self.created_at.elapsed() > max_age
    }
    
    pub fn is_low_confidence(&self, threshold: f64) -> bool {
        self.confidence < threshold
    }
}

/// L2 Inference Cache implementation
#[derive(Debug)]
pub struct L2InferenceCache {
    config: CacheConfig,
    cache: Arc<RwLock<HashMap<String, CacheEntry<InferenceResult>>>>,
    access_order: Arc<RwLock<VecDeque<String>>>, // For LRU tracking
    stats: Arc<RwLock<CacheStats>>,
}

impl L2InferenceCache {
    pub fn new(config: CacheConfig) -> Self {
        let stats = CacheStats::new(config.max_size);
        Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(stats)),
        }
    }
    
    pub fn with_default_config() -> Self {
        let mut config = CacheConfig::default();
        config.max_size = 2000; // Larger default for inference cache
        config.ttl = Some(Duration::from_secs(7200)); // 2 hour TTL
        config.eviction_policy = EvictionPolicy::LRU;
        Self::new(config)
    }
    
    /// Get cache statistics directly
    pub fn get_stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }
    
    /// Get an inference result for a model-input pair
    pub fn get_inference(&self, model_id: &str, input: &str) -> Option<InferenceResult> {
        let key = keygen::inference_key(model_id, input);
        
        if let Some(result) = CacheLayer::<String, InferenceResult>::get(self, &key) {
            // Check if result is stale
            if let Some(ttl) = self.config.ttl {
                if result.is_stale(ttl) {
                    // Result is stale, remove it
                    let _ = CacheLayer::<String, InferenceResult>::invalidate(self, &key);
                    return None;
                }
            }
            
            // Check if result has low confidence (optional filtering)
            if let Some(confidence_threshold) = self.config.confidence_threshold {
                if result.is_low_confidence(confidence_threshold) {
                    // Low confidence result, remove it
                    let _ = CacheLayer::<String, InferenceResult>::invalidate(self, &key);
                    return None;
                }
            }
            
            // Update access order for LRU
            self.update_access_order(&key);
            return Some(result);
        }
        
        None
    }
    
    /// Store an inference result for a model-input pair
    pub fn put_inference(&self, model_id: &str, input: &str, result: InferenceResult) -> Result<(), CacheError> {
        let key = keygen::inference_key(model_id, input);
        self.put(key, result)
    }
    
    /// Invalidate a specific model-input pair
    pub fn invalidate_inference(&self, model_id: &str, input: &str) -> Result<(), CacheError> {
        let key = keygen::inference_key(model_id, input);
        CacheLayer::<String, InferenceResult>::invalidate(self, &key)
    }
    
    /// Get all results for a specific model
    pub fn get_model_results(&self, model_id: &str) -> Vec<(String, InferenceResult)> {
        let cache = self.cache.read().unwrap();
        cache
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{}::", model_id)))
            .map(|(key, entry)| {
                let input = key.split("::").nth(1).unwrap_or("unknown");
                (input.to_string(), entry.value.clone())
            })
            .collect()
    }
    
    /// Get all results for a specific input (across all models)
    pub fn get_input_results(&self, input: &str) -> Vec<(String, InferenceResult)> {
        let cache = self.cache.read().unwrap();
        let input_hash = keygen::hash_input(input);
        cache
            .iter()
            .filter(|(key, _)| key.ends_with(&format!("::{}", input_hash)))
            .map(|(key, entry)| {
                let model_id = key.split("::").next().unwrap_or("unknown");
                (model_id.to_string(), entry.value.clone())
            })
            .collect()
    }
    
    /// Update access order for LRU eviction
    fn update_access_order(&self, key: &str) {
        let mut order = self.access_order.write().unwrap();
        
        // Remove key from current position
        if let Some(pos) = order.iter().position(|k| k == key) {
            order.remove(pos);
        }
        
        // Add to front (most recently used)
        order.push_front(key.to_string());
    }
    
    /// Evict entries based on the configured policy
    fn evict_if_needed(&self) -> Result<(), CacheError> {
        let mut cache = self.cache.write().unwrap();
        let mut order = self.access_order.write().unwrap();
        
        if cache.len() <= self.config.max_size {
            return Ok(());
        }
        
        let to_evict = cache.len() - self.config.max_size;
        
        match self.config.eviction_policy {
            EvictionPolicy::LRU => {
                // Remove least recently used entries
                for _ in 0..to_evict {
                    if let Some(key) = order.pop_back() {
                        cache.remove(&key);
                    }
                }
            }
            EvictionPolicy::FIFO => {
                // Remove oldest entries (first in)
                let keys: Vec<String> = cache.keys().cloned().collect();
                for key in keys.iter().take(to_evict) {
                    cache.remove(key);
                    if let Some(pos) = order.iter().position(|k| k == key) {
                        order.remove(pos);
                    }
                }
            }
            EvictionPolicy::Random => {
                // Remove random entries
                let keys: Vec<String> = cache.keys().cloned().collect();
                use rand::seq::SliceRandom;
                let mut rng = rand::thread_rng();
                let to_remove: Vec<&String> = keys.choose_multiple(&mut rng, to_evict).collect();
                
                for key in to_remove {
                    cache.remove(key);
                    if let Some(pos) = order.iter().position(|k| k == key) {
                        order.remove(pos);
                    }
                }
            }
            EvictionPolicy::TTL => {
                // Remove expired entries
                if let Some(ttl) = self.config.ttl {
                    let _now = Instant::now();
                    let expired_keys: Vec<String> = cache
                        .iter()
                        .filter(|(_, entry)| entry.value.is_stale(ttl))
                        .map(|(key, _)| key.clone())
                        .collect();
                    
                    for key in expired_keys {
                        cache.remove(&key);
                        if let Some(pos) = order.iter().position(|k| k == &key) {
                            order.remove(pos);
                        }
                    }
                }
            }
            EvictionPolicy::LFU => {
                // Remove least frequently used entries
                let mut entries: Vec<(String, u64)> = cache
                    .iter()
                    .map(|(key, entry)| (key.clone(), entry.access_count))
                    .collect();
                
                entries.sort_by_key(|(_, count)| *count);
                
                for (key, _) in entries.iter().take(to_evict) {
                    cache.remove(key);
                    if let Some(pos) = order.iter().position(|k| k == key) {
                        order.remove(pos);
                    }
                }
            }
        }
        
        Ok(())
    }
}

impl CacheLayer<String, InferenceResult> for L2InferenceCache {
    fn get(&self, key: &String) -> Option<InferenceResult> {
        let mut stats = self.stats.write().unwrap();
        
        if let Some(entry) = self.cache.read().unwrap().get(key) {
            stats.hits += 1;
            stats.update_hit_rate();
            Some(entry.value.clone())
        } else {
            stats.misses += 1;
            stats.update_hit_rate();
            None
        }
    }
    
    fn put(&self, key: String, value: InferenceResult) -> Result<(), CacheError> {
        let mut stats = self.stats.write().unwrap();
        
        // Evict if needed before inserting
        self.evict_if_needed()?;
        
        let entry = CacheEntry::new(value);
        
        self.cache.write().unwrap().insert(key.clone(), entry);
        self.update_access_order(&key);
        
        stats.puts += 1;
        stats.size = self.cache.read().unwrap().len();
        
        Ok(())
    }
    
    fn invalidate(&self, key: &String) -> Result<(), CacheError> {
        let mut stats = self.stats.write().unwrap();
        
        if self.cache.write().unwrap().remove(key).is_some() {
            // Remove from access order
            let mut order = self.access_order.write().unwrap();
            if let Some(pos) = order.iter().position(|k| k == key) {
                order.remove(pos);
            }
            
            stats.invalidations += 1;
            stats.size = self.cache.read().unwrap().len();
        }
        
        Ok(())
    }
    
    fn stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }
    
    fn clear(&self) -> Result<(), CacheError> {
        let mut stats = self.stats.write().unwrap();
        let mut cache = self.cache.write().unwrap();
        let mut order = self.access_order.write().unwrap();
        
        cache.clear();
        order.clear();
        
        stats.size = 0;
        stats.invalidations += 1;
        
        Ok(())
    }
    
    fn config(&self) -> &CacheConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_inference_result() {
        let result = InferenceResult::new(
            "Hello, world!".to_string(),
            0.95,
            "gpt4o-v1".to_string(),
            150,
        );
        
        assert_eq!(result.output, "Hello, world!");
        assert_eq!(result.confidence, 0.95);
        assert_eq!(result.model_version, "gpt4o-v1");
        assert_eq!(result.inference_time_ms, 150);
        assert!(!result.is_stale(Duration::from_secs(1)));
        assert!(!result.is_low_confidence(0.9));
        assert!(result.is_low_confidence(0.99));
    }
    
    #[test]
    fn test_l2_cache_basic_operations() {
        let cache = L2InferenceCache::with_default_config();
        
        let result = InferenceResult::new(
            "Test output".to_string(),
            0.9,
            "test-model".to_string(),
            100,
        );
        
        // Put inference result
        cache.put_inference("test-model", "test input", result.clone()).unwrap();
        
        // Get inference result
        let retrieved = cache.get_inference("test-model", "test input").unwrap();
        assert_eq!(retrieved.output, "Test output");
        assert_eq!(retrieved.confidence, 0.9);
        
        // Check stats
        let stats = cache.get_stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.puts, 1);
    }
    
    #[test]
    fn test_l2_cache_invalidation() {
        let cache = L2InferenceCache::with_default_config();
        
        let result = InferenceResult::new(
            "Test output".to_string(),
            0.9,
            "test-model".to_string(),
            100,
        );
        
        cache.put_inference("test-model", "test input", result).unwrap();
        
        // Should exist
        assert!(cache.get_inference("test-model", "test input").is_some());
        
        // Invalidate
        cache.invalidate_inference("test-model", "test input").unwrap();
        
        // Should not exist
        assert!(cache.get_inference("test-model", "test input").is_none());
        
        let stats = cache.get_stats();
        assert_eq!(stats.invalidations, 1);
    }
    
    #[test]
    fn test_l2_cache_model_results() {
        let cache = L2InferenceCache::with_default_config();
        
        let result1 = InferenceResult::new(
            "Output 1".to_string(),
            0.9,
            "model-a".to_string(),
            100,
        );
        let result2 = InferenceResult::new(
            "Output 2".to_string(),
            0.8,
            "model-a".to_string(),
            120,
        );
        let result3 = InferenceResult::new(
            "Output 3".to_string(),
            0.95,
            "model-b".to_string(),
            80,
        );
        
        cache.put_inference("model-a", "input 1", result1).unwrap();
        cache.put_inference("model-a", "input 2", result2).unwrap();
        cache.put_inference("model-b", "input 1", result3).unwrap();
        
        let model_a_results = cache.get_model_results("model-a");
        assert_eq!(model_a_results.len(), 2);
        
        let model_b_results = cache.get_model_results("model-b");
        assert_eq!(model_b_results.len(), 1);
    }
    
    #[test]
    fn test_l2_cache_stats() {
        let cache = L2InferenceCache::with_default_config();
        
        let result = InferenceResult::new(
            "Test output".to_string(),
            0.9,
            "test-model".to_string(),
            100,
        );
        
        // Initial stats
        let initial_stats = cache.get_stats();
        assert_eq!(initial_stats.hits, 0);
        assert_eq!(initial_stats.misses, 0);
        assert_eq!(initial_stats.puts, 0);
        
        // Put and get
        cache.put_inference("test-model", "test input", result).unwrap();
        cache.get_inference("test-model", "test input").unwrap();
        
        let final_stats = cache.get_stats();
        assert_eq!(final_stats.hits, 1);
        assert_eq!(final_stats.misses, 0);
        assert_eq!(final_stats.puts, 1);
        assert!(final_stats.hit_rate > 0.0);
    }
    

} 