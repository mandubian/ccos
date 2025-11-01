# RTFS 2.0 Specifications (New)

This directory contains the **rewritten RTFS 2.0 language specifications** based on the actual implemented codebase after the CCOS-RTFS decoupling migration.

## Purpose

The original specifications in `../specs/` were written before the decoupling and did not accurately reflect the final architecture. These new specifications are derived from:

- **Codebase Analysis**: Direct examination of AST, runtime, and grammar files
- **Architectural Clarity**: Understanding gained from CCOS-RTFS decoupling
- **Implementation Truth**: Specifications match what was actually built

## Philosophy

RTFS 2.0 is a **pure functional language** with a **strict host boundary**:

- **Pure Kernel**: All RTFS code is referentially transparent
- **Host Yielding**: Side effects are mediated through CCOS capabilities
- **Security First**: Mandatory governance and audit trails
- **Minimal & Extensible**: Small core with powerful extension mechanisms

## Design Purpose: LLM-Native Task Execution

RTFS 2.0 is architected as a **language designed for LLMs to generate data structures and execution logic** that represents **task fulfillment workflows for user intents**:

### LLM-Driven Code Generation
- **Pure Kernel**: Safe environment for LLM-generated logic without side effect concerns
- **S-Expression Syntax**: Uniform, programmable structure that LLMs can reliably parse and generate
- **Type System**: Safety guardrails that catch LLM generation errors while remaining optional
- **Homoiconic Design**: Code-as-data enables LLMs to analyze and transform their own outputs

### Intent-Based Task Execution
- **Host Boundary**: Clear separation between reasoning logic and external task actions
- **Capability System**: Governed access to external services for verifiable task fulfillment
- **Causal Chain**: Complete audit trail of LLM-generated workflow execution
- **Governance**: Security validation of generated code against user intent

### Human-LLM-System Synergy
- **Humans Specify**: High-level intents in natural language
- **LLMs Generate**: Executable RTFS workflows that fulfill those intents
- **CCOS Governs**: All external interactions with security and auditability
- **Systems Execute**: Verifiable autonomy through pure functional execution

This creates a **conversational programming paradigm** where LLMs can be "programmed by conversation" to generate trustworthy, autonomous task execution logic.

## Specification Structure

### 00-philosophy.md
Core principles and architectural foundations of RTFS 2.0.

### 01-syntax-and-grammar.md
Complete syntax reference, grammar rules, and language constructs.

### 02-evaluation-and-runtime.md
Evaluation model, scoping rules, and runtime architecture.

### 13-type-system.md (UPDATED - Formal Specification)
**Complete formal type system** with subtyping, bidirectional type checking, soundness proofs, and theoretical foundations. Includes:
- Formal type grammar and inference rules
- Subtyping relation with 12 axioms
- Progress and Preservation theorems
- Algorithmic type checking specification
- References to Pierce, Cardelli, Davies & Pfenning

### 04-host-boundary-and-capabilities.md
Host interaction mechanisms, capability system, and security model.

### 05-macro-system.md
Compile-time metaprogramming, quasiquote, and hygienic macros.

### 06-standard-library.md
Comprehensive function reference for pure and impure operations.

### 07-architecture-analysis.md
Critical analysis of strengths, weaknesses, and future directions.

## Key Architectural Insights

### Pure Kernel Design
RTFS maintains **referential transparency** by yielding control to CCOS for all side effects. This enables:
- Deterministic testing and reasoning
- Safe composition of pure and effectful code
- Mandatory security governance

### LLM-Native Architecture
The design prioritizes **LLM comprehension and generation**:
- **S-Expression Syntax**: Reduces generation errors through uniform structure
- **Host Boundary Clarity**: Makes intent fulfillment logic explicit and auditable
- **Type System Safety**: Provides guardrails for LLM-generated code
- **Macro Extensibility**: Enables LLMs to create domain-specific constructs

### Host Boundary Mechanism
The `ExecutionOutcome` enum implements **control flow inversion**:
- `Complete(value)`: Pure computation finished
- `RequiresHost(call)`: Host intervention needed

### Security by Design
Every host call includes:
- **Capability ID**: Fully qualified operation identifier
- **Security Context**: Agent, intent, and permission information
- **Causal Context**: Audit trail for governance
- **Metadata**: Performance and reliability hints

### Type System Approach
**Structural typing** with **runtime validation**:
- Optional type annotations
- Refinement types with logical predicates
- Union/intersection types for composition
- Gradual adoption without breaking changes

## Relationship to CCOS

RTFS serves as CCOS's **computational substrate**:

- **Pure Logic Layer**: Deterministic computation
- **Capability Marketplace**: Service discovery and invocation
- **Governance Kernel**: Security and audit infrastructure
- **Causal Chain**: Immutable audit trails

## Implementation Status

These specifications reflect the **implemented architecture** as of the decoupling migration. Key components:

- ✅ **AST**: Comprehensive expression types
- ✅ **Runtime**: Yield-based host boundary
- ✅ **Type System**: Structural types with predicates
- ✅ **Macro System**: Basic compile-time transformation
- ✅ **Standard Library**: Pure and impure function categories
- ✅ **Host Integration**: CCOS capability invocation

## Future Directions

The architecture analysis identifies areas for improvement:

### LLM-Driven Development Priorities
- **Type System Simplification**: Reduce complexity for reliable LLM generation
- **Host Call Optimization**: Streamline governance for common task patterns
- **Error Handling Standardization**: Predictable patterns for LLM-generated code
- **Intent-Aware Modules**: Code organization around task boundaries

### Performance & Usability
- **Host Boundary Optimization**: Reduce latency for LLM-generated workflows
- **Evaluation Speed**: Fast feedback for interactive LLM development
- **Memory Efficiency**: Optimize for LLM context window constraints

### Ecosystem Growth
- **LLM Integration Tools**: Code generation assistance and validation
- **Task Templates**: Reusable patterns for common intent types
- **Workflow Composition**: High-level constructs for combining task steps

## Reading Order

For new readers:
1. Start with **00-philosophy.md** for conceptual foundation
2. Read **01-language-overview.md** for language basics
3. Study **04-evaluation-and-runtime.md** for execution model
4. Read **13-type-system.md** for the formal type system specification
5. Explore **07-host-boundary-and-capabilities.md** for CCOS integration
6. Review **11-architecture-analysis.md** for LLM-driven development insights

For LLM developers:
- Focus on **02-syntax-and-grammar.md** for generation patterns
- Study **07-host-boundary-and-capabilities.md** for task execution
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

Migration tools and compatibility layers should be developed to ease transition.</content>
<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs-new/README.md