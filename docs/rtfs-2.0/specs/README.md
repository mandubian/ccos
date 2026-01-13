# RTFS 2.0 Specifications (New)

This directory contains the **rewritten RTFS 2.0 language specifications** based on the actual implemented codebase after the CCOS-RTFS decoupling migration.

## Purpose

The original specifications in `./specs/` were written before the decoupling and did not accurately reflect the final architecture. These new specifications are derived from:

- **Codebase Analysis**: Direct examination of AST, runtime, and grammar files
- **Architectural Clarity**: Understanding gained from CCOS-RTFS decoupling
- **Implementation Truth**: Specifications match what was actually built

## Status Legend

**⚠️ IMPORTANT**: This documentation is being cleaned up to ensure accuracy. Some specifications describe design targets or aspirational features that are not yet fully implemented. Each specification file now includes an "Implementation Status" section indicating what is implemented versus planned.

**Legend**:
- ✅ **Implemented**: Features fully implemented and tested
- ⚠️ **Partial**: Partially implemented or basic support exists
- 🚧 **Design**: Design specification; implementation in progress or planned
- ❌ **Not Implemented**: Described but not yet implemented

**Verification Status**: Examples in this documentation are being validated against the actual implementation. The [Implementation Status Guide](../guides/implementation-status.md) is a work-in-progress tracker; when in doubt, treat the RTFS source (`rtfs/src/`) as the ground truth.

## Specifications Index

Below is the complete list of RTFS 2.0 specifications. Each document focuses on a specific aspect of the language or its runtime environment.

| # | Specification | Description & Focus |
|---|---------------|---------------------|
| 00 | [Philosophy](00-philosophy.md) | Core tenets: Purity, LLM-Native, Governance |
| 01 | [Language Overview](01-language-overview.md) | High-level summary of language features and syntax |
| 02 | [Syntax & Grammar](02-syntax-and-grammar.md) | Formal EBNF and S-expression structure |
| 03 | [Core Syntax & Data Types](03-core-syntax-data-types.md) | Literals, Lists, Vectors, Maps, Keywords |
| 04 | [Evaluation & Runtime](04-evaluation-and-runtime.md) | Scopes, Closures, TCO, Jump-based execution |
| 04b| [Host Boundary](04-host-boundary.md) | `ExecutionOutcome`, Requests, and Responses |
| 05 | [Pattern Matching](05-pattern-matching-destructuring.md) | Destructuring and `match` expression |
| 07 | [Module System](07-module-system.md) | `module`, `import`, exports, module registry |
| 08 | [Macro System](08-macro-system.md) | Metaprogramming, `defmacro`, Quasiquotation |
| 09 | [Streaming Capabilities](09-streaming-capabilities.md) | Streaming outcomes and host-mediated streams |
| 10 | [Standard Library](10-standard-library.md) | Comprehensive function reference for pure operations |
| 11 | [Architecture Overview](11-architecture-analysis.md) | System components, diagrams, and LLM-fit analysis |
| 12 | [IR & Compilation](12-ir-and-compilation.md) | Lowering to S-Expression IR and Bytecode |
| 13 | [Type System](13-type-system.md) | Formal typing rules, Subtyping, and Validation |
| 14 | [Concurrency Model](14-concurrency-model.md) | Step orchestration and host-mediated parallelism |
| 15 | [Error Handling](15-error-handling-recovery.md) | `try/catch/finally` and error propagation |
| 16 | [Security Model](16-security-model.md) | Sandboxing, Capabilities, and Governance |
| 17 | [Performance](17-performance-optimization.md) | Memory management and IR optimizations |
| 18 | [Interoperability](18-interoperability.md) | JSON, MCP Tools, and Host Integration |

---

## Technical Architecture Overview

### Human-LLM-System Synergy
- **Humans Specify**: High-level intents in natural language
- **LLMs Generate**: Executable RTFS workflows that fulfill those intents
- **CCOS Governs**: All external interactions with security and auditability
- **Systems Execute**: Verifiable autonomy through pure functional execution

This creates a **conversational programming paradigm** where LLMs can be "programmed by conversation" to generate trustworthy, autonomous task execution logic.

## Implementation Status

These specifications document both **implemented features** and **design targets** for RTFS 2.0. The table below shows the current implementation status of each core engine component:

| Component | Status | Notes |
|-----------|--------|-------|
| **AST** | ✅ **Implemented** | Comprehensive expression types |
| **Parser** | ✅ **Implemented** | Pest grammar with full S-expression support |
| **Runtime** | ✅ **Implemented** | Yield-based host boundary with `ExecutionOutcome::RequiresHost` |
| **Type System** | ⚠️ **Partial** | Strong runtime validation + IR type checker exists; inference/refinement coverage is still evolving |
| **Standard Library** | ✅ **Implemented** | Pure functions only; effectful operations via capabilities |
| **Host Integration** | ✅ **Implemented** | CCOS capability invocation with security context |
| **Module System** | ✅ **Implemented** | `module` + `import` + `(:exports [...])` supported; docstrings/versioning/tooling are still partial |
| **Macro System** | ✅ **Implemented** | Full implementation with `defmacro`, quasiquote |
| **Pattern Matching** | ✅ **Implemented** | Comprehensive destructuring in AST and runtime |
| **Concurrency / Steps** | ⚠️ **Partial** | `step-parallel` exists, but evaluator execution is currently sequential with deterministic aggregation; host-mediated parallelism is still being fleshed out |
| **Streaming** | ⚠️ **Partial** | Primarily host-mediated via capabilities; native streaming operators are not yet part of the core language |
| **Metadata Support** | ⚠️ **Partial** | `^{:runtime.* ...}` hints propagate to Host; general metadata is not preserved as a runtime value / through IR compilation |

## Future Directions & Implementation Gaps

The following areas represent future work or partially implemented features. These gaps are being addressed in ongoing development:

### Core Language Features
- **Macro System Enhancement**: Advanced hygiene mechanisms, debugging tools, and macro documentation
- **Module System Ergonomics**: Better module metadata (docstrings, versions), dependency tooling, packaging/registry conventions, and stronger import/export ergonomics
- **Type System Enhancement**: More complete refinement predicates, inference improvements, and clearer “compile-time vs runtime validation” boundaries
- **Concurrency Semantics**: Host-mediated parallel execution semantics beyond sequential evaluation (scheduling, determinism, merge policies, error aggregation)
- **Streaming Primitives**: Native streaming operators (currently host-mediated via capabilities)

### LLM-Driven Development Priorities
- **Type System Simplification**: Reduce complexity for reliable LLM generation
- **Host Call Optimization**: Streamline governance for common task patterns
- **Error Handling Standardization**: Predictable patterns for LLM-generated code
- **Intent-Aware Modules**: Code organization around task boundaries

### Performance & Usability
- **Host Boundary Optimization**: Reduce latency for LLM-generated workflows
- **Evaluation Speed**: Fast feedback for interactive LLM development
- **Memory Efficiency**: Optimize for LLM context window constraints
- **Compiler Optimizations**: Additional IR optimizations and bytecode improvements

### Ecosystem Growth
- **Standard Library Expansion**: Additional pure functions and data structure utilities
- **LLM Integration Tools**: Code generation assistance and validation
- **Task Templates**: Reusable patterns for common intent types
- **Workflow Composition**: High-level constructs for combining task steps
- **Development Tooling**: Enhanced REPL, debugging, and profiling tools

## Reading Order

For new readers:
1. Start with **00-philosophy.md** for conceptual foundation
2. Read **01-language-overview.md** for language basics
3. Study **04-evaluation-and-runtime.md** for execution model
4. Read **13-type-system.md** for the formal type system specification
5. Explore **04-host-boundary.md** for CCOS integration
6. Review **11-architecture-analysis.md** for LLM-driven development insights

For LLM developers:
- Focus on **02-syntax-and-grammar.md** for generation patterns
- Study **04-host-boundary.md** for task execution
- Review **10-standard-library.md** for available operations
- See **[Type Checking Guide](../guides/type-checking-guide.md)** for practical type usage

## Contributing

When modifying RTFS:
1. Update these specifications to reflect changes
2. Ensure specifications match implementation
3. Add rationale for architectural decisions
4. Include performance and security implications

## Migration from RTFS 1.x

RTFS 2.0 represents a significant evolution:
- **Purity Enforcement**: All side effects must go through host
- **Type System**: Structural typing replaces nominal typing
- **Security**: Mandatory governance for all external operations
- **Simplicity**: Reduced core language with host extensibility

Migration tools and compatibility layers should be developed to ease transition.
