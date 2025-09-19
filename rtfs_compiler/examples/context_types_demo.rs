//! Context Types Demo - Showing RuntimeContext, CCOSEnvironment, and ExecutionContext
//!
//! This example demonstrates how the three different context types work together:
//! 1. RuntimeContext - Security and permissions
//! 2. CCOSEnvironment - Complete runtime environment
//! 3. ExecutionContext - Hierarchical data management

use rtfs_compiler::runtime::RuntimeContext;
use rtfs_compiler::ccos::environment::{
    CCOSEnvironment, CCOSBuilder, SecurityLevel, CapabilityCategory
};
use rtfs_compiler::ccos::execution_context::{ContextManager, IsolationLevel};
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::runtime::error::RuntimeResult;

fn main() -> RuntimeResult<()> {
    println!("=== CCOS Context Types Demo ===\n");

    // 1. RUNTIME CONTEXT (Security & Permissions)
    println!("1. RUNTIME CONTEXT (Security & Permissions)");
    println!("   Determines what operations are allowed\n");
    
    let runtime_context = RuntimeContext::controlled(vec![
        "ccos.io.read-file".to_string(),
        "ccos.io.write-file".to_string(),
    ]);
    
    println!("   Security Level: {:?}", runtime_context.security_level);
    println!("   Allowed Capabilities: {:?}", runtime_context.allowed_capabilities);
    println!("   Max Memory: {:?} bytes", runtime_context.max_memory_usage);
    println!("   Max Execution Time: {:?} ms", runtime_context.max_execution_time);
    println!("   Requires MicroVM: {}", runtime_context.use_microvm);
    println!();

    // 2. CCOS ENVIRONMENT (Complete Runtime)
    println!("2. CCOS ENVIRONMENT (Complete Runtime)");
    println!("   Provides the complete execution environment\n");
    
    let ccos_env = CCOSBuilder::new()
        .security_level(SecurityLevel::Standard)
        .enable_category(CapabilityCategory::FileIO)
        .enable_category(CapabilityCategory::System)
        .max_execution_time(5000)
        .verbose(true)
        .build()?;
    
    println!("   Environment created with {} capabilities", ccos_env.list_capabilities().len());
    println!("   Security Level: {:?}", ccos_env.config().security_level);
    println!("   Enabled Categories: {:?}", ccos_env.config().enabled_categories);
    println!();

    // 3. EXECUTION CONTEXT (Hierarchical Data Management)
    println!("3. EXECUTION CONTEXT (Hierarchical Data Management)");
    println!("   Manages data flow during execution\n");
    
    let mut exec_context = ContextManager::new();
    exec_context.initialize(Some("main-workflow".to_string()));
    
    // Set some execution data
    exec_context.set("user_id".to_string(), Value::String("user123".to_string()))?;
    exec_context.set("session_id".to_string(), Value::String("session456".to_string()))?;
    exec_context.set("preferences".to_string(), Value::String("dark_mode".to_string()))?;
    
    println!("   Root Context ID: {:?}", exec_context.current_context_id());
    println!("   Context Depth: {}", exec_context.depth());
    println!("   Data in root context:");
    println!("     - user_id: {:?}", exec_context.get("user_id"));
    println!("     - session_id: {:?}", exec_context.get("session_id"));
    println!("     - preferences: {:?}", exec_context.get("preferences"));
    println!();

    // 4. DEMONSTRATE HIERARCHICAL DATA FLOW
    println!("4. HIERARCHICAL DATA FLOW");
    println!("   Showing how data flows through contexts\n");
    
    // Create a child context that inherits data
    let child_id = exec_context.enter_step("data-processing", IsolationLevel::Inherit)?;
    println!("   Entered child context: {}", child_id);
    println!("   Child inherits parent data:");
    println!("     - user_id: {:?}", exec_context.get("user_id"));
    println!("     - session_id: {:?}", exec_context.get("session_id"));
    
    // Add data in child context
    exec_context.set("processing_result".to_string(), Value::String("success".to_string()))?;
    exec_context.set("temp_data".to_string(), Value::Integer(42))?;
    println!("   Added data in child context:");
    println!("     - processing_result: {:?}", exec_context.get("processing_result"));
    println!("     - temp_data: {:?}", exec_context.get("temp_data"));
    
    // Exit child context
    exec_context.exit_step()?;
    println!("   Exited child context");
    println!("   Back to root context - child data is not visible:");
    println!("     - processing_result: {:?}", exec_context.get("processing_result"));
    println!("     - temp_data: {:?}", exec_context.get("temp_data"));
    println!();

    // 5. HOW THEY WORK TOGETHER
    println!("5. HOW THEY WORK TOGETHER");
    println!("   RuntimeContext: Controls what operations are allowed");
    println!("   CCOSEnvironment: Provides the execution environment");
    println!("   ExecutionContext: Manages data flow during execution");
    println!();
    println!("   Example workflow:");
    println!("   1. RuntimeContext allows file I/O operations");
    println!("   2. CCOSEnvironment provides the file I/O capabilities");
    println!("   3. ExecutionContext stores file paths and results");
    println!("   4. Data flows hierarchically through step contexts");
    println!();

    println!("✅ All three context types work together seamlessly!");
    Ok(())
}
