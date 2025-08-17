//! Hierarchical Execution Context Management
//!
//! This module implements the hierarchical execution context management system for CCOS.
//! It provides context inheritance, data propagation, isolation, and checkpoint/resume capabilities.
//!
//! The system supports:
//! - Hierarchical context stack with parent-child relationships
//! - Automatic context inheritance and data propagation
//! - Context isolation for parallel execution
//! - Context serialization and checkpoint/resume capability
//! - Integration with RTFS evaluator and step special forms

use crate::runtime::values::Value;
use crate::runtime::error::{RuntimeResult, RuntimeError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Deep-merge helper for runtime values
fn deep_merge_values(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Map(dst), Value::Map(src)) => {
            for (key, src_val) in src.iter() {
                if let Some(dst_val) = dst.get_mut(key) {
                    deep_merge_values(dst_val, src_val);
                } else {
                    dst.insert(key.clone(), src_val.clone());
                }
            }
        }
        (Value::Vector(dst_vec), Value::Vector(src_vec)) => {
            dst_vec.extend(src_vec.clone());
        }
        // For other types or mismatched kinds, source overwrites target
        (dst, src) => {
            *dst = src.clone();
        }
    }
}

/// Metadata associated with an execution context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetadata {
    pub created_at: u64,
    pub step_name: Option<String>,
    pub step_id: Option<String>,
    pub checkpoint_id: Option<String>,
    pub is_parallel: bool,
    pub isolation_level: IsolationLevel,
    pub tags: HashMap<String, String>,
}

/// Defines the isolation level for context data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationLevel {
    /// Inherit all parent data, can modify own data
    Inherit,
    /// Isolated from siblings, can only read parent data
    Isolated,
    /// Completely isolated, no parent data access
    Sandboxed,
}

/// Represents a single execution context in the hierarchy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub data: HashMap<String, Value>,
    pub metadata: ContextMetadata,
    pub children: Vec<String>, // IDs of child contexts
}

impl ExecutionContext {
    /// Creates a new execution context
    pub fn new(step_name: Option<String>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        Self {
            id: Uuid::new_v4().to_string(),
            parent_id: None,
            data: HashMap::new(),
            metadata: ContextMetadata {
                created_at: now,
                step_name,
                step_id: None,
                checkpoint_id: None,
                is_parallel: false,
                isolation_level: IsolationLevel::Inherit,
                tags: HashMap::new(),
            },
            children: Vec::new(),
        }
    }

    /// Creates a child context that inherits from this one
    pub fn create_child(&mut self, step_name: Option<String>, isolation_level: IsolationLevel) -> Self {
        let mut child = ExecutionContext::new(step_name);
        child.parent_id = Some(self.id.clone());
        child.metadata.isolation_level = isolation_level;
        
        // Add child to parent's children list
        self.children.push(child.id.clone());
        
        child
    }

    /// Sets a value in the context data
    pub fn set(&mut self, key: String, value: Value) {
        self.data.insert(key, value);
    }

    /// Gets a value from the context data
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// Checks if the context has a specific key
    pub fn has_key(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Merges data from another context into this one
    pub fn merge_data(&mut self, other: &ExecutionContext, conflict_resolution: ConflictResolution) {
        for (key, value) in &other.data {
            match conflict_resolution {
                ConflictResolution::KeepExisting => {
                    if !self.data.contains_key(key) {
                        self.data.insert(key.clone(), value.clone());
                    }
                }
                ConflictResolution::Overwrite => {
                    self.data.insert(key.clone(), value.clone());
                }
                ConflictResolution::Merge => {
                    if let Some(existing) = self.data.get_mut(key) {
                        deep_merge_values(existing, value);
                    } else {
                        self.data.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    /// Creates a checkpoint of the current context state
    pub fn create_checkpoint(&mut self, checkpoint_id: String) {
        self.metadata.checkpoint_id = Some(checkpoint_id);
    }

    /// Serializes the context for storage or transmission
    pub fn serialize(&self) -> RuntimeResult<String> {
        serde_json::to_string(self)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize context: {}", e)))
    }

    /// Deserializes a context from a string
    pub fn deserialize(data: &str) -> RuntimeResult<Self> {
        serde_json::from_str(data)
            .map_err(|e| RuntimeError::Generic(format!("Failed to deserialize context: {}", e)))
    }
}

/// Defines how to resolve conflicts when merging context data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    KeepExisting,
    Overwrite,
    Merge,
}

/// Manages the hierarchical stack of execution contexts
#[derive(Debug, Clone)]
pub struct ContextStack {
    contexts: HashMap<String, ExecutionContext>,
    current_id: Option<String>,
    root_id: Option<String>,
}

impl ContextStack {
    /// Creates a new empty context stack
    pub fn new() -> Self {
        Self {
            contexts: HashMap::new(),
            current_id: None,
            root_id: None,
        }
    }

    /// Creates a new context stack with a root context
    pub fn with_root(step_name: Option<String>) -> Self {
        let mut stack = Self::new();
        let root_id = step_name.clone().unwrap_or_else(|| "root".to_string());
        let mut root_context = ExecutionContext::new(step_name);
        root_context.id = root_id.clone();
        stack.contexts.insert(root_id.clone(), root_context);
        stack.current_id = Some(root_id.clone());
        stack.root_id = Some(root_id);
        stack
    }

    /// Gets the current context
    pub fn current(&self) -> Option<&ExecutionContext> {
        self.current_id.as_ref().and_then(|id| self.contexts.get(id))
    }

    /// Gets a mutable reference to the current context
    pub fn current_mut(&mut self) -> Option<&mut ExecutionContext> {
        self.current_id.as_ref().and_then(|id| self.contexts.get_mut(id))
    }

    /// Gets a context by ID
    pub fn get(&self, context_id: &str) -> Option<&ExecutionContext> {
        self.contexts.get(context_id)
    }

    /// Gets a mutable reference to a context by ID
    pub fn get_mut(&mut self, context_id: &str) -> Option<&mut ExecutionContext> {
        self.contexts.get_mut(context_id)
    }

    /// Pushes a new context onto the stack
    pub fn push(&mut self, step_name: Option<String>, isolation_level: IsolationLevel) -> RuntimeResult<String> {
        // If the stack was never initialized with a root, create a default root
        if self.current_id.is_none() {
            let mut root = ExecutionContext::new(Some("root".to_string()));
            let root_id = "root".to_string();
            root.id = root_id.clone();
            self.contexts.insert(root_id.clone(), root);
            self.current_id = Some(root_id.clone());
            self.root_id = Some(root_id);
        }

        let current = self.current_mut()
            .ok_or_else(|| RuntimeError::Generic("No current context to create child from".to_string()))?;

        let child = current.create_child(step_name, isolation_level);
        let child_id = child.id.clone();
        
        self.contexts.insert(child_id.clone(), child);
        self.current_id = Some(child_id.clone());
        
        Ok(child_id)
    }

    /// Pops the current context from the stack
    pub fn pop(&mut self) -> RuntimeResult<Option<ExecutionContext>> {
        let current_id = self.current_id.take()
            .ok_or_else(|| RuntimeError::Generic("No current context to pop".to_string()))?;
        
        let current_context = self.contexts.remove(&current_id)
            .ok_or_else(|| RuntimeError::Generic("Current context not found in stack".to_string()))?;
        
        // Update parent's current_id
        if let Some(parent_id) = &current_context.parent_id {
            self.current_id = Some(parent_id.clone());
        }
        
        Ok(Some(current_context))
    }

    /// Looks up a value in the current context hierarchy
    pub fn lookup(&self, key: &str) -> Option<Value> {
        let mut current_id = self.current_id.as_ref()?;
        
        loop {
            if let Some(context) = self.contexts.get(current_id) {
                if let Some(value) = context.get(key) {
                    return Some(value.clone());
                }
                
                // Move to parent if available and isolation level allows
                if let Some(parent_id) = &context.parent_id {
                    if context.metadata.isolation_level != IsolationLevel::Sandboxed {
                        current_id = parent_id;
                        continue;
                    }
                }
            }
            break;
        }
        
        None
    }

    /// Sets a value in the current context
    pub fn set(&mut self, key: String, value: Value) -> RuntimeResult<()> {
        let current = self.current_mut()
            .ok_or_else(|| RuntimeError::Generic("No current context to set value in".to_string()))?;
        
        current.set(key, value);
        Ok(())
    }

    /// Creates a checkpoint of the current context state
    pub fn checkpoint(&mut self, checkpoint_id: String) -> RuntimeResult<()> {
        let current = self.current_mut()
            .ok_or_else(|| RuntimeError::Generic("No current context to checkpoint".to_string()))?;
        
        current.create_checkpoint(checkpoint_id);
        Ok(())
    }

    /// Serializes the entire context stack
    pub fn serialize(&self) -> RuntimeResult<String> {
        let stack_data = ContextStackData {
            contexts: self.contexts.clone(),
            current_id: self.current_id.clone(),
            root_id: self.root_id.clone(),
        };
        
        serde_json::to_string(&stack_data)
            .map_err(|e| RuntimeError::Generic(format!("Failed to serialize context stack: {}", e)))
    }

    /// Deserializes a context stack from a string
    pub fn deserialize(data: &str) -> RuntimeResult<Self> {
        let stack_data: ContextStackData = serde_json::from_str(data)
            .map_err(|e| RuntimeError::Generic(format!("Failed to deserialize context stack: {}", e)))?;
        
        Ok(Self {
            contexts: stack_data.contexts,
            current_id: stack_data.current_id,
            root_id: stack_data.root_id,
        })
    }

    /// Gets the depth of the current context in the hierarchy
    pub fn depth(&self) -> usize {
        let mut depth = 0;
        let mut current_id = self.current_id.as_ref();
        
        while let Some(id) = current_id {
            if let Some(context) = self.contexts.get(id) {
                depth += 1;
                current_id = context.parent_id.as_ref();
            } else {
                break;
            }
        }
        
        depth
    }

    /// Gets all ancestor contexts of the current context
    pub fn ancestors(&self) -> Vec<&ExecutionContext> {
        let mut ancestors = Vec::new();
        let mut current_id = self.current_id.as_ref();
        
        while let Some(id) = current_id {
            if let Some(context) = self.contexts.get(id) {
                if let Some(parent_id) = &context.parent_id {
                    if let Some(parent) = self.contexts.get(parent_id) {
                        ancestors.push(parent);
                        current_id = Some(parent_id);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        
        ancestors
    }

    /// Gets all sibling contexts of the current context
    pub fn siblings(&self) -> Vec<&ExecutionContext> {
        let mut siblings = Vec::new();
        
        if let Some(current) = self.current() {
            if let Some(parent_id) = &current.parent_id {
                if let Some(parent) = self.contexts.get(parent_id) {
                    for child_id in &parent.children {
                        if child_id != &current.id {
                            if let Some(sibling) = self.contexts.get(child_id) {
                                siblings.push(sibling);
                            }
                        }
                    }
                }
            }
        }
        
        siblings
    }

    /// Creates an isolated context for parallel execution
    pub fn create_parallel_context(&mut self, step_name: Option<String>) -> RuntimeResult<String> {
        let current = self.current_mut()
            .ok_or_else(|| RuntimeError::Generic("No current context to create parallel context from".to_string()))?;
        
        let mut parallel_context = current.create_child(step_name, IsolationLevel::Isolated);
        parallel_context.metadata.is_parallel = true;
        
        let parallel_id = parallel_context.id.clone();
        self.contexts.insert(parallel_id.clone(), parallel_context);
        
        Ok(parallel_id)
    }

    /// Switches to a different context
    pub fn switch_to(&mut self, context_id: &str) -> RuntimeResult<()> {
        if !self.contexts.contains_key(context_id) {
            return Err(RuntimeError::Generic(format!("Context {} not found", context_id)));
        }
        
        self.current_id = Some(context_id.to_string());
        Ok(())
    }

    /// Merges data from a child context back to its parent
    pub fn merge_child_to_parent(&mut self, child_id: &str, conflict_resolution: ConflictResolution) -> RuntimeResult<()> {
        let child_data = {
            let child = self.contexts.get(child_id)
                .ok_or_else(|| RuntimeError::Generic(format!("Child context {} not found", child_id)))?;
            
            let parent_id = child.parent_id.as_ref()
                .ok_or_else(|| RuntimeError::Generic("Child context has no parent".to_string()))?;
            
            (parent_id.clone(), child.data.clone())
        };
        
        let parent = self.contexts.get_mut(&child_data.0)
            .ok_or_else(|| RuntimeError::Generic(format!("Parent context {} not found", child_data.0)))?;
        
        for (key, value) in &child_data.1 {
            match conflict_resolution {
                ConflictResolution::KeepExisting => {
                    if !parent.data.contains_key(key) {
                        parent.data.insert(key.clone(), value.clone());
                    }
                }
                ConflictResolution::Overwrite => {
                    parent.data.insert(key.clone(), value.clone());
                }
                ConflictResolution::Merge => {
                    if let Some(existing) = parent.data.get_mut(key) {
                        deep_merge_values(existing, value);
                    } else {
                        parent.data.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Ok(())
    }
}

/// Internal data structure for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextStackData {
    contexts: HashMap<String, ExecutionContext>,
    current_id: Option<String>,
    root_id: Option<String>,
}

/// Context manager that integrates with the RTFS evaluator and CCOS components
#[derive(Debug, Clone)]
pub struct ContextManager {
    stack: ContextStack,
    checkpoint_interval: Option<u64>, // milliseconds
    last_checkpoint: u64,
}

impl ContextManager {
    /// Creates a new context manager
    pub fn new() -> Self {
        Self {
            stack: ContextStack::new(),
            checkpoint_interval: None,
            last_checkpoint: 0,
        }
    }

    /// Creates a new context manager with automatic checkpointing
    pub fn with_checkpointing(interval_ms: u64) -> Self {
        Self {
            stack: ContextStack::new(),
            checkpoint_interval: Some(interval_ms),
            last_checkpoint: 0,
        }
    }

    /// Initializes the context manager with a root context
    pub fn initialize(&mut self, step_name: Option<String>) {
        self.stack = ContextStack::with_root(step_name);
    }

    /// Enters a new step context
    pub fn enter_step(&mut self, step_name: &str, isolation_level: IsolationLevel) -> RuntimeResult<String> {
        let context_id = self.stack.push(Some(step_name.to_string()), isolation_level)?;
        
        // Set step_id in metadata
        if let Some(context) = self.stack.get_mut(&context_id) {
            context.metadata.step_id = Some(context_id.clone());
        }
        
        // Check if we need to create a checkpoint
        self.maybe_checkpoint()?;
        
        Ok(context_id)
    }

    /// Exits the current step context
    pub fn exit_step(&mut self) -> RuntimeResult<Option<ExecutionContext>> {
        self.stack.pop()
    }

    /// Gets a value from the current context hierarchy
    pub fn get(&self, key: &str) -> Option<Value> {
        self.stack.lookup(key)
    }

    /// Sets a value in the current context
    pub fn set(&mut self, key: String, value: Value) -> RuntimeResult<()> {
        self.stack.set(key, value)
    }

    /// Creates a manual checkpoint
    pub fn checkpoint(&mut self, checkpoint_id: String) -> RuntimeResult<()> {
        self.stack.checkpoint(checkpoint_id)
    }

    /// Serializes the current state for resumption
    pub fn serialize(&self) -> RuntimeResult<String> {
        self.stack.serialize()
    }

    /// Deserializes and restores the state
    pub fn deserialize(&mut self, data: &str) -> RuntimeResult<()> {
        self.stack = ContextStack::deserialize(data)?;
        Ok(())
    }

    /// Creates an isolated context for parallel execution
    pub fn create_parallel_context(&mut self, step_name: Option<String>) -> RuntimeResult<String> {
        self.stack.create_parallel_context(step_name)
    }

    /// Switches to a different context
    pub fn switch_to(&mut self, context_id: &str) -> RuntimeResult<()> {
        self.stack.switch_to(context_id)
    }

    /// Merge a child context to its parent using a conflict policy
    pub fn merge_child_to_parent(&mut self, child_id: &str, policy: ConflictResolution) -> RuntimeResult<()> {
        self.stack.merge_child_to_parent(child_id, policy)
    }

    /// Convenience: begin an isolated child context for a branch
    pub fn begin_isolated(&mut self, step_name: &str) -> RuntimeResult<String> {
        let child_id = self.stack.create_parallel_context(Some(step_name.to_string()))?;
        self.stack.switch_to(&child_id)?;
        Ok(child_id)
    }

    /// Convenience: end an isolated child context and merge back to parent
    pub fn end_isolated(&mut self, child_id: &str, policy: ConflictResolution) -> RuntimeResult<()> {
        // Capture parent before merge
        let parent_id_opt = self.stack
            .get(child_id)
            .and_then(|c| c.parent_id.clone());
        self.stack.merge_child_to_parent(child_id, policy)?;
        if let Some(parent_id) = parent_id_opt {
            // Switch current context back to parent after merge
            let _ = self.stack.switch_to(&parent_id);
        }
        Ok(())
    }

    /// Gets the current context depth
    pub fn depth(&self) -> usize {
        self.stack.depth()
    }

    /// Gets the current context ID
    pub fn current_context_id(&self) -> Option<&str> {
        self.stack.current_id.as_deref()
    }

    /// Checks if automatic checkpointing is needed
    fn maybe_checkpoint(&mut self) -> RuntimeResult<()> {
        if let Some(interval) = self.checkpoint_interval {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            
            if now - self.last_checkpoint >= interval {
                let checkpoint_id = format!("auto_{}", now);
                self.checkpoint(checkpoint_id)?;
                self.last_checkpoint = now;
            }
        }
        Ok(())
    }
}

impl Default for ContextManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let mut context = ExecutionContext::new(Some("test".to_string()));
        assert_eq!(context.metadata.step_name, Some("test".to_string()));
        assert_eq!(context.metadata.isolation_level, IsolationLevel::Inherit);
    }

    #[test]
    fn test_context_inheritance() {
        let mut parent = ExecutionContext::new(Some("parent".to_string()));
        parent.set("key".to_string(), Value::String("value".to_string()));
        
        let child = parent.create_child(Some("child".to_string()), IsolationLevel::Inherit);
        assert_eq!(child.parent_id, Some(parent.id));
        assert!(parent.children.contains(&child.id));
    }

    #[test]
    fn test_context_stack_operations() {
        let mut stack = ContextStack::with_root(Some("root".to_string()));
        
        // Push child context
        let child_id = stack.push(Some("child".to_string()), IsolationLevel::Inherit).unwrap();
        assert_eq!(stack.current().unwrap().metadata.step_name, Some("child".to_string()));
        
        // Set and get value
        stack.set("test_key".to_string(), Value::String("test_value".to_string())).unwrap();
        assert_eq!(stack.lookup("test_key"), Some(Value::String("test_value".to_string())));
        
        // Pop context
        let popped = stack.pop().unwrap().unwrap();
        assert_eq!(popped.metadata.step_name, Some("child".to_string()));
    }

    #[test]
    fn test_context_manager() {
        let mut manager = ContextManager::new();
        manager.initialize(Some("root".to_string()));
        
        let context_id = manager.enter_step("test_step", IsolationLevel::Inherit).unwrap();
        manager.set("key".to_string(), Value::String("value".to_string())).unwrap();
        
        assert_eq!(manager.get("key"), Some(Value::String("value".to_string())));
        assert_eq!(manager.current_context_id(), Some(context_id.as_str()));
        
        let exited = manager.exit_step().unwrap().unwrap();
        assert_eq!(exited.metadata.step_name, Some("test_step".to_string()));
    }

    #[test]
    fn test_serialization() {
        let mut context = ExecutionContext::new(Some("test".to_string()));
        context.set("key".to_string(), Value::String("value".to_string()));
        
        let serialized = context.serialize().unwrap();
        let deserialized = ExecutionContext::deserialize(&serialized).unwrap();
        
        assert_eq!(deserialized.get("key"), Some(&Value::String("value".to_string())));
    }
}
