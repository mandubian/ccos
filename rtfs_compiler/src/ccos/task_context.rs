//! Task Context Implementation
//!
//! This module implements the Task Context system that provides context propagation
//! across execution, including the @context-key syntax support.

use super::types::{ContextKey, Intent, IntentId};
use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Context store for key-value storage
#[derive(Clone)]
pub struct ContextStore {
    storage: HashMap<ContextKey, ContextValue>,
    types: HashMap<ContextKey, ContextType>,
    access_control: AccessControl,
}

impl ContextStore {
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
            types: HashMap::new(),
            access_control: AccessControl::new(),
        }
    }

    pub fn set_value(&mut self, key: ContextKey, value: ContextValue) -> Result<(), RuntimeError> {
        let context_type = ContextType::from_value(&value);
        self.types.insert(key.clone(), context_type);
        self.storage.insert(key, value);
        Ok(())
    }

    pub fn get_value(&self, key: &ContextKey) -> Option<&ContextValue> {
        self.storage.get(key)
    }

    pub fn get_value_mut(&mut self, key: &ContextKey) -> Option<&mut ContextValue> {
        self.storage.get_mut(key)
    }

    pub fn get_type(&self, key: &ContextKey) -> Option<&ContextType> {
        self.types.get(key)
    }

    pub fn remove_value(&mut self, key: &ContextKey) -> Option<ContextValue> {
        self.types.remove(key);
        self.storage.remove(key)
    }

    pub fn has_key(&self, key: &ContextKey) -> bool {
        self.storage.contains_key(key)
    }

    pub fn get_all_keys(&self) -> Vec<&ContextKey> {
        self.storage.keys().collect()
    }

    pub fn clear(&mut self) {
        self.storage.clear();
        self.types.clear();
    }

    pub fn size(&self) -> usize {
        self.storage.len()
    }
}

/// Context value with metadata
#[derive(Debug, Clone)]
pub struct ContextValue {
    pub value: Value,
    pub created_at: u64,
    pub updated_at: u64,
    pub access_count: u64,
    pub metadata: HashMap<String, Value>,
}

impl ContextValue {
    pub fn new(value: Value) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            value,
            created_at: now,
            updated_at: now,
            access_count: 0,
            metadata: HashMap::new(),
        }
    }

    pub fn with_metadata(mut self, key: String, value: Value) -> Self {
        self.metadata.insert(key, value);
        self
    }

    pub fn update_value(&mut self, value: Value) {
        self.value = value;
        self.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.access_count += 1;
    }

    pub fn increment_access(&mut self) {
        self.access_count += 1;
    }
}

/// Context type information
#[derive(Debug, Clone)]
pub struct ContextType {
    pub base_type: String,
    pub is_mutable: bool,
    pub is_persistent: bool,
    pub validation_rules: Vec<String>,
}

impl ContextType {
    pub fn from_value(value: &ContextValue) -> Self {
        let base_type = match &value.value {
            Value::Integer(_) | Value::Float(_) => "number",
            Value::String(_) => "string",
            Value::Boolean(_) => "boolean",
            Value::Vector(_) => "vector",
            Value::Map(_) => "map",
            Value::Nil => "nil",
            Value::Function(_) => "function",
            _ => "unknown",
        }
        .to_string();

        Self {
            base_type,
            is_mutable: true,
            is_persistent: false,
            validation_rules: Vec::new(),
        }
    }
}

/// Access control for context keys
#[derive(Clone)]
pub struct AccessControl {
    permissions: HashMap<ContextKey, ContextPermissions>,
    default_permissions: ContextPermissions,
}

impl AccessControl {
    pub fn new() -> Self {
        Self {
            permissions: HashMap::new(),
            default_permissions: ContextPermissions::default(),
        }
    }

    pub fn set_permissions(&mut self, key: ContextKey, permissions: ContextPermissions) {
        self.permissions.insert(key, permissions);
    }

    pub fn get_permissions(&self, key: &ContextKey) -> &ContextPermissions {
        self.permissions
            .get(key)
            .unwrap_or(&self.default_permissions)
    }

    pub fn can_read(&self, key: &ContextKey) -> bool {
        self.get_permissions(key).can_read
    }

    pub fn can_write(&self, key: &ContextKey) -> bool {
        self.get_permissions(key).can_write
    }

    pub fn can_delete(&self, key: &ContextKey) -> bool {
        self.get_permissions(key).can_delete
    }
}

/// Permissions for a context key
#[derive(Debug, Clone)]
pub struct ContextPermissions {
    pub can_read: bool,
    pub can_write: bool,
    pub can_delete: bool,
    pub can_propagate: bool,
}

impl Default for ContextPermissions {
    fn default() -> Self {
        Self {
            can_read: true,
            can_write: true,
            can_delete: true,
            can_propagate: true,
        }
    }
}

/// Context propagation system
pub struct ContextPropagation {
    propagation_rules: HashMap<ContextKey, PropagationRule>,
    propagation_history: Vec<PropagationEvent>,
}

impl ContextPropagation {
    pub fn new() -> Self {
        Self {
            propagation_rules: HashMap::new(),
            propagation_history: Vec::new(),
        }
    }

    pub fn set_propagation_rule(&mut self, key: ContextKey, rule: PropagationRule) {
        self.propagation_rules.insert(key, rule);
    }

    pub fn propagate_context(
        &mut self,
        source: &ContextStore,
        target: &mut ContextStore,
    ) -> Result<(), RuntimeError> {
        for (key, value) in &source.storage {
            if let Some(rule) = self.propagation_rules.get(key) {
                if rule.should_propagate(value) {
                    target.set_value(key.clone(), value.clone())?;

                    // Record propagation event
                    let event = PropagationEvent {
                        key: key.clone(),
                        source: "source".to_string(),
                        target: "target".to_string(),
                        timestamp: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                    };
                    self.propagation_history.push(event);
                }
            }
        }
        Ok(())
    }

    pub fn get_propagation_history(&self) -> &[PropagationEvent] {
        &self.propagation_history
    }
}

/// Propagation rule for a context key
#[derive(Debug, Clone)]
pub struct PropagationRule {
    pub propagate_always: bool,
    pub propagate_if_changed: bool,
    pub propagate_if_accessed: bool,
    pub max_propagations: Option<usize>,
}

impl PropagationRule {
    pub fn new() -> Self {
        Self {
            propagate_always: false,
            propagate_if_changed: true,
            propagate_if_accessed: false,
            max_propagations: None,
        }
    }

    pub fn should_propagate(&self, value: &ContextValue) -> bool {
        if self.propagate_always {
            return true;
        }

        if self.propagate_if_changed && value.access_count > 0 {
            return true;
        }

        if self.propagate_if_accessed && value.access_count > 1 {
            return true;
        }

        false
    }
}

/// Propagation event record
#[derive(Debug, Clone)]
pub struct PropagationEvent {
    pub key: ContextKey,
    pub source: String,
    pub target: String,
    pub timestamp: u64,
}

/// Context-aware execution
pub struct ContextAwareExecution {
    context_stack: Vec<ContextFrame>,
    current_frame: Option<ContextFrame>,
}

impl ContextAwareExecution {
    pub fn new() -> Self {
        Self {
            context_stack: Vec::new(),
            current_frame: None,
        }
    }

    pub fn push_context(&mut self, frame: ContextFrame) {
        if let Some(current) = self.current_frame.take() {
            self.context_stack.push(current);
        }
        self.current_frame = Some(frame);
    }

    pub fn pop_context(&mut self) -> Option<ContextFrame> {
        let current = self.current_frame.take();
        self.current_frame = self.context_stack.pop();
        current
    }

    pub fn get_current_context(&self) -> Option<&ContextFrame> {
        self.current_frame.as_ref()
    }

    pub fn resolve_context_key(&self, key: &ContextKey) -> Option<&ContextValue> {
        // Search current frame first
        if let Some(frame) = &self.current_frame {
            if let Some(value) = frame.context.get_value(key) {
                return Some(value);
            }
        }

        // Search stack from top to bottom
        for frame in self.context_stack.iter().rev() {
            if let Some(value) = frame.context.get_value(key) {
                return Some(value);
            }
        }

        None
    }

    pub fn set_context_key(
        &mut self,
        key: ContextKey,
        value: ContextValue,
    ) -> Result<(), RuntimeError> {
        if let Some(frame) = &mut self.current_frame {
            frame.context.set_value(key, value)
        } else {
            Err(RuntimeError::new("No active context frame"))
        }
    }
}

/// Context frame for execution scope
#[derive(Clone)]
pub struct ContextFrame {
    pub frame_id: String,
    pub context: ContextStore,
    pub parent_frame_id: Option<String>,
    pub created_at: u64,
    pub metadata: HashMap<String, Value>,
}

impl ContextFrame {
    pub fn new(frame_id: String) -> Self {
        Self {
            frame_id,
            context: ContextStore::new(),
            parent_frame_id: None,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_parent(mut self, parent_frame_id: String) -> Self {
        self.parent_frame_id = Some(parent_frame_id);
        self
    }
}

/// Context persistence for long-term storage
pub struct ContextPersistence {
    storage: HashMap<String, PersistedContext>,
    persistence_rules: HashMap<ContextKey, PersistenceRule>,
}

impl ContextPersistence {
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
            persistence_rules: HashMap::new(),
        }
    }

    pub fn persist_context(
        &mut self,
        context_id: String,
        context: &ContextStore,
    ) -> Result<(), RuntimeError> {
        let persisted = PersistedContext {
            context_id: context_id.clone(),
            context: context.clone(),
            persisted_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.storage.insert(context_id, persisted);
        Ok(())
    }

    pub fn load_context(&self, context_id: &str) -> Option<&PersistedContext> {
        self.storage.get(context_id)
    }

    pub fn set_persistence_rule(&mut self, key: ContextKey, rule: PersistenceRule) {
        self.persistence_rules.insert(key, rule);
    }

    pub fn should_persist(&self, key: &ContextKey, value: &ContextValue) -> bool {
        if let Some(rule) = self.persistence_rules.get(key) {
            rule.should_persist(value)
        } else {
            false
        }
    }
}

/// Persisted context
#[derive(Clone)]
pub struct PersistedContext {
    pub context_id: String,
    pub context: ContextStore,
    pub persisted_at: u64,
}

/// Persistence rule
#[derive(Debug, Clone)]
pub struct PersistenceRule {
    pub persist_always: bool,
    pub persist_if_accessed: bool,
    pub persist_if_changed: bool,
    pub ttl_seconds: Option<u64>,
}

impl PersistenceRule {
    pub fn new() -> Self {
        Self {
            persist_always: false,
            persist_if_accessed: true,
            persist_if_changed: true,
            ttl_seconds: None,
        }
    }

    pub fn should_persist(&self, value: &ContextValue) -> bool {
        if self.persist_always {
            return true;
        }

        if self.persist_if_accessed && value.access_count > 0 {
            return true;
        }

        if self.persist_if_changed && value.access_count > 1 {
            return true;
        }

        false
    }
}

/// Main Task Context implementation
pub struct TaskContext {
    context_store: ContextStore,
    propagation: ContextPropagation,
    execution: ContextAwareExecution,
    persistence: ContextPersistence,
}

impl TaskContext {
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            context_store: ContextStore::new(),
            propagation: ContextPropagation::new(),
            execution: ContextAwareExecution::new(),
            persistence: ContextPersistence::new(),
        })
    }

    /// Set a context value
    pub fn set_context(&mut self, key: ContextKey, value: Value) -> Result<(), RuntimeError> {
        let context_value = ContextValue::new(value);
        self.context_store.set_value(key, context_value)
    }

    /// Get a context value
    pub fn get_context(&self, key: &ContextKey) -> Option<&Value> {
        self.context_store.get_value(key).map(|cv| &cv.value)
    }

    /// Get a context value with metadata
    pub fn get_context_value(&self, key: &ContextKey) -> Option<&ContextValue> {
        self.context_store.get_value(key)
    }

    /// Update a context value
    pub fn update_context(&mut self, key: &ContextKey, value: Value) -> Result<(), RuntimeError> {
        if let Some(context_value) = self.context_store.get_value_mut(key) {
            context_value.update_value(value);
            Ok(())
        } else {
            Err(RuntimeError::new(&format!(
                "Context key '{}' not found",
                key
            )))
        }
    }

    /// Remove a context value
    pub fn remove_context(&mut self, key: &ContextKey) -> Option<Value> {
        self.context_store.remove_value(key).map(|cv| cv.value)
    }

    /// Check if a context key exists
    pub fn has_context(&self, key: &ContextKey) -> bool {
        self.context_store.has_key(key)
    }

    /// Get all context keys
    pub fn get_all_keys(&self) -> Vec<&ContextKey> {
        self.context_store.get_all_keys()
    }

    /// Push a new execution context
    pub fn push_execution_context(&mut self, frame_id: String) {
        let frame = ContextFrame::new(frame_id);
        self.execution.push_context(frame);
    }

    /// Pop the current execution context
    pub fn pop_execution_context(&mut self) -> Option<ContextFrame> {
        self.execution.pop_context()
    }

    /// Resolve a context key in the current execution context
    pub fn resolve_context_key(&self, key: &ContextKey) -> Option<&ContextValue> {
        self.execution.resolve_context_key(key)
    }

    /// Set a context key in the current execution context
    pub fn set_execution_context(
        &mut self,
        key: ContextKey,
        value: ContextValue,
    ) -> Result<(), RuntimeError> {
        self.execution.set_context_key(key, value)
    }

    /// Propagate context to another context store
    pub fn propagate_to(&mut self, target: &mut ContextStore) -> Result<(), RuntimeError> {
        self.propagation
            .propagate_context(&self.context_store, target)
    }

    /// Persist the current context
    pub fn persist_context(&mut self, context_id: String) -> Result<(), RuntimeError> {
        self.persistence
            .persist_context(context_id, &self.context_store)
    }

    /// Load a persisted context
    pub fn load_persisted_context(&self, context_id: &str) -> Option<&PersistedContext> {
        self.persistence.load_context(context_id)
    }

    /// Clear all context
    pub fn clear(&mut self) {
        self.context_store.clear();
    }

    /// Get context size
    pub fn size(&self) -> usize {
        self.context_store.size()
    }

    /// Get propagation history
    pub fn get_propagation_history(&self) -> &[PropagationEvent] {
        self.propagation.get_propagation_history()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::values::Value;

    #[test]
    fn test_task_context_creation() {
        let context = TaskContext::new();
        assert!(context.is_ok());
    }

    #[test]
    fn test_context_set_and_get() {
        let mut context = TaskContext::new().unwrap();

        assert!(context
            .set_context("test_key".to_string(), Value::Float(42.0))
            .is_ok());
        assert_eq!(
            context.get_context(&"test_key".to_string()),
            Some(&Value::Float(42.0))
        );
    }

    #[test]
    fn test_context_update() {
        let mut context = TaskContext::new().unwrap();

        context
            .set_context("test_key".to_string(), Value::Float(42.0))
            .unwrap();
        context
            .update_context(&"test_key".to_string(), Value::Float(100.0))
            .unwrap();

        assert_eq!(
            context.get_context(&"test_key".to_string()),
            Some(&Value::Float(100.0))
        );
    }

    #[test]
    fn test_execution_context() {
        let mut context = TaskContext::new().unwrap();

        context.push_execution_context("frame1".to_string());
        context
            .set_execution_context(
                "exec_key".to_string(),
                ContextValue::new(Value::String("exec_value".to_string())),
            )
            .unwrap();

        assert!(context
            .resolve_context_key(&"exec_key".to_string())
            .is_some());

        let frame = context.pop_execution_context();
        assert!(frame.is_some());
        assert_eq!(frame.unwrap().frame_id, "frame1");
    }

    #[test]
    fn test_context_persistence() {
        let mut context = TaskContext::new().unwrap();

        context
            .set_context(
                "persist_key".to_string(),
                Value::String("persist_value".to_string()),
            )
            .unwrap();
        context.persist_context("test_context".to_string()).unwrap();

        let persisted = context.load_persisted_context("test_context");
        assert!(persisted.is_some());
        assert_eq!(persisted.unwrap().context_id, "test_context");
    }
}
