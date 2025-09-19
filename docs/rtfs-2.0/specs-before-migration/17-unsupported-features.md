# RTFS 2.0 Unsupported Features

## Overview

This document tracks features that are currently **not implemented** in RTFS 2.0 but have been identified as potentially valuable additions. These features were discovered during end-to-end testing and have been commented out in the test suite until proper implementation.

## Feature Categories

### 1. Literal Syntax Features

#### Character Literals
**Status**: Not implemented
**Syntax**: `\a`, `\newline`, `\space`
**Description**: Character literals for representing single characters
**Use Case**: String manipulation, character-based operations
**Priority**: High
**GitHub Issue**: [#138](https://github.com/mandubian/ccos/issues/138)

#### Regular Expression Literals
**Status**: Not implemented
**Syntax**: `#"[a-zA-Z]+"`
**Description**: Built-in regular expression support
**Use Case**: Pattern matching and text processing
**Priority**: Medium
**GitHub Issue**: [#139](https://github.com/mandubian/ccos/issues/139)

#### Symbol Quote Syntax
**Status**: Not implemented
**Syntax**: `'symbol-literal`
**Description**: Explicit symbol creation syntax
**Use Case**: Creating symbols programmatically, avoiding variable resolution
**Priority**: High
**GitHub Issue**: [#140](https://github.com/mandubian/ccos/issues/140)

### 2. Advanced Type System Features

#### Type Constraints with Predicates
**Status**: Not implemented
**Syntax**: `[:and :int [:> 0]]`
**Description**: Complex type constraints using predicate functions
**Use Case**: Runtime type validation with custom constraints
**Priority**: Medium
**GitHub Issue**: [#141](https://github.com/mandubian/ccos/issues/141)

#### Advanced Type Validation
**Status**: Partially implemented
**Syntax**: Range-based constraints like `[:>= 0] [:<= 100]`
**Description**: Range and complex validation rules
**Use Case**: Input validation, business logic constraints
**Priority**: Medium
**GitHub Issue**: [#142](https://github.com/mandubian/ccos/issues/142)

### 3. Collection Operations

#### Map Filtering with Destructuring
**Status**: Not implemented
**Syntax**: `(filter (fn [[k v]] (> v 5)) map)`
**Description**: Pattern matching in filter predicates for maps
**Use Case**: Complex filtering logic on key-value pairs
**Priority**: High
**GitHub Issue**: [#143](https://github.com/mandubian/ccos/issues/143)

#### Vector Comprehensions
**Status**: Not implemented
**Syntax**: `(for [x [1 2 3] y [10 20]] (+ x y))`
**Description**: List comprehension syntax for generating collections
**Use Case**: Declarative collection transformations
**Priority**: Medium
**GitHub Issue**: [#144](https://github.com/mandubian/ccos/issues/144)

#### Function Literals in Collection Operations
**Status**: Partially supported
**Syntax**: `(some (fn [x] (= x :b)) [:a :b :c])`
**Description**: Anonymous functions in higher-order operations
**Use Case**: Inline predicates and transformations
**Priority**: High
**GitHub Issue**: [#145](https://github.com/mandubian/ccos/issues/145)

### 4. Lazy Evaluation

#### Delay/Force Mechanism
**Status**: Not implemented
**Syntax**: `(delay ...)`, `(force ...)`
**Description**: Lazy evaluation and memoization
**Use Case**: Performance optimization, infinite sequences
**Priority**: Low
**GitHub Issue**: [#146](https://github.com/mandubian/ccos/issues/146)

## Implementation Considerations

### Current Limitations

1. **Parser Complexity**: Some features require significant parser modifications
2. **Type System Maturity**: Advanced type features depend on complete type system implementation
3. **Runtime Performance**: Lazy evaluation may impact performance characteristics
4. **Language Design**: Some features may conflict with RTFS's functional purity goals

### Potential Implementation Approaches

#### Character Literals
- Extend literal parser to recognize backslash escapes
- Add character type to the type system
- Unicode support considerations

#### Symbol Quote Syntax
- Modify parser to distinguish between quoted and unquoted symbols
- Update symbol resolution logic
- Consider namespace implications

#### Destructuring in Lambdas
- Extend pattern matching to function parameters
- Update AST to support destructuring patterns
- Type inference for destructured parameters

#### Type Predicates
- Implement predicate function system
- Add runtime type checking
- Integration with existing type system

## Testing Status

All unsupported features have been commented out in the test suite:
- `tests/rtfs_files/features/literal_values.rtfs`
- `tests/rtfs_files/features/vector_operations.rtfs`
- `tests/rtfs_files/features/type_system.rtfs`
- `tests/rtfs_files/features/map_operations.rtfs`
- `tests/rtfs_files/features/def_defn_expressions.rtfs`

Tests can be re-enabled once features are implemented.

## Related Specifications

- [01-language-features.md](./01-language-features.md) - Core language features
- [05-native-type-system.md](./05-native-type-system.md) - Type system design
- [10-formal-language-specification.md](./10-formal-language-specification.md) - Formal grammar

## Future Considerations

### Version Planning
- **RTFS 2.1**: Character literals, symbol quotes, destructuring
- **RTFS 2.2**: Type predicates, vector comprehensions
- **RTFS 3.0**: Lazy evaluation, advanced type system features

### Compatibility
- Ensure backward compatibility with existing RTFS code
- Consider migration paths for breaking changes
- Maintain functional programming principles

---

*This document will be updated as features are implemented or new unsupported features are discovered.*
