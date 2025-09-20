//! State Provider Abstraction for RTFS 2.0 Host-Backed State
//! 
//! This module provides a trait-based abstraction for state providers,
//! enabling easy integration with different backend systems (Redis, databases, etc.)
//! while maintaining the current mock implementations.

use crate::runtime::values::Value;
use crate::runtime::RuntimeError;
use std::collections::HashMap;

/// Result type for state provider operations
pub type StateResult<T> = Result<T, RuntimeError>;

/// Trait for state providers that can handle RTFS 2.0 host-backed state operations
/// 
/// This trait abstracts the implementation details of state storage,
/// allowing different backends (mock, Redis, database) to be plugged in seamlessly.
#[async_trait::async_trait]
pub trait StateProvider: Send + Sync {
    /// Get a value from the key-value store
    async fn kv_get(&self, key: &str) -> StateResult<Option<Value>>;
    
    /// Put a value in the key-value store
    async fn kv_put(&self, key: &str, value: Value) -> StateResult<()>;
    
    /// Compare-and-swap operation for atomic updates
    async fn kv_cas_put(&self, key: &str, expected: Option<Value>, new_value: Value) -> StateResult<bool>;
    
    /// Increment a counter atomically
    async fn counter_inc(&self, key: &str, increment: i64) -> StateResult<i64>;
    
    /// Append an event to an event log
    async fn event_append(&self, key: &str, event: Value) -> StateResult<()>;
    
    /// Get all events for a given key
    async fn event_get_all(&self, key: &str) -> StateResult<Vec<Value>>;
}

/// Mock implementation of StateProvider for development and testing
/// 
/// This implementation uses in-memory data structures to simulate
/// state operations without requiring external dependencies.
pub struct MockStateProvider {
    kv_store: std::sync::RwLock<HashMap<String, Value>>,
    counters: std::sync::RwLock<HashMap<String, i64>>,
    event_logs: std::sync::RwLock<HashMap<String, Vec<Value>>>,
}

impl MockStateProvider {
    /// Create a new MockStateProvider with empty state
    pub fn new() -> Self {
        Self {
            kv_store: std::sync::RwLock::new(HashMap::new()),
            counters: std::sync::RwLock::new(HashMap::new()),
            event_logs: std::sync::RwLock::new(HashMap::new()),
        }
    }
    
    /// Create a new MockStateProvider with initial state for testing
    pub fn with_initial_state(
        kv_data: HashMap<String, Value>,
        counters: HashMap<String, i64>,
        events: HashMap<String, Vec<Value>>,
    ) -> Self {
        Self {
            kv_store: std::sync::RwLock::new(kv_data),
            counters: std::sync::RwLock::new(counters),
            event_logs: std::sync::RwLock::new(events),
        }
    }
}

impl Default for MockStateProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl StateProvider for MockStateProvider {
    async fn kv_get(&self, key: &str) -> StateResult<Option<Value>> {
        let store = self.kv_store.read().map_err(|_| {
            RuntimeError::Generic("Failed to acquire read lock on KV store".to_string())
        })?;
        Ok(store.get(key).cloned())
    }
    
    async fn kv_put(&self, key: &str, value: Value) -> StateResult<()> {
        let mut store = self.kv_store.write().map_err(|_| {
            RuntimeError::Generic("Failed to acquire write lock on KV store".to_string())
        })?;
        store.insert(key.to_string(), value);
        Ok(())
    }
    
    async fn kv_cas_put(&self, key: &str, expected: Option<Value>, new_value: Value) -> StateResult<bool> {
        let mut store = self.kv_store.write().map_err(|_| {
            RuntimeError::Generic("Failed to acquire write lock on KV store".to_string())
        })?;
        
        let current = store.get(key).cloned();
        if current == expected {
            store.insert(key.to_string(), new_value);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    async fn counter_inc(&self, key: &str, increment: i64) -> StateResult<i64> {
        let mut counters = self.counters.write().map_err(|_| {
            RuntimeError::Generic("Failed to acquire write lock on counters".to_string())
        })?;
        
        let current = counters.get(key).copied().unwrap_or(0);
        let new_value = current + increment;
        counters.insert(key.to_string(), new_value);
        Ok(new_value)
    }
    
    async fn event_append(&self, key: &str, event: Value) -> StateResult<()> {
        let mut logs = self.event_logs.write().map_err(|_| {
            RuntimeError::Generic("Failed to acquire write lock on event logs".to_string())
        })?;
        
        logs.entry(key.to_string())
            .or_insert_with(Vec::new)
            .push(event);
        Ok(())
    }
    
    async fn event_get_all(&self, key: &str) -> StateResult<Vec<Value>> {
        let logs = self.event_logs.read().map_err(|_| {
            RuntimeError::Generic("Failed to acquire read lock on event logs".to_string())
        })?;
        
        Ok(logs.get(key).cloned().unwrap_or_default())
    }
}

/// State provider registry for managing different providers
pub struct StateProviderRegistry {
    provider: Box<dyn StateProvider>,
}

impl StateProviderRegistry {
    /// Create a new registry with the given provider
    pub fn new(provider: Box<dyn StateProvider>) -> Self {
        Self { provider }
    }
    
    /// Create a registry with the default mock provider
    pub fn with_mock() -> Self {
        Self::new(Box::new(MockStateProvider::new()))
    }
    
    /// Get the current state provider
    pub fn provider(&self) -> &dyn StateProvider {
        self.provider.as_ref()
    }
    
    /// Replace the state provider (useful for testing or runtime switching)
    pub fn set_provider(&mut self, provider: Box<dyn StateProvider>) {
        self.provider = provider;
    }
}

impl Default for StateProviderRegistry {
    fn default() -> Self {
        Self::with_mock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::values::Value;
    
    #[tokio::test]
    async fn test_mock_state_provider_kv_operations() {
        let provider = MockStateProvider::new();
        
        // Test KV operations
        let key = "test-key";
        let value = Value::String("test-value".to_string());
        
        // Initially empty
        assert_eq!(provider.kv_get(key).await.unwrap(), None);
        
        // Put a value
        provider.kv_put(key, value.clone()).await.unwrap();
        
        // Get the value back
        assert_eq!(provider.kv_get(key).await.unwrap(), Some(value));
    }
    
    #[tokio::test]
    async fn test_mock_state_provider_cas_operations() {
        let provider = MockStateProvider::new();
        let key = "cas-test";
        let initial_value = Value::Number(42.0);
        let new_value = Value::Number(84.0);
        
        // CAS with None (key doesn't exist) should succeed
        let result = provider.kv_cas_put(key, None, initial_value.clone()).await.unwrap();
        assert!(result);
        assert_eq!(provider.kv_get(key).await.unwrap(), Some(initial_value.clone()));
        
        // CAS with correct expected value should succeed
        let result = provider.kv_cas_put(key, Some(initial_value), new_value.clone()).await.unwrap();
        assert!(result);
        assert_eq!(provider.kv_get(key).await.unwrap(), Some(new_value));
        
        // CAS with wrong expected value should fail
        let result = provider.kv_cas_put(key, Some(Value::Number(100.0)), Value::Number(200.0)).await.unwrap();
        assert!(!result);
        assert_eq!(provider.kv_get(key).await.unwrap(), Some(new_value));
    }
    
    #[tokio::test]
    async fn test_mock_state_provider_counter_operations() {
        let provider = MockStateProvider::new();
        let key = "counter-test";
        
        // Initial increment from 0
        let result = provider.counter_inc(key, 5).await.unwrap();
        assert_eq!(result, 5);
        
        // Another increment
        let result = provider.counter_inc(key, 3).await.unwrap();
        assert_eq!(result, 8);
        
        // Negative increment
        let result = provider.counter_inc(key, -2).await.unwrap();
        assert_eq!(result, 6);
    }
    
    #[tokio::test]
    async fn test_mock_state_provider_event_operations() {
        let provider = MockStateProvider::new();
        let key = "event-log";
        let event1 = Value::String("event1".to_string());
        let event2 = Value::String("event2".to_string());
        
        // Append events
        provider.event_append(key, event1.clone()).await.unwrap();
        provider.event_append(key, event2.clone()).await.unwrap();
        
        // Get all events
        let events = provider.event_get_all(key).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], event1);
        assert_eq!(events[1], event2);
        
        // Get events for non-existent key
        let empty_events = provider.event_get_all("non-existent").await.unwrap();
        assert!(empty_events.is_empty());
    }
    
    #[tokio::test]
    async fn test_state_provider_registry() {
        let mut registry = StateProviderRegistry::with_mock();
        
        // Test that we can get the provider
        let provider = registry.provider();
        
        // Test basic operation
        provider.kv_put("test", Value::String("value".to_string())).await.unwrap();
        assert_eq!(provider.kv_get("test").await.unwrap(), Some(Value::String("value".to_string())));
        
        // Test provider replacement
        let new_provider = MockStateProvider::with_initial_state(
            [("pre-existing".to_string(), Value::String("pre-value".to_string()))].iter().cloned().collect(),
            HashMap::new(),
            HashMap::new(),
        );
        registry.set_provider(Box::new(new_provider));
        
        // Should have the pre-existing value
        assert_eq!(registry.provider().kv_get("pre-existing").await.unwrap(), 
                  Some(Value::String("pre-value".to_string())));
        
        // Should not have the old value
        assert_eq!(registry.provider().kv_get("test").await.unwrap(), None);
    }
}
