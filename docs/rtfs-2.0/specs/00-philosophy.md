# RTFS 2.0 Philosophy

## Core Principles

RTFS 2.0 is a **pure functional language** designed for **secure, auditable computation** within the CCOS (Cognitive Cognitive Operating System) ecosystem. Its design emphasizes **minimalism**, **extensibility**, and a **clear host boundary** that enables deterministic execution while delegating all side effects to the host environment.

## Pure Kernel with Host Boundary

RTFS operates as a **pure functional kernel** that yields control to its host (CCOS) whenever non-pure operations are required. This design achieves:

- **Deterministic Execution**: All RTFS code is referentially transparent
- **Security by Design**: Side effects are mediated through CCOS governance
- **Auditability**: Every host interaction is tracked in the causal chain
- **Composability**: Pure functions can be safely combined and reasoned about

### Control Flow Inversion

RTFS uses **yield-based control flow inversion** where pure computation alternates with host-mediated effects:

```clojure
;; Pure RTFS computation
(let [x (+ 1 2)
      y (* x 3)]
  ;; Yield to host for side effect
  (call :ccos.io/println "Result:" y))
```

The runtime returns `ExecutionOutcome::RequiresHost(HostCall)` when encountering operations that require external capabilities, allowing CCOS to make governance decisions.

## Minimal Extensible Language

RTFS provides a **small but powerful core** that can be extended through:

- **Macros**: Compile-time code transformation
- **Host Capabilities**: Runtime extension through CCOS marketplace
- **Type System**: Structural typing with refinement predicates
- **Pattern Matching**: Destructuring and conditional logic

### Homoiconic Design

RTFS uses **s-expressions** as its primary representation, enabling:

- **Code as Data**: Programs can manipulate their own structure
- **Macro System**: Powerful compile-time metaprogramming
- **Simple Syntax**: Uniform representation for all language constructs

## Security and Governance

Every RTFS execution is governed by CCOS security policies:

- **Capability-Based Security**: All external operations require explicit capabilities
- **Causal Chain Tracking**: Every host call includes audit context
- **Runtime Context**: Security metadata flows through execution
- **Delegation Decisions**: CCOS can approve, deny, or modify host calls

## Type System Philosophy

RTFS employs **structural typing** focused on runtime verification:

- **Refinement Types**: Base types with logical predicates
- **Union/Intersection Types**: Flexible composition
- **Optional Types**: Explicit nullability handling
- **Resource Types**: Safe resource management

## Design Goals

1. **Simplicity**: Small core language that's easy to understand and implement
2. **Security**: All side effects are mediated and auditable
3. **Performance**: Efficient execution with minimal overhead
4. **Extensibility**: Rich ecosystem through host capabilities and macros
5. **Correctness**: Strong typing and pure functional semantics

## Relationship to CCOS

RTFS is the **computational substrate** of CCOS, providing:

- **Pure Logic Layer**: Deterministic computation
- **Host Integration**: Clean boundary for side effects
- **Capability Marketplace**: Runtime service discovery
- **Governance Kernel**: Security and audit infrastructure

This separation enables CCOS to provide **cognitive services** while RTFS ensures **computational integrity**.</content>
<parameter name="filePath">/home/mandubian/workspaces/mandubian/ccos/docs/rtfs-2.0/specs-new/00-philosophy.md