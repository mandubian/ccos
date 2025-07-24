# CCOS/RTFS AI Agent Instructions

## Project Overview
This is **CCOS (Cognitive Computing Operating System)** with **RTFS (Reason about The Fucking Spec)** - a language and architecture designed BY AI FOR AI agents. The project explores AI autonomy with formal governance, privilege separation, and verifiable execution.

## Critical Architecture Concepts

### 1. Separation of Powers Design
- **Arbiter** (AI Planner): Low-privilege cognitive engine that proposes but cannot act
- **Governance Kernel**: High-privilege validator that authorizes actions against Constitution
- **Orchestrator**: Deterministic executor for authorized plans
- All actions recorded in immutable **Causal Chain** for auditing

### 2. RTFS Language Paradigm
- **S-expression syntax** (Lisp-like) optimized for AI generation: `(intent :goal "analyze competitor")`
- **Three core object types**: `Intent` (why), `Plan` (how), `Action` (what happened)
- **Strong static typing** with immutable data structures
- **Capability-based security** through marketplace pattern

## Development Workflow

### Essential Commands (run from `rtfs_compiler/`)
```bash
# Interactive development
cargo run --bin rtfs-repl

# Run full test suite
cargo test

# Build optimized compiler
cargo build --release --bin rtfs-compiler

# Performance analysis
cargo run --bin rtfs-compiler -- --input file.rtfs --execute --show-timing --show-stats
```

### Test Structure
- **Integration tests**: `tests/integration_tests.rs` with `TestConfig` pattern
- **RTFS test files**: `tests/rtfs_files/*.rtfs` 
- **Test pattern**: `run_all_tests_for_file(&TestConfig::new("test_name"))`
- Tests validate parsing, type checking, execution, and error handling

### Runtime Architecture
- **Multiple strategies**: AST interpreter, IR compiler, hybrid fallback
- **Module system**: `src/runtime/mod.rs` with capability marketplace
- **Type system**: `src/ast.rs` with Value enum and strong typing
- **Security**: `src/runtime/security.rs` for capability validation

## Project-Specific Patterns

### 1. RTFS Object Builders
Use specialized builders for core RTFS 2.0 objects:
```rust
use crate::builders::{IntentBuilder, PlanBuilder, ActionBuilder};

let intent = IntentBuilder::new()
    .goal("competitive analysis")
    .constraint(":max-cost", "50.00")
    .build();
```

### 2. Capability Integration
Capabilities are discovered through marketplace, not hardcoded:
```rust
// In runtime/capability_marketplace.rs
let capability = marketplace.discover_capability(&capability_spec)?;
let result = capability.execute(&params).await?;
```

### 3. Error Handling Philosophy
All errors must be deterministic and auditable:
```rust
// Prefer Result<T, RuntimeError> over panics
// Log all errors to causal chain for governance review
```

## Critical Integration Points

### Parser Grammar
- **File**: `src/rtfs.pest` (Pest grammar)
- **Special forms**: Handled in `src/parser/special_forms.rs`
- **Comments**: Multi-line and documentation supported
- **Metadata**: Delegation annotations like `:local-model "gpt-4"`

### Type System
- **Core types**: Int, Float, String, Bool, Vector, Map, Function
- **RTFS objects**: Intent, Plan, Action, Resource, Capability
- **Type checking**: Enforced at compile-time with inference

### Module System
- **Async loading**: `src/runtime/module_runtime.rs`
- **Cross-module imports**: Resolved at runtime
- **Security boundaries**: Modules isolated by governance kernel

## Build & Debug Guidelines

### Performance Considerations
- **Sub-millisecond compilation** for simple expressions (300-550μs target)
- **IR optimization** with configurable levels: `--opt-level aggressive|basic|none`
- **Benchmark frequently** using `:bench` in REPL

### Common Pitfalls
1. **Don't bypass capability system** - always use marketplace for external calls
2. **Maintain immutability** - all data structures should be immutable by default
3. **Test both runtime strategies** - AST and IR may behave differently
4. **Validate RTFS syntax** - use `cargo run --bin rtfs-repl` to test expressions

### Debug Workflow
1. **REPL first**: Test expressions interactively with `:ast` and `:ir` visualization
2. **Integration tests**: Add to `tests/integration_tests.rs` following existing patterns
3. **Error tracing**: Use `RuntimeError` with full context for governance audit trail

## Key Files for Understanding
- `plan.md`: Original AI language design specification
- `README.md`: Architecture overview and governance model
- `src/lib.rs`: Main module exports and public API
- `src/runtime/evaluator.rs`: Core execution engine
- `rtfs_compiler/examples/`: Working RTFS code examples
