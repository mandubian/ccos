//! Cache hint handler - memoizes capability results.

use crate::hints::types::{BoxFuture, ExecutionContext, HintHandler, NextExecutor};
use crate::types::{Action, ActionType};
use rtfs::runtime::execution_outcome::HostCall;
use rtfs::runtime::values::Value;
use rtfs::runtime::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Handler for the `runtime.learning.cache` hint.
///
/// Memoizes capability results based on input arguments.
/// Cached values expire after a configurable TTL.
///
/// # Hint Format
/// ```rtfs
/// ^{:runtime.learning.cache {:ttl-ms 60000 :max-entries 100}}
/// ```
///
/// # Fields
/// - `ttl-ms`: Time-to-live in milliseconds (default: 60000 = 1 minute)
/// - `max-entries`: Maximum cache entries per capability (default: 100)
pub struct CacheHintHandler {
    /// Cache per capability ID -> (args_hash -> (value, timestamp))
    caches: Mutex<HashMap<String, CapabilityCache>>,
}

struct CapabilityCache {
    entries: HashMap<u64, CacheEntry>,
    max_entries: usize,
    ttl: Duration,
}

struct CacheEntry {
    value: Value,
    created_at: Instant,
}

impl CapabilityCache {
    fn new(max_entries: usize, ttl_ms: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_entries,
            ttl: Duration::from_millis(ttl_ms),
        }
    }

    fn get(&self, args_hash: u64) -> Option<&Value> {
        self.entries.get(&args_hash).and_then(|entry| {
            if entry.created_at.elapsed() < self.ttl {
                Some(&entry.value)
            } else {
                None
            }
        })
    }

    fn put(&mut self, args_hash: u64, value: Value) {
        // Evict expired entries if at capacity
        if self.entries.len() >= self.max_entries {
            let _now = Instant::now();
            self.entries
                .retain(|_, entry| entry.created_at.elapsed() < self.ttl);
        }

        // If still at capacity, evict oldest
        if self.entries.len() >= self.max_entries {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.created_at)
                .map(|(k, _)| *k)
            {
                self.entries.remove(&oldest_key);
            }
        }

        self.entries.insert(
            args_hash,
            CacheEntry {
                value,
                created_at: Instant::now(),
            },
        );
    }
}

impl CacheHintHandler {
    pub fn new() -> Self {
        Self {
            caches: Mutex::new(HashMap::new()),
        }
    }

    fn extract_u64_from_map(value: &Value, key: &str) -> Option<u64> {
        if let Value::Map(map) = value {
            for (k, v) in map {
                let key_str = match k {
                    rtfs::ast::MapKey::Keyword(kw) => &kw.0,
                    rtfs::ast::MapKey::String(s) => s,
                    _ => continue,
                };
                if key_str == key {
                    return match v {
                        Value::Integer(i) => Some(*i as u64),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    /// Simple hash function for Value (for cache key)
    fn hash_args(args: &[Value]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        for arg in args {
            // Use debug representation for hashing
            format!("{:?}", arg).hash(&mut hasher);
        }
        hasher.finish()
    }
}

impl Default for CacheHintHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl HintHandler for CacheHintHandler {
    fn hint_key(&self) -> &str {
        "runtime.learning.cache"
    }

    fn priority(&self) -> u32 {
        2 // Earliest - check cache before anything else
    }

    fn validate_hint(&self, hint_value: &Value) -> RuntimeResult<()> {
        if let Value::Map(_) = hint_value {
            Ok(())
        } else {
            Err(RuntimeError::Generic(
                "cache hint value must be a map".to_string(),
            ))
        }
    }

    fn apply<'a>(
        &'a self,
        host_call: &'a HostCall,
        hint_value: &'a Value,
        ctx: &'a ExecutionContext,
        next: NextExecutor<'a>,
    ) -> BoxFuture<'a, RuntimeResult<Value>> {
        Box::pin(async move {
            let ttl_ms = Self::extract_u64_from_map(hint_value, "ttl-ms").unwrap_or(60000);
            let max_entries =
                Self::extract_u64_from_map(hint_value, "max-entries").unwrap_or(100) as usize;

            let capability_id = host_call.capability_id.clone();
            let args_hash = Self::hash_args(&host_call.args);

            // Check cache
            {
                let caches = self.caches.lock().unwrap();
                if let Some(cache) = caches.get(&capability_id) {
                    if let Some(cached_value) = cache.get(args_hash) {
                        // Cache hit!
                        if let Ok(mut chain) = ctx.causal_chain.lock() {
                            let _ = chain.append(
                                &Action::new(
                                    ActionType::HintApplied,
                                    format!("capability:{}", host_call.capability_id),
                                    String::new(),
                                )
                                .with_metadata("hint", "cache:HIT"),
                            );
                        }
                        return Ok(cached_value.clone());
                    }
                }
            }

            // Cache miss - execute
            if let Ok(mut chain) = ctx.causal_chain.lock() {
                let _ = chain.append(
                    &Action::new(
                        ActionType::HintApplied,
                        format!("capability:{}", host_call.capability_id),
                        String::new(),
                    )
                    .with_metadata("hint", "cache:MISS"),
                );
            }

            let result = next().await;

            // Store in cache if successful
            if let Ok(ref value) = result {
                let mut caches = self.caches.lock().unwrap();
                let cache = caches
                    .entry(capability_id)
                    .or_insert_with(|| CapabilityCache::new(max_entries, ttl_ms));
                cache.put(args_hash, value.clone());
            }

            result
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_handler_key_and_priority() {
        let handler = CacheHintHandler::new();
        assert_eq!(handler.hint_key(), "runtime.learning.cache");
        assert_eq!(handler.priority(), 2); // Very early
    }

    #[test]
    fn test_args_hash_different() {
        let args1 = vec![Value::Integer(1), Value::String("a".to_string())];
        let args2 = vec![Value::Integer(2), Value::String("a".to_string())];
        assert_ne!(
            CacheHintHandler::hash_args(&args1),
            CacheHintHandler::hash_args(&args2)
        );
    }

    #[test]
    fn test_args_hash_same() {
        let args1 = vec![Value::Integer(1), Value::String("a".to_string())];
        let args2 = vec![Value::Integer(1), Value::String("a".to_string())];
        assert_eq!(
            CacheHintHandler::hash_args(&args1),
            CacheHintHandler::hash_args(&args2)
        );
    }
}
