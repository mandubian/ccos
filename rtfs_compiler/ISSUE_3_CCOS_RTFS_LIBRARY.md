# Issue #3: Create CCOS RTFS Library for Intent Graph Functions

**Status:** Open  
**Priority:** High  
**Type:** Feature Implementation  
**Created:** 2025-01-27  
**Related:** [Issue #2: Enhanced Intent Graph](./ISSUE_2_COMPLETION_REPORT.md)

## 🎯 **Objective**

Create a comprehensive CCOS RTFS library that exposes all Intent Graph functionality to RTFS programs, enabling RTFS developers to interact with the Intent Graph system using native RTFS syntax.

## 📋 **Problem Statement**

Currently, all Intent Graph functionality exists only in Rust (`IntentGraph` struct) but is not accessible from RTFS programs. RTFS developers cannot:

- Create or manage intents using RTFS syntax
- Query intent relationships and hierarchies
- Store or restore subgraphs
- Work with weighted edges and metadata
- Integrate Intent Graph operations into RTFS plans

## 🚀 **Solution: CCOS RTFS Library**

### **3.1. Core RTFS Functions to Implement**

#### **Intent Management**
```clojure
;; Create new intent
(ccos.intent-graph/create-intent {
  :goal "Deploy production web service"
  :constraints {:availability (> 0.99)}
  :preferences {:region "us-east-1"}
  :success-criteria (and (deployed? service) (healthy? service))
})

;; Get intent by ID
(ccos.intent-graph/get-intent :intent-id)

;; Update intent status
(ccos.intent-graph/update-intent-status :intent-id :completed)

;; Find intents by criteria
(ccos.intent-graph/find-intents {
  :status :active
  :goal-contains "deploy"
})
```

#### **Relationship Management**
```clojure
;; Create basic edge
(ccos.intent-graph/create-edge :intent-a :intent-b :depends-on)

;; Create weighted edge with metadata
(ccos.intent-graph/create-weighted-edge 
  :intent-a :intent-b :conflicts-with
  :weight 0.8
  :metadata {
    :reason "Resource contention"
    :severity :high
    :confidence 0.95
  })

;; Get relationship information
(ccos.intent-graph/get-parent-intents :intent-id)
(ccos.intent-graph/get-child-intents :intent-id)
(ccos.intent-graph/get-strongly-connected-intents :intent-id)
(ccos.intent-graph/get-intent-hierarchy :intent-id)
```

#### **Subgraph Operations**
```clojure
;; Store subgraph from root intent
(ccos.intent-graph/store-subgraph-from-root :root-intent-id :path)

;; Store subgraph from child intent  
(ccos.intent-graph/store-subgraph-from-child :child-intent-id :path)

;; Restore subgraph
(ccos.intent-graph/restore-subgraph :path)

;; Backup entire graph
(ccos.intent-graph/backup :path)
```

#### **Graph Analysis**
```clojure
;; Get all edges for intent
(ccos.intent-graph/get-edges-for-intent :intent-id)

;; Find intents by relationship type
(ccos.intent-graph/find-intents-by-relationship :intent-id :depends-on)

;; Analyze relationship strength
(ccos.intent-graph/get-relationship-strength :intent-a :intent-b)

;; Get active intents
(ccos.intent-graph/get-active-intents)
```

### **3.2. Implementation Plan**

#### **Phase 1: Core Library Structure**
1. **Create CCOS RTFS Module**
   - `src/rtfs/ccos/mod.rs` - Main CCOS RTFS module
   - `src/rtfs/ccos/intent_graph.rs` - Intent Graph RTFS functions
   - `src/rtfs/ccos/types.rs` - RTFS type conversions

2. **Implement Function Wrappers**
   - Wrap all `IntentGraph` methods in RTFS-compatible functions
   - Handle type conversions between RTFS `Value` and Rust types
   - Implement proper error handling and RTFS error types

3. **Register Capabilities**
   - Register all CCOS functions as capabilities in the marketplace
   - Implement capability attestation and security validation
   - Add to standard library loading

#### **Phase 2: RTFS Integration**
1. **RTFS Syntax Support**
   - Ensure all functions work with RTFS syntax
   - Support both keyword and string parameter styles
   - Implement proper RTFS value handling

2. **Type Safety**
   - Add schema validation for all function parameters
   - Implement RTFS type annotations for CCOS objects
   - Ensure compile-time validation where possible

3. **Error Handling**
   - Convert Rust errors to RTFS-compatible error types
   - Implement proper error propagation in RTFS context
   - Add meaningful error messages for RTFS developers

#### **Phase 3: Advanced Features**
1. **Query Language**
   - Implement RTFS-based query language for Intent Graph
   - Support complex filtering and search operations
   - Add semantic search capabilities

2. **Batch Operations**
   - Support bulk intent creation and updates
   - Implement transaction-like operations
   - Add atomic subgraph operations

3. **Performance Optimization**
   - Implement caching for frequently accessed data
   - Add lazy loading for large graphs
   - Optimize RTFS function calls

### **3.3. Technical Requirements**

#### **File Structure**
```
src/rtfs/ccos/
├── mod.rs                 # Main CCOS RTFS module
├── intent_graph.rs        # Intent Graph RTFS functions
├── types.rs              # Type conversions
├── capabilities.rs       # Capability registration
└── tests/                # RTFS function tests
    ├── intent_graph_tests.rtfs
    ├── subgraph_tests.rtfs
    └── integration_tests.rtfs
```

#### **Function Signatures**
```rust
// Example function wrapper
pub fn ccos_create_intent(
    args: Vec<Value>,
    evaluator: &Evaluator,
    env: &mut Environment,
) -> RuntimeResult<Value> {
    // Implementation
}

// Type conversion helpers
pub fn rtfs_value_to_intent(value: &Value) -> Result<Intent, RuntimeError> {
    // Convert RTFS Value to Intent
}

pub fn intent_to_rtfs_value(intent: &Intent) -> Value {
    // Convert Intent to RTFS Value
}
```

#### **Capability Registration**
```rust
// Register CCOS capabilities
pub async fn register_ccos_capabilities(
    marketplace: &CapabilityMarketplace
) -> RuntimeResult<()> {
    // Register all CCOS functions
    marketplace.register_local_capability(
        "ccos.intent-graph.create-intent".to_string(),
        "Create Intent".to_string(),
        "Creates a new intent in the graph".to_string(),
        Arc::new(ccos_create_intent),
    ).await?;
    
    // Register more capabilities...
    Ok(())
}
```

### **3.4. Testing Strategy**

#### **Unit Tests**
- Test each RTFS function individually
- Verify type conversions and error handling
- Test edge cases and invalid inputs

#### **Integration Tests**
- Test RTFS programs that use CCOS functions
- Verify Intent Graph operations from RTFS context
- Test subgraph storage and restore from RTFS

#### **RTFS Test Files**
```clojure
;; test_intent_creation.rtfs
(let [intent-id (ccos.intent-graph/create-intent {
  :goal "Test intent"
  :constraints {:test true}
})]
  (assert (not (nil? intent-id)))
  (ccos.intent-graph/update-intent-status intent-id :completed))

;; test_subgraph_operations.rtfs
(let [root-id (ccos.intent-graph/create-intent {:goal "Root"})
      child-id (ccos.intent-graph/create-intent {:goal "Child"})]
  (ccos.intent-graph/create-edge child-id root-id :is-subgoal-of)
  (ccos.intent-graph/store-subgraph-from-root root-id "test-subgraph.json")
  (ccos.intent-graph/restore-subgraph "test-subgraph.json"))
```

### **3.5. Documentation Requirements**

#### **RTFS API Documentation**
- Complete function reference with examples
- Type definitions and schemas
- Error codes and handling
- Best practices and patterns

#### **Integration Guide**
- How to use CCOS functions in RTFS plans
- Integration with step special forms
- Performance considerations
- Security and capability management

#### **Examples and Tutorials**
- Basic intent creation and management
- Complex relationship modeling
- Subgraph operations and context switching
- Real-world use cases and patterns

## ✅ **Success Criteria**

### **Functional Requirements**
- [ ] All Intent Graph functions accessible from RTFS
- [ ] Proper type conversion between RTFS and Rust
- [ ] Comprehensive error handling and validation
- [ ] Subgraph storage and restore from RTFS
- [ ] Weighted edges and metadata support

### **Performance Requirements**
- [ ] RTFS function calls perform within acceptable limits
- [ ] Large graph operations don't block RTFS execution
- [ ] Efficient memory usage for graph operations
- [ ] Proper async handling for long-running operations

### **Quality Requirements**
- [ ] 100% test coverage for RTFS functions
- [ ] All tests passing in CI/CD pipeline
- [ ] Documentation complete and accurate
- [ ] Security validation and capability attestation
- [ ] RTFS syntax compliance and validation

## 🔗 **Dependencies**

- **Issue #2**: Enhanced Intent Graph (✅ Complete)
- **RTFS 2.0**: Core RTFS functionality (✅ Available)
- **Capability System**: For function registration (✅ Available)
- **Standard Library**: For integration (✅ Available)

## 📅 **Timeline**

- **Phase 1**: Core Library Structure (2-3 days)
- **Phase 2**: RTFS Integration (2-3 days)  
- **Phase 3**: Advanced Features (3-4 days)
- **Testing & Documentation**: (2-3 days)

**Total Estimated Time**: 9-13 days

## 🎯 **Next Steps**

1. **Create RTFS CCOS module structure**
2. **Implement core function wrappers**
3. **Add capability registration**
4. **Create comprehensive test suite**
5. **Write documentation and examples**

This issue will complete the Intent Graph integration by making all functionality accessible to RTFS programs, enabling the full vision of CCOS as an RTFS-native cognitive computing system. 