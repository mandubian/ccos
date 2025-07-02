# RTFS 2.0 Object Builders: Fluent API & LLM Integration

## Purpose

RTFS 2.0 object builders provide a type-safe, chainable, and ergonomic API for constructing RTFS objects (Intent, Plan, Action, Capability, Resource, Module) in Rust. This enables both developers and LLMs to generate valid RTFS 2.0 code without requiring deep knowledge of the syntax, and supports validation, error reporting, and conversion to RTFS source.

## Fluent Interface Pattern

Each builder exposes chainable methods for setting properties, constraints, and relationships. Validation is performed at build time, and helpful suggestions are available for incomplete objects.

### Example: IntentBuilder

```rust
use rtfs_compiler::builders::{IntentBuilder, Priority, Constraint};

let intent = IntentBuilder::new("analyze-sales")
    .with_goal("Analyze sales data for Q2")
    .with_priority(Priority::High)
    .with_constraint(Constraint::MaxCost(50.0))?
    .build()?;

let rtfs_code = intent.to_rtfs()?;
```

### Example: PlanBuilder

```rust
use rtfs_compiler::builders::{PlanBuilder, PlanStep, Priority};

let step = PlanStep::new("step1", "load-data").with_parameter("file", "sales.csv");
let plan = PlanBuilder::new("sales-plan")
    .for_intent("analyze-sales")
    .with_step(step)
    .with_priority(Priority::Medium)
    .build()?;
```

### Example: ActionBuilder

```rust
use rtfs_compiler::builders::ActionBuilder;

let action = ActionBuilder::new("load-data")
    .for_capability("file-reader")
    .with_parameter("path", "sales.csv")
    .with_cost(5.0)?
    .build()?;
```

### Example: CapabilityBuilder

```rust
use rtfs_compiler::builders::{CapabilityBuilder, FunctionSignature, Parameter};

let signature = FunctionSignature::new("load", "csv")
    .with_parameter(Parameter::new("path", "string", true));
let capability = CapabilityBuilder::new("file-reader")
    .with_provider("core")
    .with_function_signature(signature)
    .build()?;
```

### Example: ResourceBuilder

```rust
use rtfs_compiler::builders::{ResourceBuilder, AccessControl, ResourceLifecycle};

let ac = AccessControl::new("user1", vec!["read".to_string()], true);
let lc = ResourceLifecycle::new("2025-01-01T00:00:00Z", "active");
let resource = ResourceBuilder::new("sales-data")
    .with_type("file")
    .with_access_control(ac)
    .with_lifecycle(lc)
    .with_property("path", "sales.csv")
    .build()?;
```

### Example: ModuleBuilder

```rust
use rtfs_compiler::builders::ModuleBuilder;

let module = ModuleBuilder::new("sales-module")
    .with_export("analyze-sales")
    .with_version("1.0.0")
    .build()?;
```

## LLM Integration

- Builders support `.from_natural_language(prompt)` for intent generation from text.
- Validation and suggestion methods provide LLM-friendly error messages.
- Progressive learning: LLMs can start with simple objects and add complexity incrementally.
- All builders support `.to_rtfs()` for generating valid RTFS 2.0 source code.

## Error Handling & Validation

- All builders validate required fields and constraints.
- Errors are returned as `BuilderError` or a list of suggestions.
- Suggestions for missing fields are available via `.suggest_completion()`.

## Summary

The builder API makes RTFS 2.0 programmatic object creation robust, accessible, and LLM-friendly, supporting both developer productivity and AI-driven code generation.
