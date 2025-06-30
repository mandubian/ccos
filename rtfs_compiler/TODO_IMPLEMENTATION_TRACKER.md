# RTFS Compiler - Implementation Tracker

This document tracks all unimplemented functions, TODO items, and missing implementations in the RTFS compiler codebase.

## Critical Unimplemented Functions

### Runtime Evaluator (`src/runtime/evaluator.rs`)

#### Pattern Matching

- **Line 712**: `unimplemented!()` in `eval_fn` - Complex pattern matching in function parameters
- **Line 758**: `unimplemented!()` in `eval_defn` - Complex pattern matching in function parameters
- **Line 1211**: Complex match pattern matching not yet implemented
- **Line 1229**: Complex catch pattern matching not yet implemented

#### Agent Discovery

- **Line 846**: TODO: Implement agent discovery in `eval_discover_agents`

#### Type System

- **Line 1241**: TODO: Implement actual type coercion logic in `coerce_value_to_type`

#### IR Integration

- **Line 1450**: TODO: Implement IR function calling

### Runtime Values (`src/runtime/values.rs`)

#### Expression Conversion

- **Line 234**: `unimplemented!()` in `From<Expression>` implementation - Missing conversion for non-literal expressions

### IR Runtime (`src/runtime/ir_runtime.rs`)

#### IR Node Execution

- **Line 172**: Generic "Execution for IR node is not yet implemented" - Multiple IR node types need implementation

## Standard Library Unimplemented Functions (`src/runtime/stdlib.rs`)

### File Operations

- **Line 1983-1999**: JSON parsing not implemented
- **Line 1990-1992**: JSON serialization not implemented
- **Line 1997-1999**: File operations not implemented (read-file)
- **Line 2004-2006**: File operations not implemented (write-file)
- **Line 2011-2013**: File operations not implemented (append-file)
- **Line 2018-2020**: File operations not implemented (delete-file)

### HTTP Operations

- **Line 2048-2050**: HTTP operations not implemented

### Agent System

- **Line 2076**: Agent discovery not implemented
- **Line 2081**: Task coordination not implemented
- **Line 2139**: Agent discovery and assessment not implemented
- **Line 2144**: System baseline establishment not implemented

### Higher-Order Functions (Previously Disabled)

- **Line 857-865**: Map and filter functions were previously disabled but now implemented with evaluator support

## IR Converter Unimplemented Features (`src/ir/converter.rs`)

### Source Location Tracking

- **Line 825**: TODO: Add source location
- **Line 830**: TODO: Add source location
- **Line 840**: TODO: Add source location
- **Line 895**: TODO: Add source location
- **Line 904**: TODO: Add source location

### Type System

- **Line 861**: TODO: Add specific timestamp type
- **Line 862**: TODO: Add specific UUID type
- **Line 863**: TODO: Add specific resource handle type
- **Line 1727**: TODO: Implement remaining type conversions

### Capture Analysis

- **Line 1058**: TODO: Implement capture analysis
- **Line 1324**: TODO: Capture analysis placeholder

### Pattern Matching

- **Line 1260**: TODO: Handle other patterns in params
- **Line 1289**: TODO: Handle other patterns in variadic params
- **Line 1594**: TODO: Handle options

## Parser Unimplemented Features

### Expression Parser (`src/parser/expressions.rs`)

- **Line 104**: `build_expression not implemented for rule` - Generic unimplemented expression building

### Top-Level Parser (`src/parser/toplevel.rs`)

- **Line 213**: TODO: Implement import definition parsing
- **Line 225**: TODO: Parse docstring if present

## AST Features (`src/ast.rs`)

- **Line 76**: TODO: Consider :or { default-val literal } if needed for destructuring

## Integration Tests (`src/integration_tests.rs`)

- **Line 1588**: TODO: Add support for ` (quasiquote) and , (unquote) syntax to grammar
- **Line 1599**: TODO: Add support for ` (quasiquote), , (unquote), and ,@ (unquote-splicing) syntax to grammar

## Development Tooling (`src/development_tooling.rs`)

- **Line 30-32**: IrStrategy execution not yet implemented
- **Line 568**: Custom expectations not implemented

## Error Handling (`src/runtime/error.rs`)

- **Line 221**: TODO: Re-enable IR converter integration when ir_converter module is available
- **Line 267**: TODO: Re-enable when IR is integrated
- **Line 273**: TODO: Re-enable when IR is integrated

## Summary Demo (`src/summary_demo.rs`)

- **Line 24**: TODO: Fix enhanced_ir_demo compilation

## Priority Implementation Order

### High Priority (Core Functionality)

1. **Pattern Matching in Functions** - Lines 712, 758 in evaluator.rs
2. **IR Node Execution** - Line 172 in ir_runtime.rs
3. **Expression Conversion** - Line 234 in values.rs
4. **Type Coercion** - Line 1241 in evaluator.rs

### Medium Priority (Standard Library)

1. **File Operations** - Lines 1997-2020 in stdlib.rs
2. **JSON Operations** - Lines 1983-1992 in stdlib.rs
3. **HTTP Operations** - Lines 2048-2050 in stdlib.rs

### Low Priority (Advanced Features)

1. **Agent System** - Lines 2076-2144 in stdlib.rs
2. **Source Location Tracking** - Lines 825-904 in converter.rs
3. **Capture Analysis** - Lines 1058, 1324 in converter.rs
4. **Quasiquote Syntax** - Lines 1588-1599 in integration_tests.rs

## Notes

- Many functions marked as "not implemented" are actually placeholders that return errors or default values
- The IR runtime has the most unimplemented functionality, as it's a newer addition
- Pattern matching is a recurring theme across multiple modules
- File and HTTP operations are common missing stdlib functions
- Agent discovery and coordination features are planned but not yet implemented

## Status Updates

- **Higher-order functions** (map, filter, reduce) were recently implemented with full evaluator support
- **Module loading** was recently fixed and is now working
- **Letrec** was removed from grammar and replaced with recursion detection in regular `let`
- **IR integration** is partially complete but many node types still need implementation
