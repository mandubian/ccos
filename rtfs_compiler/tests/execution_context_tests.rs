//! Tests for Hierarchical Execution Context Management

use rtfs_compiler::ccos::execution_context::{ContextManager, IsolationLevel, ConflictResolution};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeResult;
use rtfs_compiler::ast::MapKey;
use std::collections::HashMap;

#[test]
fn test_context_creation_and_basic_operations() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    
    // Initialize with root context
    manager.initialize(Some("root".to_string()));
    assert_eq!(manager.current_context_id(), Some("root"));
    
    // Set and get values
    manager.set("key1".to_string(), Value::String("value1".to_string()))?;
    assert_eq!(manager.get("key1"), Some(Value::String("value1".to_string())));
    
    // Test step creation
    let child_id = manager.enter_step("child1", IsolationLevel::Inherit)?;
    assert_eq!(manager.current_context_id(), Some(child_id.as_str()));
    
    // Child should inherit parent's values
    assert_eq!(manager.get("key1"), Some(Value::String("value1".to_string())));
    
    // Set value in child
    manager.set("key2".to_string(), Value::Integer(42))?;
    assert_eq!(manager.get("key2"), Some(Value::Integer(42)));
    
    // Switch back to parent
    manager.switch_to("root")?;
    assert_eq!(manager.current_context_id(), Some("root"));
    
    // Parent should not see child's values
    assert_eq!(manager.get("key2"), None);
    
    Ok(())
}

#[test]
fn test_context_isolation_levels() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    manager.set("shared".to_string(), Value::String("root_value".to_string()))?;
    
    // Test Inherit isolation
    let inherit_id = manager.enter_step("inherit", IsolationLevel::Inherit)?;
    manager.switch_to(&inherit_id)?;
    assert_eq!(manager.get("shared"), Some(Value::String("root_value".to_string())));
    
    // Test Isolated isolation
    manager.switch_to("root")?;
    let isolated_id = manager.enter_step("isolated", IsolationLevel::Isolated)?;
    manager.switch_to(&isolated_id)?;
    assert_eq!(manager.get("shared"), Some(Value::String("root_value".to_string()))); // Isolated can read parent data
    
    // Test Sandboxed isolation
    manager.switch_to("root")?;
    let sandboxed_id = manager.enter_step("sandboxed", IsolationLevel::Sandboxed)?;
    manager.switch_to(&sandboxed_id)?;
    assert_eq!(manager.get("shared"), None); // Sandboxed context doesn't inherit
    
    // Sandboxed context should allow setting values
    manager.set("new_key".to_string(), Value::Integer(1))?;
    assert_eq!(manager.get("new_key"), Some(Value::Integer(1)));
    
    Ok(())
}

#[test]
fn test_context_serialization() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    
    // Create simple data structure
    manager.set("timeout".to_string(), Value::Integer(30))?;
    manager.set("retries".to_string(), Value::Integer(3))?;
    
    // Serialize context
    let serialized = manager.serialize()?;
    assert!(!serialized.is_empty());
    
    // Create new manager and deserialize
    let mut new_manager = ContextManager::new();
    new_manager.initialize(Some("restored".to_string()));
    new_manager.deserialize(&serialized)?;
    
    // Verify data was restored
    let restored_timeout = new_manager.get("timeout");
    let restored_retries = new_manager.get("retries");
    assert_eq!(restored_timeout, Some(Value::Integer(30)));
    assert_eq!(restored_retries, Some(Value::Integer(3)));
    
    Ok(())
}

#[test]
fn test_context_checkpointing() -> RuntimeResult<()> {
    let mut manager = ContextManager::with_checkpointing(100); // 100ms interval
    manager.initialize(Some("root".to_string()));
    
    // Set some data
    manager.set("checkpoint_data".to_string(), Value::String("test_value".to_string()))?;
    
    // Trigger checkpoint
    manager.checkpoint("test_checkpoint".to_string())?;
    
    // Verify data is still accessible
    assert_eq!(manager.get("checkpoint_data"), Some(Value::String("test_value".to_string())));
    
    Ok(())
}

#[test]
fn test_parallel_context_execution() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    manager.set("root_data".to_string(), Value::Integer(100))?;
    
    // Create multiple isolated contexts for parallel execution
    let context1_id = manager.create_parallel_context(Some("parallel1".to_string()))?;
    let context2_id = manager.create_parallel_context(Some("parallel2".to_string()))?;
    
    // Simulate parallel execution by switching between contexts
    manager.switch_to(&context1_id)?;
    manager.set("parallel_data".to_string(), Value::String("context1_value".to_string()))?;
    
    manager.switch_to(&context2_id)?;
    manager.set("parallel_data".to_string(), Value::String("context2_value".to_string()))?;
    
    // Verify contexts are isolated
    manager.switch_to(&context1_id)?;
    assert_eq!(manager.get("parallel_data"), Some(Value::String("context1_value".to_string())));
    
    manager.switch_to(&context2_id)?;
    assert_eq!(manager.get("parallel_data"), Some(Value::String("context2_value".to_string())));
    
    // Verify root context is unchanged
    manager.switch_to("root")?;
    assert_eq!(manager.get("parallel_data"), None);
    assert_eq!(manager.get("root_data"), Some(Value::Integer(100)));
    
    Ok(())
}

#[test]
fn test_context_depth_tracking() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));
    
    assert_eq!(manager.depth(), 1);
    
    let child1_id = manager.enter_step("child1", IsolationLevel::Inherit)?;
    assert_eq!(manager.depth(), 2);
    
    let child2_id = manager.enter_step("child2", IsolationLevel::Inherit)?;
    assert_eq!(manager.depth(), 3);
    
    // Switch back to parent
    manager.switch_to("root")?;
    assert_eq!(manager.depth(), 1);
    
    Ok(())
}

#[test]
fn test_parent_wins_merge_policy_default() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));

    // Parent has an existing value for key "k"
    manager.set("k".to_string(), Value::String("parent".to_string()))?;

    // Create isolated child and override the same key
    let child_id = manager.create_parallel_context(Some("branch".to_string()))?;
    manager.switch_to(&child_id)?;
    manager.set("k".to_string(), Value::String("child".to_string()))?;
    manager.set("new_key".to_string(), Value::Integer(1))?;

    // Merge back with parent-wins (keep existing)
    manager.merge_child_to_parent(&child_id, ConflictResolution::KeepExisting)?;

    // Verify parent value is kept, and new child-only keys are merged
    manager.switch_to("root")?;
    assert_eq!(manager.get("k"), Some(Value::String("parent".to_string())));
    assert_eq!(manager.get("new_key"), Some(Value::Integer(1)));

    Ok(())
}

#[test]
fn test_manual_overwrite_merge_policy() -> RuntimeResult<()> {
    let mut manager = ContextManager::new();
    manager.initialize(Some("root".to_string()));

    // Parent value
    manager.set("k".to_string(), Value::String("parent".to_string()))?;

    // Child overrides same key
    let child_id = manager.create_parallel_context(Some("branch".to_string()))?;
    manager.switch_to(&child_id)?;
    manager.set("k".to_string(), Value::String("child".to_string()))?;

    // Overwrite policy should replace parent value
    manager.merge_child_to_parent(&child_id, ConflictResolution::Overwrite)?;

    manager.switch_to("root")?;
    assert_eq!(manager.get("k"), Some(Value::String("child".to_string())));

    Ok(())
}
