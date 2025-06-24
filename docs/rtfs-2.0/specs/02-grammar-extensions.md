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
1. **Object Type Definitions**: 5 new top-level forms
2. **Versioned Namespacing**: Add version component to existing namespaces
3. **Object Schema Validation**: Extend existing validation
4. **Program Structure**: Support multiple object definitions

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

### 2. Object Type Definitions

**Pattern** (based on existing `task_definition`):
```pest
// Top-level program now supports all object types
program = { SOI ~ (object_definition | task_definition | module_definition | expression)* ~ EOI }

object_definition = _{ intent_definition | plan_definition | action_definition | capability_definition | resource_definition }

// Intent Object Definition
intent_definition = { "(" ~ intent_keyword ~ intent_property+ ~ ")" }
intent_keyword = @{ "intent" }
intent_property = {
    (":type" ~ versioned_type)
  | (":intent-id" ~ (uuid | string))
  | (":goal" ~ string)
  | (":created-at" ~ timestamp)
  | (":created-by" ~ string)
  | (":priority" ~ keyword)
  | (":constraints" ~ map)
  | (":success-criteria" ~ expression)
  | (":parent-intent" ~ (uuid | string))
  | (":child-intents" ~ vector)
  | (":status" ~ keyword)
  | (":metadata" ~ map)
}

// Plan Object Definition  
plan_definition = { "(" ~ plan_keyword ~ plan_property+ ~ ")" }
plan_keyword = @{ "plan" }
plan_property = {
    (":type" ~ versioned_type)
  | (":plan-id" ~ (uuid | string))  
  | (":created-at" ~ timestamp)
  | (":created-by" ~ keyword)
  | (":intent-ids" ~ vector)
  | (":strategy" ~ keyword)
  | (":estimated-cost" ~ float)
  | (":estimated-duration" ~ integer)
  | (":program" ~ map)
  | (":status" ~ keyword)
  | (":execution-context" ~ map)
}

// Action Object Definition
action_definition = { "(" ~ action_keyword ~ action_property+ ~ ")" }
action_keyword = @{ "action" }
action_property = {
    (":type" ~ versioned_type)
  | (":action-id" ~ (uuid | string))
  | (":timestamp" ~ timestamp)
  | (":plan-id" ~ (uuid | string))
  | (":step-id" ~ string)
  | (":intent-id" ~ (uuid | string))
  | (":capability-used" ~ versioned_type)
  | (":executor" ~ map)
  | (":input" ~ map)
  | (":output" ~ map)
  | (":execution" ~ map)
  | (":signature" ~ map)
}

// Capability Object Definition
capability_definition = { "(" ~ capability_keyword ~ capability_property+ ~ ")" }
capability_keyword = @{ "capability" }
capability_property = {
    (":type" ~ versioned_type)
  | (":capability-id" ~ versioned_type)
  | (":created-at" ~ timestamp)
  | (":provider" ~ map)
  | (":function" ~ map)
  | (":sla" ~ map)
  | (":technical" ~ map)
  | (":status" ~ keyword)
  | (":marketplace" ~ map)
}

// Resource Object Definition
resource_definition = { "(" ~ resource_keyword ~ resource_property+ ~ ")" }
resource_keyword = @{ "resource" }
resource_property = {
    (":type" ~ versioned_type)
  | (":resource-id" ~ (uuid | string))
  | (":handle" ~ resource_handle)
  | (":created-at" ~ timestamp)
  | (":created-by" ~ (uuid | string))
  | (":content" ~ map)
  | (":storage" ~ map)
  | (":lifecycle" ~ map)
  | (":metadata" ~ map)
  | (":access" ~ map)
}
```

### 3. Resource Reference Syntax

Add special syntax for resource references:
```pest
// Resource reference: (resource:ref "step-1.output")  
resource_ref = { "(" ~ resource_ref_keyword ~ string ~ ")" }
resource_ref_keyword = @{ "resource:ref" }

// Resource handle: resource://path/to/data
resource_handle = @{ "resource://" ~ (!WHITESPACE ~ ANY)+ }

// Update expression to include these
expression = _{ 
    literal | keyword | symbol | task_context_access | 
    resource_ref |
    special_form | list | vector | map 
}

### 4. Enhanced Literal Types

Support for additional literal types needed by RTFS 2.0:
```pest
// ISO 8601 timestamp literal
timestamp = @{ ASCII_DIGIT{4} ~ "-" ~ ASCII_DIGIT{2} ~ "-" ~ ASCII_DIGIT{2} ~ 
               "T" ~ ASCII_DIGIT{2} ~ ":" ~ ASCII_DIGIT{2} ~ ":" ~ ASCII_DIGIT{2} ~ 
               ("." ~ ASCII_DIGIT{3})? ~ "Z" }

// UUID literal  
uuid = @{ ASCII_HEX_DIGIT{8} ~ "-" ~ ASCII_HEX_DIGIT{4} ~ "-" ~ ASCII_HEX_DIGIT{4} ~ 
          "-" ~ ASCII_HEX_DIGIT{4} ~ "-" ~ ASCII_HEX_DIGIT{12} }

// Update literal rule
literal = { 
    timestamp | uuid | resource_handle | float | integer | string | boolean | nil | keyword 
}

## Backward Compatibility Note

Since we're not maintaining RTFS 1.0 compatibility, we can:
- **Simplify**: Remove unused grammar rules from RTFS 1.0
- **Optimize**: Restructure rules for better performance  
- **Clean**: Remove legacy workarounds and technical debt
- **Focus**: Design purely for RTFS 2.0 use cases

## Example RTFS 2.0 Files

### Simple Intent Definition
```rtfs
(intent
  :type :rtfs.core:v2.0:intent
  :intent-id "intent-12345"
  :goal "Analyze quarterly sales data"
  :priority :high
  :constraints {
    :max-cost 50.0
    :deadline "2025-06-25T17:00:00Z"
  }
  :success-criteria (fn [result] (> (:confidence result) 0.9))
  :status :active)
```

### Simple Plan Definition  
```rtfs
(plan
  :type :rtfs.core:v2.0:plan
  :plan-id "plan-67890"
  :intent-ids ["intent-12345"]
  :strategy :parallel-analysis
  :program {
    :steps [
      {
        :step-id "step-1"
        :action :fetch-data
        :capability :com.acme.db:v1.0:sales-query
        :params {:query "SELECT * FROM sales"}
      }
    ]
  }
  :status :ready)
```

### Resource Reference Usage
```rtfs
(plan
  :type :rtfs.core:v2.0:plan
  :plan-id "plan-abc"
  :program {
    :steps [
      {
        :step-id "step-2"
        :depends-on ["step-1"]
        :params {:data (resource:ref "step-1.output")}
      }
    ]
  })
```

## Implementation Files to Modify

1. **`rtfs_compiler/src/rtfs.pest`** - Grammar extensions
2. **`rtfs_compiler/src/ast.rs`** - AST structure updates  
3. **`rtfs_compiler/src/parser.rs`** - Parser logic updates
4. **`rtfs_compiler/src/repl.rs`** - REPL command extensions
5. **`rtfs_compiler/tests/`** - Test cases for new syntax

## Testing Strategy

### Grammar Tests  
- Parse each object type independently
- Test versioned namespacing edge cases
- Validate error handling for malformed objects

### Integration Tests
- Mixed RTFS 2.0 files with multiple object types
- Resource reference resolution  
- Object validation with schema checking

### REPL Tests
- Interactive object creation and inspection
- Pretty printing and formatting
- Autocomplete and help systems
