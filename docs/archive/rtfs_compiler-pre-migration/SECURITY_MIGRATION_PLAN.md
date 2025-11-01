# RTFS/CCOS Security Separation Migration Plan

## Overview
This document outlines the step-by-step migration from the current mixed stdlib to a secure, separated architecture where RTFS contains only pure functions and CCOS handles all dangerous operations.

## Current State Analysis

### ✅ Safe Functions (Keep in RTFS)
- **Arithmetic**: `+`, `-`, `*`, `/`, `max`, `min`
- **Comparison**: `=`, `!=`, `>`, `<`, `>=`, `<=`
- **Boolean**: `and`, `or`, `not`
- **String**: `str`, `substring`, `string-length`, `string-contains`, `type-name`
- **Collections**: `vector`, `hash-map`, `get`, `assoc`, `dissoc`, `count`, `first`, `rest`, `cons`, `conj`, `get-in`, `partition`
- **Functional**: `map`, `filter`, `reduce` (pure operations)
- **Type Predicates**: `int?`, `float?`, `number?`, `string?`, `boolean?`, `nil?`, `map?`, `vector?`, `keyword?`, `symbol?`, `fn?`, `empty?`
- **Utility**: `inc`, `dec`, `range`, `factorial`, `length`

### ❌ Dangerous Functions (Move to CCOS)
- **File I/O**: `tool:open-file`, `tool:read-line`, `tool:write-line`, `tool:close-file`, `tool:file-exists?`
- **Network**: `tool:http-fetch`
- **System**: `tool:get-env`, `tool:current-time`, `tool:current-timestamp-ms`
- **Data**: `tool:parse-json`, `tool:serialize-json`
- **Output**: `tool:log`, `tool:print`, `println`
- **Agent**: `discover-agents`, `task-coordination`, `ask-human`, `discover-and-assess-agents`, `establish-system-baseline`

## Migration Steps

### Phase 1: Create Secure Infrastructure ✅ DONE
- [x] Create `SecureStandardLibrary` with only safe functions
- [x] Create `CapabilityRegistry` for dangerous operations
- [x] Create `SecurityContext` and policy system
- [x] Update `call` function to reject direct capability calls

### Phase 2: Implement Security Boundaries
```bash
# Update runtime to use secure stdlib by default
# File: src/runtime/evaluator.rs
```

**Implementation:**
1. **Create secure environment factory**:
   ```rust
   pub fn create_secure_environment() -> Environment {
       SecureStandardLibrary::create_secure_environment()
   }
   ```

2. **Update evaluator to use security context**:
   ```rust
   pub struct Evaluator {
       security_context: RuntimeContext,
       capability_registry: Option<Arc<CapabilityRegistry>>,
   }
   ```

3. **Add capability permission checking**:
   ```rust
   fn check_capability_permission(&self, capability_id: &str) -> Result<(), RuntimeError> {
       if !self.security_context.is_capability_allowed(capability_id) {
           return Err(RuntimeError::PermissionDenied(format!(
               "Capability '{}' not allowed in current security context", 
               capability_id
           )));
       }
       Ok(())
   }
   ```

### Phase 3: Update Call Function Integration
```rust
// In stdlib.rs - update call function to route through CCOS
fn call_capability(
    args: Vec<Value>,
    evaluator: &Evaluator,
    env: &mut Environment,
) -> RuntimeResult<Value> {
    let capability_id = extract_capability_id(&args)?;
    
    // Check permissions
    evaluator.check_capability_permission(&capability_id)?;
    
    // Route through CCOS if capability registry available
    if let Some(registry) = &evaluator.capability_registry {
        if let Some(capability) = registry.get_capability(&capability_id) {
            // Check if requires microVM
            if evaluator.security_context.requires_microvm(&capability_id) {
                return execute_in_microvm(capability, args);
            }
            
            // Execute with permission checking
            return execute_with_monitoring(capability, args);
        }
    }
    
    // Fallback error
    Err(RuntimeError::Generic(format!(
        "Capability '{}' not available in current runtime context",
        capability_id
    )))
}
```

### Phase 4: Implement MicroVM Integration
```rust
// New file: src/runtime/microvm.rs
pub struct MicroVMExecutor {
    // Integration with secure execution environment
    // Could use FireCracker, gVisor, or similar
}

impl MicroVMExecutor {
    pub fn execute_capability(
        &self,
        capability: &Capability,
        args: Vec<Value>,
        context: &RuntimeContext,
    ) -> RuntimeResult<Value> {
        // 1. Serialize arguments
        // 2. Launch microVM with restricted resources
        // 3. Execute capability in isolated environment
        // 4. Return result or timeout/error
        todo!("Implement microVM execution")
    }
}
```

### Phase 5: Update Module System
```rust
// Update module loading to use secure context
pub fn load_module_with_security(
    module_path: &str,
    security_context: RuntimeContext,
) -> RuntimeResult<Module> {
    // Validate module against security policy
    SecurityValidator::validate(&security_context)?;
    
    // Load module with restricted environment
    let mut env = match security_context.security_level {
        SecurityLevel::Pure => SecureStandardLibrary::create_secure_environment(),
        SecurityLevel::Controlled => create_controlled_environment(&security_context),
        SecurityLevel::Full => create_full_environment(&security_context),
    };
    
    load_module_into_environment(module_path, &mut env)
}
```

### Phase 6: Testing and Validation
```rust
// Test cases for security boundaries
#[cfg(test)]
mod security_tests {
    #[test]
    fn test_pure_context_blocks_dangerous_operations() {
        let context = RuntimeContext::pure();
        let evaluator = Evaluator::with_context(context);
        
        // Should fail - file operations not allowed
        let result = evaluator.eval("(call :ccos.io.open-file \"test.txt\")");
        assert!(matches!(result, Err(RuntimeError::PermissionDenied(_))));
    }
    
    #[test]
    fn test_controlled_context_allows_specific_capabilities() {
        let context = SecurityPolicies::data_processing();
        let evaluator = Evaluator::with_context(context);
        
        // Should succeed - JSON parsing allowed
        let result = evaluator.eval("(call :ccos.data.parse-json \"{\\\"test\\\": true}\")");
        assert!(result.is_ok());
        
        // Should fail - file operations not allowed
        let result = evaluator.eval("(call :ccos.io.open-file \"test.txt\")");
        assert!(matches!(result, Err(RuntimeError::PermissionDenied(_))));
    }
}
```

## Security Benefits

### 1. **Principle of Least Privilege**
- RTFS programs start with minimal permissions
- Capabilities must be explicitly granted
- Fine-grained control over what code can do

### 2. **Secure by Default**
- Pure functions can't cause system damage
- All I/O operations go through controlled channels
- Dangerous operations isolated in microVMs

### 3. **Auditability**
- All capability calls logged to causal chain
- Clear separation between pure and effectful code
- Security policies explicitly defined

### 4. **Composability**
- Pure RTFS functions can be safely composed
- Capabilities can be granted contextually
- Different security levels for different use cases

## Implementation Priority

1. **HIGH**: Complete Phase 2 (Security Boundaries) ⚠️
2. **HIGH**: Complete Phase 3 (Call Function Integration) ⚠️
3. **MEDIUM**: Phase 4 (MicroVM Integration) for dangerous operations
4. **MEDIUM**: Phase 5 (Module System Updates)
5. **LOW**: Phase 6 (Complete test coverage)

## Backward Compatibility

### Migration Strategy
1. **Deprecation warnings** for direct stdlib dangerous functions
2. **Compatibility layer** that routes old calls through CCOS
3. **Documentation** showing migration path
4. **Tools** to automatically update existing code

### Example Migration
```rtfs
;; Old way (deprecated)
(tool:log "Hello World")

;; New way (secure)
(call :ccos.io.log "Hello World")
```

This ensures existing code continues to work while encouraging migration to the secure model.
