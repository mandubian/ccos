# Hierarchical Execution Context Management Implementation

## Context Types Clarification

This implementation introduces a **third context type** that complements the existing two:

### 1. **RuntimeContext** (Security & Permissions)
- **Purpose**: Security policy and capability permissions
- **Scope**: What operations are allowed, security levels, resource limits
- **Data**: Security level, allowed capabilities, memory limits, execution time limits
- **Example**: "This code can only access file I/O capabilities and has 64MB memory limit"

### 2. **CCOSEnvironment** (Complete Runtime Environment)
- **Purpose**: Complete CCOS runtime setup and configuration
- **Scope**: The entire execution environment including evaluator, marketplace, host
- **Data**: Configuration, evaluator instance, capability marketplace, causal chain
- **Example**: "The complete CCOS environment with all components initialized"

### 3. **ExecutionContext** (Hierarchical Data & State Management) ⭐ **NEW**
- **Purpose**: Hierarchical data propagation and state management during execution
- **Scope**: Runtime data that flows between steps, contexts, and parallel executions
- **Data**: Variables, configuration, state that needs to be shared or isolated
- **Example**: "User preferences, step results, and temporary data that flows through the execution hierarchy"

### How They Work Together

```
CCOSEnvironment
├── RuntimeContext (Security policies)
├── Evaluator
│   └── ContextManager (NEW: Hierarchical data management)
├── CapabilityMarketplace
└── RuntimeHost
```

- **RuntimeContext** determines **what** can be executed (security)
- **CCOSEnvironment** provides **where** execution happens (runtime)
- **ExecutionContext** manages **how** data flows during execution (state)

## Overview

This document describes the implementation of Hierarchical Execution Context Management for the CCOS Orchestrator, addressing the requirements specified in issue #79.

## Features Implemented

### 1. Hierarchical Context Stack
- **Context Inheritance**: Child contexts can inherit data from parent contexts
- **Data Propagation**: Automatic propagation of configuration and state data through the hierarchy
- **Context Isolation**: Multiple isolation levels for different use cases
- **Context Switching**: Seamless switching between contexts in the hierarchy

### 2. Isolation Levels
- **Inherit**: Inherit all parent data, can modify own data
- **Isolated**: Isolated from siblings, can read parent data but changes are isolated
- **Sandboxed**: Completely isolated, no parent data access

### 3. Context Management Operations
- **Context Creation**: Create new contexts with specific isolation levels
- **Context Switching**: Switch between contexts in the hierarchy
- **Data Access**: Get and set values in the current context
- **Context Exit**: Exit contexts and optionally merge data back to parent

### 4. Parallel Execution Support
- **Parallel Contexts**: Create isolated contexts for parallel execution
- **Context Isolation**: Parallel contexts are isolated from each other
- **Data Safety**: No data corruption between parallel execution paths

### 5. Checkpointing and Serialization
- **Automatic Checkpointing**: Configurable automatic checkpointing at intervals
- **Manual Checkpointing**: Create checkpoints at specific points
- **Context Serialization**: Serialize entire context stacks for persistence
- **Context Restoration**: Deserialize and restore context state

### 6. Integration with RTFS Evaluator
- **Step Special Forms**: Integration with `step` and `step-parallel` special forms
- **Context-Aware Evaluation**: Evaluator maintains execution context
- **Automatic Context Management**: Contexts are automatically created and managed during step execution

## Core Components

### 1. ExecutionContext
```rust
pub struct ExecutionContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub data: HashMap<String, Value>,
    pub metadata: ContextMetadata,
    pub children: Vec<String>,
}
```

### 2. ContextManager
```rust
pub struct ContextManager {
    stack: ContextStack,
    checkpoint_interval: Option<u64>,
    last_checkpoint: u64,
}
```

### 3. IsolationLevel
```rust
pub enum IsolationLevel {
    Inherit,    // Inherit all parent data
    Isolated,   // Isolated from siblings, can read parent data
    Sandboxed,  // Completely isolated
}
```

## Usage Examples

### Basic Context Operations
```rust
let mut manager = ContextManager::new();
manager.initialize(Some("root".to_string()));

// Set root-level data
manager.set("config".to_string(), Value::String("production".to_string()))?;

// Enter a step context
let step_id = manager.enter_step("validation", IsolationLevel::Inherit)?;

// Access inherited data
let config = manager.get("config"); // Returns Some("production")

// Set step-specific data
manager.set("records_processed".to_string(), Value::Integer(1000))?;

// Exit step
let result = manager.exit_step()?;
```

### Parallel Execution
```rust
// Create parallel contexts
let parallel1_id = manager.create_parallel_context(Some("worker-1".to_string()))?;
let parallel2_id = manager.create_parallel_context(Some("worker-2".to_string()))?;

// Switch between parallel contexts
manager.switch_to(&parallel1_id)?;
manager.set("worker_data".to_string(), Value::String("worker1_value".to_string()))?;

manager.switch_to(&parallel2_id)?;
manager.set("worker_data".to_string(), Value::String("worker2_value".to_string()))?;

// Contexts are isolated - no data corruption
```

### Checkpointing and Serialization
```rust
// Create a checkpoint
manager.checkpoint("milestone-1".to_string())?;

// Serialize the entire context
let serialized = manager.serialize()?;

// Restore in a new manager
let mut new_manager = ContextManager::new();
new_manager.initialize(Some("restored".to_string()));
new_manager.deserialize(&serialized)?;
```

## Integration with RTFS Step Forms

The hierarchical execution context is integrated with RTFS step special forms:

### Step Form Integration
```rust
// In eval_step_form
let context_id = context_manager.enter_step(&step_name, IsolationLevel::Inherit)?;

// Execute step body
let result = self.eval_exprs(body_exprs, env)?;

// Exit step context
let step_result = context_manager.exit_step()?;
```

### Parallel Step Form Integration
```rust
// In eval_step_parallel_form
for (index, expr) in args.iter().enumerate() {
    let parallel_id = context_manager.create_parallel_context(
        Some(format!("parallel-{}", index))
    )?;
    
    context_manager.switch_to(&parallel_id)?;
    let result = self.eval_expr(expr, env)?;
    results.push(result);
}
```

## Test Coverage

The implementation includes comprehensive test coverage:

- **Context Creation and Basic Operations**: Tests basic context creation, data setting, and retrieval
- **Context Isolation Levels**: Tests all three isolation levels (Inherit, Isolated, Sandboxed)
- **Context Depth Tracking**: Tests hierarchical depth tracking
- **Context Serialization**: Tests serialization and deserialization
- **Parallel Context Execution**: Tests parallel context isolation
- **Context Checkpointing**: Tests checkpoint creation and restoration

All tests pass successfully, demonstrating the robustness of the implementation.

## Benefits

1. **Improved Data Management**: Hierarchical context management provides better organization of execution state
2. **Enhanced Parallelism**: Isolated contexts enable safe parallel execution
3. **Better Debugging**: Context tracking provides clear execution paths
4. **Fault Tolerance**: Checkpointing enables recovery from failures
5. **Scalability**: Context isolation supports complex workflow execution

## Future Enhancements

1. **Context Merging Strategies**: Advanced merging strategies for complex data structures
2. **Context Monitoring**: Real-time monitoring and visualization of context hierarchies
3. **Performance Optimization**: Optimized context switching for high-frequency operations
4. **Distributed Contexts**: Support for distributed context management across multiple nodes

## Conclusion

The Hierarchical Execution Context Management system provides a robust foundation for managing complex execution state in CCOS. It addresses all the requirements from issue #79 and provides a solid base for future enhancements.

The implementation is well-tested, documented, and integrated with the existing RTFS evaluator, making it ready for production use.
