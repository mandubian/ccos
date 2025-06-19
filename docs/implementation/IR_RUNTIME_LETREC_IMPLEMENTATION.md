# IR Runtime Letrec Implementation

This document describes the implementation of letrec semantics (recursive and mutually recursive function support) in the RTFS IR runtime.

## Overview

The IR runtime now supports the same letrec semantics as the AST runtime, but optimized for the IR representation. This enables recursive and mutually recursive functions to work correctly when using the IR execution path.

## Implementation Strategy

### Two-Pass Approach

Like the AST runtime, the IR runtime uses a two-pass strategy for letrec evaluation:

1. **Pass 1: Placeholder Creation**
   - Identify lambda bindings in let expressions
   - Create `FunctionPlaceholder` values with `RefCell` interior mutability
   - Bind placeholders immediately in the environment
   - Separate function bindings from other bindings

2. **Pass 2: Resolution**
   - Evaluate non-function bindings first
   - Create actual `Function::IrLambda` values for lambda bindings
   - Update placeholder cells to point to resolved functions

### Key Components

#### IrLambda Function Variant

Added a new `Function::IrLambda` variant to the `Function` enum in `src/runtime/values.rs`:

```rust
/// IR-based lambda functions (for IR runtime)
IrLambda {
    params: Vec<crate::ir::IrNode>,
    variadic_param: Option<Box<crate::ir::IrNode>>,
    body: Vec<crate::ir::IrNode>,
    closure_env: Box<crate::runtime::ir_runtime::IrEnvironment>,
},
```

#### Enhanced execute_let Method

Modified `execute_let` in `src/runtime/ir_runtime.rs` to implement the two-pass letrec strategy:

- Identifies `IrNode::Lambda` nodes in bindings
- Creates `FunctionPlaceholder` values for recursive references
- Resolves placeholders with `Function::IrLambda` instances

#### IR Lambda Function Calling

Added `call_ir_lambda` method to handle execution of IR-based lambda functions:

- Manages parameter binding with `IrEnvironment`
- Executes function body in closure environment
- Supports the same function calling semantics as AST functions

#### Updated Function Calling Logic

Enhanced `call_function` method to handle the new `Function::IrLambda` variant, maintaining compatibility with existing `Builtin` and `UserDefined` functions.

## Technical Details

### Environment Management

- Uses `IrEnvironment` instead of AST `Environment`
- Bindings are keyed by `NodeId` rather than symbol names
- Maintains proper closure capture for recursive functions

### Placeholder Strategy

- Uses the same `Value::FunctionPlaceholder(Rc<RefCell<Value>>)` approach as AST runtime
- Ensures mutual recursion works by allowing functions to reference each other during creation
- Provides consistent semantics across both runtimes

### IR vs AST Integration

The implementation bridges IR and AST representations:

- Stores IR nodes directly in `IrLambda` functions
- Maintains separate environments for IR execution
- Preserves type safety and performance benefits of the IR approach

## Benefits

1. **Consistency**: IR runtime now has the same recursive function capabilities as AST runtime
2. **Performance**: Maintains IR runtime's performance advantages while supporting recursion
3. **Completeness**: Enables full RTFS language feature support in both execution paths
4. **Correctness**: Implements proper letrec semantics for mutual recursion

## Testing

The existing recursive function tests in `tests/` should work with both AST and IR runtimes once IR conversion is fully implemented. The letrec implementation is ready for testing when the full IR pipeline is available.

## Future Work

1. **IR Converter Integration**: Ensure the IR converter properly handles recursive let bindings
2. **Performance Optimization**: Consider IR-specific optimizations for recursive calls
3. **Error Reporting**: Enhance error messages for IR lambda function calls
4. **Full Pipeline Testing**: Test recursive functions through the complete AST → IR → execution pipeline

## Files Modified

- `src/runtime/values.rs`: Added `Function::IrLambda` variant
- `src/runtime/ir_runtime.rs`: Implemented letrec support with two-pass strategy
  - `execute_let`: Enhanced for letrec semantics
  - `call_function`: Added `IrLambda` case
  - `call_ir_lambda`: New method for IR lambda execution

This implementation provides a solid foundation for recursive function support in the IR runtime, maintaining compatibility with existing code while enabling advanced language features.
