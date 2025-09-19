//! Hierarchical Execution Context Management Demo
//!
//! This example demonstrates the hierarchical execution context management system
//! for CCOS, showing context inheritance, data propagation, isolation, and checkpointing.

use rtfs_compiler::ccos::execution_context::{ContextManager, IsolationLevel};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeResult;

fn main() -> RuntimeResult<()> {
    println!("=== CCOS Hierarchical Execution Context Management Demo ===\n");

    // Create a context manager with automatic checkpointing every 5 seconds
    let mut context_manager = ContextManager::with_checkpointing(5000);
    
    // Initialize with a root context
    context_manager.initialize(Some("root-workflow".to_string()));
    println!("✓ Initialized root context: {}", context_manager.current_context_id().unwrap());
    
    // Set some root-level configuration
    context_manager.set("workflow_name".to_string(), Value::String("data-processing-pipeline".to_string()))?;
    context_manager.set("max_retries".to_string(), Value::Integer(3))?;
    context_manager.set("timeout_seconds".to_string(), Value::Integer(300))?;
    
    println!("✓ Set root configuration values");
    println!("  - workflow_name: {}", context_manager.get("workflow_name").unwrap());
    println!("  - max_retries: {}", context_manager.get("max_retries").unwrap());
    println!("  - timeout_seconds: {}", context_manager.get("timeout_seconds").unwrap());

    // Demonstrate step execution with inheritance
    println!("\n--- Step 1: Data Validation (Inherit) ---");
    let validation_step_id = context_manager.enter_step("data-validation", IsolationLevel::Inherit)?;
    println!("✓ Entered validation step: {}", validation_step_id);
    
    // Validation step inherits all parent configuration
    println!("✓ Inherited configuration:");
    println!("  - workflow_name: {}", context_manager.get("workflow_name").unwrap());
    println!("  - max_retries: {}", context_manager.get("max_retries").unwrap());
    
    // Set validation-specific data
    context_manager.set("validation_rules".to_string(), Value::String("strict".to_string()))?;
    context_manager.set("records_processed".to_string(), Value::Integer(0))?;
    
    println!("✓ Set validation-specific data");
    println!("  - validation_rules: {}", context_manager.get("validation_rules").unwrap());
    println!("  - records_processed: {}", context_manager.get("records_processed").unwrap());
    
    // Simulate processing some records
    context_manager.set("records_processed".to_string(), Value::Integer(1500))?;
    println!("✓ Updated records_processed: {}", context_manager.get("records_processed").unwrap());
    
    // Exit validation step
    let validation_result = context_manager.exit_step()?;
    println!("✓ Exited validation step");
    if let Some(result) = validation_result {
        println!("  - Step result: {} records validated", result.data.get("records_processed").unwrap());
    }

    // Demonstrate isolated step execution
    println!("\n--- Step 2: Data Transformation (Isolated) ---");
    let transform_step_id = context_manager.enter_step("data-transformation", IsolationLevel::Isolated)?;
    println!("✓ Entered transformation step: {}", transform_step_id);
    
    // Isolated step can read parent data but changes are isolated
    println!("✓ Can read parent configuration:");
    println!("  - workflow_name: {}", context_manager.get("workflow_name").unwrap());
    println!("  - max_retries: {}", context_manager.get("max_retries").unwrap());
    
    // Set transformation-specific data
    context_manager.set("transformation_type".to_string(), Value::String("normalize".to_string()))?;
    context_manager.set("records_transformed".to_string(), Value::Integer(0))?;
    
    println!("✓ Set transformation-specific data");
    println!("  - transformation_type: {}", context_manager.get("transformation_type").unwrap());
    
    // Simulate transformation
    context_manager.set("records_transformed".to_string(), Value::Integer(1200))?;
    println!("✓ Updated records_transformed: {}", context_manager.get("records_transformed").unwrap());
    
    // Exit transformation step
    let transform_result = context_manager.exit_step()?;
    println!("✓ Exited transformation step");
    if let Some(result) = transform_result {
        println!("  - Step result: {} records transformed", result.data.get("records_transformed").unwrap());
    }

    // Demonstrate sandboxed step execution
    println!("\n--- Step 3: Error Handling (Sandboxed) ---");
    let error_step_id = context_manager.enter_step("error-handling", IsolationLevel::Sandboxed)?;
    println!("✓ Entered error handling step: {}", error_step_id);
    
    // Sandboxed step cannot access parent data
    println!("✓ Sandboxed context - no parent data access:");
    println!("  - workflow_name: {}", context_manager.get("workflow_name").unwrap_or(Value::String("NOT ACCESSIBLE".to_string())));
    
    // Set sandboxed data
    context_manager.set("error_count".to_string(), Value::Integer(5))?;
    context_manager.set("error_severity".to_string(), Value::String("medium".to_string()))?;
    
    println!("✓ Set sandboxed data:");
    println!("  - error_count: {}", context_manager.get("error_count").unwrap());
    println!("  - error_severity: {}", context_manager.get("error_severity").unwrap());
    
    // Exit error handling step
    let error_result = context_manager.exit_step()?;
    println!("✓ Exited error handling step");

    // Demonstrate parallel execution
    println!("\n--- Step 4: Parallel Processing ---");
    let parallel1_id = context_manager.create_parallel_context(Some("parallel-processor-1".to_string()))?;
    let parallel2_id = context_manager.create_parallel_context(Some("parallel-processor-2".to_string()))?;
    
    println!("✓ Created parallel contexts:");
    println!("  - parallel1: {}", parallel1_id);
    println!("  - parallel2: {}", parallel2_id);
    
    // Switch to first parallel context
    context_manager.switch_to(&parallel1_id)?;
    context_manager.set("processor_id".to_string(), Value::String("proc-1".to_string()))?;
    context_manager.set("items_processed".to_string(), Value::Integer(500))?;
    println!("✓ Parallel context 1:");
    println!("  - processor_id: {}", context_manager.get("processor_id").unwrap());
    println!("  - items_processed: {}", context_manager.get("items_processed").unwrap());
    
    // Switch to second parallel context
    context_manager.switch_to(&parallel2_id)?;
    context_manager.set("processor_id".to_string(), Value::String("proc-2".to_string()))?;
    context_manager.set("items_processed".to_string(), Value::Integer(700))?;
    println!("✓ Parallel context 2:");
    println!("  - processor_id: {}", context_manager.get("processor_id").unwrap());
    println!("  - items_processed: {}", context_manager.get("items_processed").unwrap());
    
    // Switch back to root
    context_manager.switch_to("root-workflow")?;
    println!("✓ Switched back to root context");

    // Demonstrate context serialization and checkpointing
    println!("\n--- Step 5: Serialization and Checkpointing ---");
    
    // Create a checkpoint
    context_manager.checkpoint("workflow-milestone".to_string())?;
    println!("✓ Created checkpoint: workflow-milestone");
    
    // Serialize the entire context
    let serialized = context_manager.serialize()?;
    println!("✓ Serialized context ({} bytes)", serialized.len());
    
    // Create a new manager and deserialize
    let mut restored_manager = ContextManager::new();
    restored_manager.initialize(Some("restored-workflow".to_string()));
    restored_manager.deserialize(&serialized)?;
    println!("✓ Deserialized context to new manager");
    
    // Verify data was restored
    println!("✓ Verified restored data:");
    println!("  - workflow_name: {}", restored_manager.get("workflow_name").unwrap());
    println!("  - max_retries: {}", restored_manager.get("max_retries").unwrap());
    println!("  - timeout_seconds: {}", restored_manager.get("timeout_seconds").unwrap());

    // Demonstrate context depth tracking
    println!("\n--- Step 6: Context Depth Tracking ---");
    println!("✓ Current context depth: {}", context_manager.depth());
    
    let nested_step_id = context_manager.enter_step("nested-step", IsolationLevel::Inherit)?;
    println!("✓ Entered nested step: {}", nested_step_id);
    println!("✓ Context depth: {}", context_manager.depth());
    
    let deeper_step_id = context_manager.enter_step("deeper-step", IsolationLevel::Inherit)?;
    println!("✓ Entered deeper step: {}", deeper_step_id);
    println!("✓ Context depth: {}", context_manager.depth());
    
    // Exit nested steps
    context_manager.exit_step()?;
    context_manager.exit_step()?;
    println!("✓ Exited nested steps");
    println!("✓ Final context depth: {}", context_manager.depth());

    println!("\n=== Demo Complete ===");
    println!("✓ All hierarchical execution context features demonstrated successfully!");
    
    Ok(())
}
