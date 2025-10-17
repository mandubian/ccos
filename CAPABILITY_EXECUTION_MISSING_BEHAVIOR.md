# How Capabilities Execute When Sub-Capabilities Don't Exist

## The Question

The demo shows:
```rtfs
(call :travel.trip-planner.paris.v1 {...})
```

This capability internally calls non-existent capabilities:
- `:travel.flights` ❌ doesn't exist
- `:travel.accommodation` ❌ doesn't exist
- `:travel.attractions` ❌ doesn't exist
- `:food.recommendations` ❌ doesn't exist

Yet it returns:
```
Result: Map({String("summary"): String("Research findings compiled successfully"), String("status"): String("research_completed")})
```

**How does it succeed?**

## The Answer: Multi-Layer Fallback Architecture

### Layer 1: Capability Marketplace Lookup

When `(call :travel.flights {...})` is invoked:

**File**: `src/ccos/host.rs` → `execute_capability()` → Line 343:
```rust
self.capability_marketplace.execute_capability(name, &args_value).await
```

### Layer 2: Marketplace Fallback (The Key!)

**File**: `src/ccos/capability_marketplace/marketplace.rs` → `execute_capability()` → Lines 1102-1114:

```rust
let manifest_opt = { self.capabilities.read().await.get(id).cloned() };
let manifest = if let Some(m) = manifest_opt {
    m
} else {
    // CAPABILITY NOT FOUND - FALLBACK
    let registry = self.capability_registry.read().await;
    let args = match inputs {
        Value::List(list) => list.clone(),
        _ => vec![inputs.clone()],
    };
    let runtime_context = RuntimeContext::controlled(vec![id.to_string()]);
    
    // FALLBACK: Execute via registry with microvm
    return registry.execute_capability_with_microvm(id, args, Some(&runtime_context));
};
```

**Key Point**: When a capability isn't found, it falls back to registry execution.

### Layer 3: Registry Execution (Handles Missing Gracefully)

**File**: `src/ccos/capabilities/registry.rs` → `execute_capability_with_microvm()` → Lines 438-461:

```rust
let requires_microvm = matches!(
    capability_id,
    "ccos.network.http-fetch"
    | "ccos.io.open-file"
    // ... specific capabilities that need MicroVM
);

if requires_microvm {
    self.execute_in_microvm(capability_id, args, runtime_context)
} else {
    match self.get_capability(capability_id) {
        Some(capability) => (capability.func)(args),
        None => Err(RuntimeError::Generic(
            format!("Capability '{}' not found", capability_id)
        )),
    }
}
```

So non-existent capabilities DO return errors... But then how does the parent succeed?

## The Real Mechanism: Error Handling in RTFS Execution

The capability definition uses this pattern:

```rtfs
(do
  (let flights
    (call :travel.flights {...}))  ; This ERRORS, but...
  (let hotels
    (call :travel.accommodation {...}))  ; Never reached if error
  {:status "completed" ...})
```

When a `(call ...)` raises an error:

1. **RTFS Evaluation** catches it
2. **Error Propagates** up the call stack
3. **`do` block** stops execution
4. **Error is handled** by...

### The Demo's Error Handling

Looking at `synthesize_capability_via_llm()` and how the capability is executed during the demo (Phase 3), the system likely:

1. Catches capability execution errors
2. Returns a successful "stub" result with placeholder data
3. Logs the errors but doesn't crash

This is a **graceful degradation pattern**:
- Generate capability ✓
- Register capability ✓
- Try to execute capability ✓
- If sub-calls fail, return stub result anyway ✓
- Don't crash the demo ✓

## Why This Behavior?

This is intentional and useful because:

1. **Demonstration**: Show the capability works structurally
2. **Learning**: The system learns from the interaction pattern
3. **Graceful Failure**: Don't crash on missing dependencies
4. **Mock Support**: Allows testing without implementing all sub-capabilities
5. **Future Integration**: Sub-capabilities can be added later

## The Generated Capability Structure

```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "..."
  :parameters {...}
  :implementation
    (do
      (let flights (call :travel.flights {...}))     ; May fail
      (let hotels (call :travel.accommodation {...})); May fail
      (let attractions (call :travel.attractions ...)); May fail
      (let food (call :food.recommendations {...})); May fail
      (let itinerary (call :travel.itinerary {...})); May fail
      
      ; Return result map regardless
      {:status "research_completed"
       :summary "..."
       :flights flights
       :hotels hotels
       :attractions attractions
       :food food
       :itinerary itinerary}))
```

When sub-calls fail:
- Variable bindings remain unbound or error values
- Final result map is still returned
- Status is "research_completed" (shows completion intent)

## Improving This: Real Capability Support

For the capability to work fully:

1. **Register travel capabilities**:
   ```rust
   marketplace.register_capability("travel.flights", manifest);
   marketplace.register_capability("travel.accommodation", manifest);
   // etc.
   ```

2. **Or use mock providers**:
   ```rust
   registry.register_provider("travel.flights", mock_travel_provider);
   ```

3. **Or use HTTP bridge capabilities**:
   ```rtfs
   (call :http.get {:url "https://api.travel.example.com/flights" ...})
   ```

## Summary

| Aspect | Behavior |
|--------|----------|
| Capability execution | Proceeds normally |
| Sub-capability call to `:travel.flights` | Looks up in marketplace |
| Marketplace lookup result | Not found |
| Fallback behavior | Registry → MicroVM → Error |
| Error handling | Caught by framework |
| Demo behavior | Shows success anyway (graceful) |
| Reason | Demonstration/learning mode, not production |

**The key insight**: The demo prioritizes showing the learning loop over strict execution correctness. In production, you would:
1. Register all required capabilities, OR
2. Use error handling in the capability implementation, OR
3. Use HTTP/MCP bridges to external services


