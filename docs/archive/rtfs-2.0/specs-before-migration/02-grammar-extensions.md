# RTFS 2.0 Grammar Extensions

**Date:** June 23, 2025  
**Version:** 0.1.0-draft  
**Status:** Draft

## Overview

This document specifies the grammar extensions needed to support RTFS 2.0 objects while leveraging the existing `rtfs.pest` infrastructure. The current grammar already has sophisticated features we can build upon:

- ✅ Namespaced identifiers (`my.module/function`)
- ✅ Complex type expressions
- ✅ Task definitions with structured properties  
- ✅ Keywords, maps, vectors
- ✅ Comprehensive special forms

## Analysis of Existing Grammar

### What We Can Reuse:
1. **`task_definition` Pattern**: Perfect template for our 5 new object types
2. **`namespaced_identifier`**: Foundation for versioned namespacing
3. **Type System**: Rich type expressions already implemented
4. **Keywords & Maps**: Core data structures ready
5. **Parsing Infrastructure**: Pest parser, AST, validation framework

### What We Need to Extend:
1. **Versioned Namespacing**: Add version component to existing namespaces
2. **Enhanced Literal Types**: Support UUID, timestamp, and resource handle literals
3. **Resource Reference Syntax**: Special syntax for referencing resources

Note: CCOS objects (Intent, Plan, Action, etc.) are **not** special language constructs. They are parsed as generic S-expressions and interpreted by the CCOS runtime layer.

## Proposed Grammar Extensions

### 1. Versioned Namespacing Extension

**Current**: `namespaced_identifier = @{ identifier ~ ("." ~ identifier)* ~ "/" ~ identifier }`

**Extended**:
```pest
// Version-aware namespaced identifier: com.acme:v1.2/function
versioned_namespace = @{ identifier ~ ("." ~ identifier)* ~ ":" ~ version ~ "/" ~ identifier }
version = @{ "v" ~ ASCII_DIGIT+ ~ ("." ~ ASCII_DIGIT+)* }

// Type identifier with full versioning: :com.acme:v1.2:intent
versioned_type = @{ ":" ~ identifier ~ ("." ~ identifier)* ~ ":" ~ version ~ ":" ~ identifier }

// Update existing rules to support versioning
symbol = { versioned_namespace | namespaced_identifier | identifier }
keyword = @{ ":" ~ (versioned_namespace | namespaced_identifier | identifier) }
```

### 2. Enhanced Literal Types

Support for additional literal types needed by RTFS 2.0:
```pest
// ISO 8601 timestamp literal
timestamp = @{ ASCII_DIGIT{4} ~ "-" ~ ASCII_DIGIT{2} ~ "-" ~ ASCII_DIGIT{2} ~ 
               "T" ~ ASCII_DIGIT{2} ~ ":" ~ ASCII_DIGIT{2} ~ ":" ~ ASCII_DIGIT{2} ~ 
               ("." ~ ASCII_DIGIT{3})? ~ "Z" }

// UUID literal  
uuid = @{ ASCII_HEX_DIGIT{8} ~ "-" ~ ASCII_HEX_DIGIT{4} ~ "-" ~ ASCII_HEX_DIGIT{4} ~ 
          "-" ~ ASCII_HEX_DIGIT{4} ~ "-" ~ ASCII_HEX_DIGIT{12} }

// Resource handle: resource://path/to/data
resource_handle = @{ "resource://" ~ (!WHITESPACE ~ ANY)+ }

// Update literal rule
literal = { 
    timestamp | uuid | resource_handle | float | integer | string | boolean | nil | keyword 
}
```

### 3. Resource Reference Syntax

Add special syntax for resource references:
```pest
// Resource reference: (resource:ref "step-1.output")  
resource_ref = { "(" ~ resource_ref_keyword ~ string ~ ")" }
resource_ref_keyword = @{ "resource:ref" }

// Update expression to include these
expression = _{ 
    literal | keyword | symbol | task_context_access | 
    resource_ref |
    special_form | list | vector | map 
}
```

## Backward Compatibility Note

Since we're not maintaining RTFS 1.0 compatibility, we can:
- **Simplify**: Remove unused grammar rules from RTFS 1.0
- **Optimize**: Restructure rules for better performance  
- **Clean**: Remove legacy workarounds and technical debt
- **Focus**: Design purely for RTFS 2.0 use cases

## Example RTFS 2.0 Files

Note: CCOS objects like `intent`, `plan`, etc. are parsed as regular S-expressions. The CCOS runtime interprets these based on the first symbol in the list.

### Resource Reference Usage
```rtfs
;; This is parsed as a regular function call
;; The CCOS runtime recognizes and handles it specially
(plan
  :type :rtfs.core:v2.0:plan
  :plan-id "plan-abc"
  :program (do
    (let [data (resource:ref "step-1.output")]
      (analyze data))))
```

### Versioned Namespacing
```rtfs
;; Using versioned capabilities
(call :com.acme.db:v1.2:sales-query {:format :csv})

;; Versioned type references
:com.openai:v1.0:gpt-capability
```

## Implementation Files to Modify

1. **`rtfs_compiler/src/rtfs.pest`** - Grammar extensions
2. **`rtfs_compiler/src/ast.rs`** - AST structure updates  
3. **`rtfs_compiler/src/parser.rs`** - Parser logic updates
4. **`rtfs_compiler/src/repl.rs`** - REPL command extensions
5. **`rtfs_compiler/tests/`** - Test cases for new syntax

## Testing Strategy

### Grammar Tests  
- Test versioned namespacing edge cases
- Validate UUID and timestamp literal parsing
- Test resource reference syntax
- Validate error handling for malformed literals

### Integration Tests
- Mixed RTFS 2.0 files with versioned namespacing
- Resource reference resolution in expressions
- Compatibility with existing RTFS syntax

### REPL Tests
- Interactive testing of new literal types
- Pretty printing of versioned identifiers
- Autocomplete for namespaced symbols
