//! L1 Delegation Cache: (Agent, Task) -> Plan memoization
//! 
//! This layer provides fast lookup for delegation decisions, caching the mapping
//! from agent-task pairs to execution plans. This is the highest-level cache
//! that operates on semantic concepts rather than low-level data.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use super::{CacheLayer, CacheStats, CacheConfig, CacheEntry, CacheError, EvictionPolicy, keygen};

/// Delegation plan with metadata
#[derive(Debug, Clone)]
pub struct DelegationPlan {
    pub target: String,           // Model provider or execution target
    pub confidence: f64,          // Confidence score (0.0 - 1.0)
    pub reasoning: String,        // Human-readable reasoning
    pub created_at: Instant,      // When this plan was created
    pub metadata: HashMap<String, String>, // Additional metadata
}

impl DelegationPlan {
    pub fn new(target: String, confidence: f64, reasoning: String) -> Self {
        Self {
            target,
            confidence,
            reasoning,
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
}

/// L1 Delegation Cache implementation
#[derive(Debug)]
pub struct L1DelegationCache {
    config: CacheConfig,
    cache: Arc<RwLock<HashMap<String, CacheEntry<DelegationPlan>>>>,
    access_order: Arc<RwLock<VecDeque<String>>>, // For LRU tracking
    stats: Arc<RwLock<CacheStats>>,
}

impl L1DelegationCache {
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
        config.max_size = 1000; // Reasonable default for delegation cache
        config.ttl = Some(Duration::from_secs(3600)); // 1 hour TTL
        config.eviction_policy = EvictionPolicy::LRU;
        Self::new(config)
    }
    
    /// Get cache statistics directly
    pub fn get_stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }
    
    /// Get a delegation plan for an agent-task pair
    pub fn get_plan(&self, agent: &str, task: &str) -> Option<DelegationPlan> {
        let key = keygen::delegation_key(agent, task);
        
        if let Some(plan) = CacheLayer::<String, DelegationPlan>::get(self, &key) {
            // Check if plan is stale
            if let Some(ttl) = self.config.ttl {
                if plan.is_stale(ttl) {
                    // Plan is stale, remove it
                    let _ = CacheLayer::<String, DelegationPlan>::invalidate(self, &key);
                    return None;
                }
            }
            
            // Update access order for LRU
            self.update_access_order(&key);
            return Some(plan);
        }
        
        None
    }
    
    /// Store a delegation plan for an agent-task pair
    pub fn put_plan(&self, agent: &str, task: &str, plan: DelegationPlan) -> Result<(), CacheError> {
        let key = keygen::delegation_key(agent, task);
        self.put(key, plan)
    }
    
    /// Invalidate a specific agent-task pair
    pub fn invalidate_plan(&self, agent: &str, task: &str) -> Result<(), CacheError> {
        let key = keygen::delegation_key(agent, task);
        CacheLayer::<String, DelegationPlan>::invalidate(self, &key)
    }
    
    /// Get all plans for a specific agent
    pub fn get_agent_plans(&self, agent: &str) -> Vec<(String, DelegationPlan)> {
        let cache = self.cache.read().unwrap();
        cache
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{}::", agent)))
            .map(|(key, entry)| {
                let task = key.split("::").nth(1).unwrap_or("unknown");
                (task.to_string(), entry.value.clone())
            })
            .collect()
    }
    
    /// Get all plans for a specific task
    pub fn get_task_plans(&self, task: &str) -> Vec<(String, DelegationPlan)> {
        let cache = self.cache.read().unwrap();
        cache
            .iter()
            .filter(|(key, _)| key.ends_with(&format!("::{}", task)))
            .map(|(key, entry)| {
                let agent = key.split("::").next().unwrap_or("unknown");
                (agent.to_string(), entry.value.clone())
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
                    let now = Instant::now();
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

impl CacheLayer<String, DelegationPlan> for L1DelegationCache {
    fn get(&self, key: &String) -> Option<DelegationPlan> {
        let mut cache = self.cache.write().unwrap();
        
        if let Some(entry) = cache.get_mut(key) {
            entry.access();
            self.update_access_order(key);
            
            let mut stats = self.stats.write().unwrap();
            stats.record_hit();
            
            Some(entry.value.clone())
        } else {
            let mut stats = self.stats.write().unwrap();
            stats.record_miss();
            None
        }
    }
    
    fn put(&self, key: String, value: DelegationPlan) -> Result<(), CacheError> {
        let entry = CacheEntry::new(value);
        
        {
            let mut cache = self.cache.write().unwrap();
            cache.insert(key.clone(), entry);
        }
        
        self.update_access_order(&key);
        
        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.record_put();
            stats.size = self.cache.read().unwrap().len();
        }
        
        // Evict if needed
        self.evict_if_needed()?;
        
        Ok(())
    }
    
    fn invalidate(&self, key: &String) -> Result<(), CacheError> {
        let mut cache = self.cache.write().unwrap();
        let mut order = self.access_order.write().unwrap();
        
        if cache.remove(key).is_some() {
            if let Some(pos) = order.iter().position(|k| k == key) {
                order.remove(pos);
            }
            
            let mut stats = self.stats.write().unwrap();
            stats.record_invalidation();
            stats.size = cache.len();
            
            Ok(())
        } else {
            Err(CacheError::KeyNotFound)
        }
    }
    
    fn stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }
    
    fn clear(&self) -> Result<(), CacheError> {
        let mut cache = self.cache.write().unwrap();
        let mut order = self.access_order.write().unwrap();
        
        cache.clear();
        order.clear();
        
        let mut stats = self.stats.write().unwrap();
        stats.size = 0;
        
        Ok(())
    }
    
    fn config(&self) -> &CacheConfig {
        &self.config
    }
}

/// Cache layer for string-based keys (compatibility with CacheManager)
impl CacheLayer<String, String> for L1DelegationCache {
    fn get(&self, key: &String) -> Option<String> {
        // This is a compatibility layer - we don't actually use string values
        // in the delegation cache, but we need this for the CacheManager
        None
    }
    
    fn put(&self, _key: String, _value: String) -> Result<(), CacheError> {
        // This is a compatibility layer - delegation cache uses DelegationPlan
        Err(CacheError::ConfigError("L1DelegationCache requires DelegationPlan values".to_string()))
    }
    
    fn invalidate(&self, key: &String) -> Result<(), CacheError> {
        // Extract agent and task from key and invalidate the plan
        if let Some((agent, task)) = key.split_once("::") {
            self.invalidate_plan(agent, task)
        } else {
            Err(CacheError::KeyNotFound)
        }
    }
    
    fn stats(&self) -> CacheStats {
        self.get_stats()
    }
    
    fn clear(&self) -> Result<(), CacheError> {
        CacheLayer::<String, DelegationPlan>::clear(self)
    }
    
    fn config(&self) -> &CacheConfig {
        CacheLayer::<String, DelegationPlan>::config(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    #[test]
    fn test_delegation_plan() {
        let plan = DelegationPlan::new(
            "echo-model".to_string(),
            0.95,
            "High confidence for simple text processing".to_string(),
        );
        
        assert_eq!(plan.target, "echo-model");
        assert_eq!(plan.confidence, 0.95);
        assert!(!plan.is_stale(Duration::from_secs(1)));
    }
    
    #[test]
    fn test_l1_cache_basic_operations() {
        let cache = L1DelegationCache::with_default_config();
        
        let plan = DelegationPlan::new(
            "echo-model".to_string(),
            0.9,
            "Test plan".to_string(),
        );
        
        // Test put and get
        assert!(cache.put_plan("agent1", "task1", plan.clone()).is_ok());
        let retrieved = cache.get_plan("agent1", "task1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().target, "echo-model");
        
        // Test miss
        let miss = cache.get_plan("agent1", "task2");
        assert!(miss.is_none());
    }
    
    #[test]
    fn test_l1_cache_invalidation() {
        let cache = L1DelegationCache::with_default_config();
        
        let plan = DelegationPlan::new(
            "echo-model".to_string(),
            0.9,
            "Test plan".to_string(),
        );
        
        cache.put_plan("agent1", "task1", plan).unwrap();
        assert!(cache.get_plan("agent1", "task1").is_some());
        
        cache.invalidate_plan("agent1", "task1").unwrap();
        assert!(cache.get_plan("agent1", "task1").is_none());
    }
    
    #[test]
    fn test_l1_cache_agent_plans() {
        let cache = L1DelegationCache::with_default_config();
        
        let plan1 = DelegationPlan::new("model1".to_string(), 0.8, "Plan 1".to_string());
        let plan2 = DelegationPlan::new("model2".to_string(), 0.9, "Plan 2".to_string());
        
        cache.put_plan("agent1", "task1", plan1).unwrap();
        cache.put_plan("agent1", "task2", plan2).unwrap();
        cache.put_plan("agent2", "task1", DelegationPlan::new("model3".to_string(), 0.7, "Plan 3".to_string())).unwrap();
        
        let agent_plans = cache.get_agent_plans("agent1");
        assert_eq!(agent_plans.len(), 2);
        
        let task_plans = cache.get_task_plans("task1");
        assert_eq!(task_plans.len(), 2);
    }
    
    #[test]
    fn test_l1_cache_stats() {
        let cache = L1DelegationCache::with_default_config();
        
        let plan = DelegationPlan::new("model1".to_string(), 0.9, "Test plan".to_string());
        
        // Put and get to generate stats
        cache.put_plan("agent1", "task1", plan).unwrap();
        cache.get_plan("agent1", "task1");
        cache.get_plan("agent1", "task2"); // Miss
        
        let stats = cache.get_stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.puts, 1);
        assert!((stats.hit_rate - 0.5).abs() < f64::EPSILON);
    }
} 