# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CCOS (Cognitive Computing Operating System) is an AI-designed architecture for autonomous agents with formal governance and verifiable execution. The system uses RTFS (Reason about The Fucking Spec), a Lisp-like S-expression language optimized for AI agents to reason about goals, plans, and actions.

RTFS is the core language used by CCOS to exchange data between entitis so try to use it and avoid JSON by default.

## Key Architecture Concepts

### Separation of Powers Design
- **Cognitive Engine (Arbiter)**: Low-privilege AI planner that proposes but cannot act directly
- **Governance Kernel**: High-privilege validator that authorizes actions against Constitution 
- **Orchestrator**: Deterministic executor for authorized plans
- **Causal Chain**: Immutable audit trail of all actions

### RTFS Core Objects
- **Intent** ("why"): High-level goals with constraints and success criteria
- **Plan** ("how"): Executable scripts to achieve intents
- **Action** ("what happened"): Immutable records of execution steps


## Key Specifications


### CCOS Specifications Location

The complete CCOS specifications can be found in the [docs/ccos/specs/](mdc:docs/ccos/specs/) directory. Key specification documents include:

- **[000-ccos-architecture.md](mdc:docs/ccos/specs/000-ccos-architecture.md)** - Complete system architecture overview
- **[001-intent-graph.md](mdc:docs/ccos/specs/001-intent-graph.md)** - Intent Graph specification and management
- **[002-plans-and-orchestration.md](mdc:docs/ccos/specs/002-plans-and-orchestration.md)** - Plans and orchestration system
- **[003-causal-chain.md](mdc:docs/ccos/specs/003-causal-chain.md)** - Causal Chain immutable audit ledger
- **[004-capabilities-and-marketplace.md](mdc:docs/ccos/specs/004-capabilities-and-marketplace.md)** - Capabilities and marketplace system
- **[014-step-special-form-design.md](mdc:docs/ccos/specs/014-step-special-form-design.md)** - Step special form for CCOS integration

## RTFS 2.0 Specifications Location

The complete RTFS 2.0 specifications can be found in the [docs/rtfs-2.0/specs/](mdc:docs/rtfs-2.0/specs/) directory. Key specification documents include:

- **[README.md](mdc:docs/rtfs-2.0/specs/README.md)** - Complete specification overview and index
- **[01-language-features.md](mdc:docs/rtfs-2.0/specs/01-language-features.md)** - Core language features and implementation status
- **[10-formal-language-specification.md](mdc:docs/rtfs-2.0/specs/10-formal-language-specification.md)** - Complete formal language specification
- **[03-object-schemas.md](mdc:docs/rtfs-2.0/specs/03-object-schemas.md)** - Object schema definitions and validation
- **[06-capability-system.md](mdc:docs/rtfs-2.0/specs/06-capability-system.md)** - Complete capability system architecture


## Development Commands

All development happens in the `rtfs_compiler/` directory:

```bash
# Build and test
cargo build --release
cargo test

# Interactive development
cargo run --bin rtfs-repl

# Run production compiler 
cargo run --bin rtfs-compiler -- --input file.rtfs --execute

# Specific test execution
cargo test --test integration_tests -- --nocapture --test-threads 1

# Performance analysis
cargo run --bin rtfs-compiler -- --input file.rtfs --execute --show-timing --show-stats

# For examples requiring an agent config, DO NOT USE config_minimal but this one
cargo run --example EXAMPLE -- --config ../config/agent_config.toml
```

## Codebase Structure

### Core Modules
- `src/lib.rs`: Main library exports and public API
- `src/parser/`: RTFS parser with Pest grammar (`src/rtfs.pest`)
- `src/runtime/`: Execution engine with multiple strategies (AST, IR)
- `src/ast.rs`: Core AST types and Value enum
- `src/ir/`: Intermediate representation with optimization
- `src/ccos/`: CCOS components (Cognitive Engine, Orchestrator, etc.)

### Key Files
- `src/runtime/evaluator.rs`: Core execution engine
- `src/runtime/ir_runtime.rs`: IR optimized execution engine
- `src/runtime/capability_marketplace.rs`: Capability discovery system
- `src/builders/`: RTFS 2.0 object builders (Intent, Plan, Action)
- `tests/rtfs_files/`: RTFS test programs
- `examples/`: Working RTFS code examples

### Runtime Architecture
- **Multi-strategy execution**: AST interpreter, IR compiler, hybrid fallback
- **Module system**: Async loading with security boundaries
- **Type system**: Strong static typing with immutable data structures
- **Capability-based security**: All external interactions through marketplace

## Development Patterns

### RTFS Object Creation
Use specialized builders for core objects:
```rust
use rtfs_compiler::builders::{IntentBuilder, PlanBuilder, ActionBuilder};

let intent = IntentBuilder::new()
    .goal("competitive analysis")
    .constraint(":max-cost", "50.00")
    .build();
```

### Capability Integration
Never hardcode external calls - always use marketplace:
```rust
let capability = marketplace.discover_capability(&capability_spec)?;
let result = capability.execute(&params).await?;
```

### Error Handling
All errors must be deterministic and auditable:
- Use `Result<T, RuntimeError>` over panics
- Log all errors to causal chain for governance review
- Maintain full context for audit trail

### Testing Philosophy
- Integration tests in `tests/integration_tests.rs` with `TestConfig` pattern
- RTFS test files in `tests/rtfs_files/*.rtfs`
- Test both AST and IR runtime strategies
- Validate parsing, type checking, execution, and error handling

## Performance Considerations

- Target sub-millisecond compilation for simple expressions (300-550μs)
- IR optimization with configurable levels: `--opt-level aggressive|basic|none`
- Use `:bench` in REPL for performance testing
- Test frequently with `cargo run --bin rtfs-repl`

## Security Model

- **Zero-trust architecture**: All components verify signatures
- **Privilege separation**: AI cannot act without explicit authorization
- **Immutable audit trail**: All actions recorded in Causal Chain
- **Capability marketplace**: Secure, versioned function discovery
- **Constitutional governance**: Formal rules enforced by kernel

## Common Pitfalls

1. **Don't bypass capability system** - always use marketplace for external calls
2. **Maintain immutability** - all data structures should be immutable by default  
3. **Test both runtime strategies** - AST and IR may behave differently
4. **Validate RTFS syntax** - use REPL to test expressions interactively

## Debugging Workflow

1. **REPL first**: Test expressions with `:ast` and `:ir` visualization
2. **Integration tests**: Add to `tests/integration_tests.rs` following existing patterns
3. **Error tracing**: Use `RuntimeError` with full context for governance audit
4. **Performance profiling**: Use `--show-timing --show-stats` flags

## Important Notes

- This is a research project exploring AI autonomy with formal governance
- The architecture prioritizes verifiability and auditability over raw performance
- RTFS is designed for AI agents, not human programmers
- All external integrations must go through the capability marketplace
- The system enforces constitutional rules that cannot be bypassed