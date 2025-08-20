# Defstruct Special Form

## Overview

The `defstruct` special form provides ergonomic syntax for defining record-like data structures in RTFS 2.0. It serves as syntactic sugar for creating structured data types with named fields and type annotations, addressing the verbose syntax currently required when using `deftype` with map refinements.

## Syntax

```
(defstruct <struct-name> <field-spec>*)

field-spec = <keyword> <type-expr>
```

Where:
- `<struct-name>` is a symbol that names the struct type
- `<field-spec>` defines a field with a keyword name and type expression
- `<type-expr>` can be any valid RTFS type expression

## Semantics

When evaluated, `defstruct` creates a constructor function in the current environment that:

1. **Validates field presence**: Ensures all declared fields are present in the input map
2. **Validates field types**: Uses the RTFS type validator to verify each field matches its declared type
3. **Returns the validated map**: If all validations pass, returns the input map unchanged
4. **Throws validation errors**: If any field is missing or has the wrong type, throws a descriptive error

## Examples

### Basic Usage

```clojure
(defstruct Person
  :name String
  :age Int
  :email String)

; Creates a constructor function named 'Person' that validates:
; - :name field exists and is a String
; - :age field exists and is an Int  
; - :email field exists and is a String
```

### Creating Struct Instances

```clojure
; Valid struct creation
(Person {:name "Alice" :age 30 :email "alice@example.com"})
; => {:name "Alice" :age 30 :email "alice@example.com"}

; Missing field - validation error
(Person {:name "Bob" :age 25})
; => RuntimeError: Required field email is missing

; Wrong type - validation error
(Person {:name "Charlie" :age "thirty" :email "charlie@example.com"})
; => RuntimeError: Field age failed type validation
```

### Empty Structs

```clojure
(defstruct EmptyStruct)

; Valid - accepts empty map
(EmptyStruct {})
; => {}
```

### Complex Field Types

```clojure
(defstruct GenerationContext
  :arbiter-version String
  :generation-timestamp Timestamp
  :input-context Any
  :reasoning-trace String)
```

## Equivalent `deftype` Form

The `defstruct` form:

```clojure
(defstruct GenerationContext
  :arbiter-version String
  :generation-timestamp Timestamp
  :input-context Any
  :reasoning-trace String)
```

Is semantically equivalent to the verbose `deftype` approach:

```clojure
(deftype GenerationContext 
  (Map Keyword Any) 
  (and (has-key? :arbiter-version)
       (has-key? :generation-timestamp)
       (has-key? :input-context)
       (has-key? :reasoning-trace)
       (= (type-of (:arbiter-version this)) String)
       (= (type-of (:generation-timestamp this)) Timestamp)
       (= (type-of (:input-context this)) Any)
       (= (type-of (:reasoning-trace this)) String)))
```

## Implementation Details

### Runtime Type Validation

The `defstruct` implementation leverages the existing RTFS type validation system:

- Uses `TypeValidator::validate_value()` for each field
- Supports all RTFS type expressions including:
  - Primitive types (`String`, `Int`, `Float`, `Bool`)
  - Complex types (`Vector`, `Map`, `Function`)
  - Refined types with predicates
  - Union and intersection types
  - Custom type aliases

### Constructor Function

The generated constructor function:

- Has arity 1 (accepts a single map argument)
- Is a `BuiltinFunctionWithContext` with access to the evaluator and type validator
- Performs field presence validation before type validation
- Returns descriptive error messages for validation failures

### Integration with RTFS Type System

- `defstruct` definitions create type constructors, not new primitive types
- The struct name is bound to the constructor function in the environment
- Validation occurs at runtime when the constructor is called
- Compatible with existing RTFS type inference and checking systems

## Benefits

1. **Improved Ergonomics**: Clean, concise syntax for a common use case
2. **Enhanced Readability**: Self-documenting struct definitions
3. **Language Consistency**: Aligns with patterns in other Lisp-like languages
4. **Type Safety**: Leverages existing RTFS type validation infrastructure
5. **Future-Ready**: Foundation for implementing full compile-time type checking

## Future Enhancements

The current implementation provides the foundation for:

- Compile-time type inference and validation
- Optional field support with default values
- Nested struct validation
- Struct inheritance or composition patterns
- Integration with RTFS capability type checking
- Performance optimizations for struct validation

## See Also

- [RTFS Native Type System](05-native-type-system.md)
- [Type Predicate Reference](05-native-type-system.md#type-predicates)
- [Map Type Definitions](05-native-type-system.md#map-types)