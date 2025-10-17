# RTFS Type Syntax Analysis: Capability Parameters

## Question

In the generated capability, are the parameter types defined correctly?

```rtfs
:parameters {:travel_dates "string" :duration "number" :budget "currency" :interests "list" :accommodation_style "string" :travel_companions "string"}
```

**Issue**: Using string literals like `"string"`, `"number"`, `"currency"`, `"list"` instead of proper RTFS type syntax.

## RTFS Type System Analysis

### What RTFS 2.0 Specs Say

From `03-core-syntax-data-types.md`:
```rtfs
;; Type-annotated function
(defn add {:type {:args [Integer Integer] :return Integer}}
  [a b]
  (+ a b))
```

**Key observation**: Types use **capitalized identifiers**: `Integer`, `String`, `Boolean`, etc. NOT string literals.

### Host Function Definition (Security Model)

From `16-security-model.md`:
```rtfs
;; Host function signature
(def-host-fn read-file
  {:capability :fs.read
   :parameters {:path String}
   :return String})
```

**Pattern**: `:parameters {:path String}` - Type is a **symbol/identifier**, not a string literal.

### Schema Definition (Streaming)

From `09-streaming-capabilities.md`:
```rtfs
;; Typed stream processing
(call :stream.validate-types input-stream
  {:schema {:type :map
            :required [:id :name]
            :properties {:id {:type :integer}
                        :name {:type :string}}}
```

**Pattern**: For schemas, uses **keyword types**: `:integer`, `:string` (lowercase keywords)

## Comparison: Different Type Declaration Styles

### ❌ INCORRECT - String Literals (Current)
```rtfs
:parameters {:travel_dates "string" :duration "number" :budget "currency"}
```

- Uses string literals `"string"`, `"number"`, etc.
- Not valid RTFS type syntax
- Won't compile/validate properly

### ✅ CORRECT - Capitalized Type Identifiers (RTFS 2.0)
```rtfs
:parameters {:travel_dates String :duration Number :budget String}
```

- Uses capitalized type names
- Matches RTFS type annotation style
- This is what `defn` and `def-host-fn` use

### 🔵 ALTERNATIVE - Lowercase Keyword Types (Schema/Meta)
```rtfs
:parameters {:travel_dates :string :duration :number :budget :string}
```

- Uses keyword types `:string`, `:number`, etc.
- More suitable for schema/metadata representations
- Seen in streaming schemas and type descriptions

## What Are Valid RTFS Types?

Based on the specs, these are the core types:

```rtfs
;; Core scalar types
String      ; Text values
Integer     ; Whole numbers
Float       ; Decimal numbers
Boolean     ; true/false
Symbol      ; Symbolic identifiers
Keyword     ; :keyword values
Nil         ; nil/null value

;; Collection types
Vector      ; [1 2 3]
List        ; '(1 2 3)
Map         ; {:key "value"}

;; Function types (in type annotations)
Function    ; (fn [x] x)

;; Special types
Any         ; Accept any type
```

## The Issue with "currency", "list", etc.

### Problem
- `"currency"` is a custom semantic type (not in RTFS core types)
- `"list"` should be `List` or `Vector` in RTFS
- `"number"` should be `Integer` or `Float`
- `"string"` should be `String`

### Why It Matters
1. **Validation**: Type checker expects recognized RTFS types
2. **Compilation**: Custom string types won't type-check
3. **Interop**: Host boundary expects standard RTFS types
4. **Capability Marketplace**: Type registry expects valid types

## Recommendation: Fix the Capability

### Current (Incorrect)
```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner for couples..."
  :parameters {:travel_dates "string" 
               :duration "number" 
               :budget "currency"    ; ❌ Not a RTFS type
               :interests "list"      ; ❌ Should be Vector or List
               :accommodation_style "string"
               :travel_companions "string"})
```

### Corrected (Option 1: Use RTFS Types)
```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner for couples..."
  :parameters {:travel_dates String 
               :duration Integer 
               :budget String      ; Represent as String, value is "5000" or "$5000"
               :interests Vector    ; Vector of interest strings
               :accommodation_style String
               :travel_companions String})
```

### Corrected (Option 2: Use Keyword Types - More Semantic)
```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner for couples..."
  :parameters {:travel_dates :string 
               :duration :number
               :budget :currency   ; ✅ Can use semantic keywords in schema
               :interests :list
               :accommodation_style :string
               :travel_companions :string})
```

## Which Option to Use?

### Use **Capitalized Types** (Option 1) when:
- Parameters are used in RTFS function bodies
- Types need compile-time checking
- Following RTFS core type system
- For internal RTFS execution

### Use **Keyword Types** (Option 2) when:
- Parameters describe schema/metadata
- For documentation and type hints
- Custom semantic types are needed
- Streaming or schema validation

## CCOS Capability Context

Looking at the capability in CCOS context, since it's:
- **Generated by LLM** (not compiled immediately)
- **Stored in marketplace** (metadata)
- **Reused across domains** (travel, research, projects)

**Better approach**: Use **keyword types** (Option 2) because:
1. More flexible for different domains
2. Semantic meaning preserved (`:currency`, `:duration`, `:list`)
3. Can be interpreted by LLM during synthesis
4. Marketplace can use for capability discovery

## Final Recommendation

Update the generated capability to use **keyword types**:

```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner for couples with museum, history and food interests"
  :parameters {:travel_dates :string 
               :duration :number 
               :budget :currency 
               :interests :list 
               :accommodation_style :string 
               :travel_companions :string}
  :implementation
    (do
      (let flights
        (call :travel.flights {:destination "Paris" :dates travel_dates :travelers travel_companions :budget budget}))
      ...))
```

This maintains:
- ✅ Semantic type information (`currency`, `list`)
- ✅ LLM compatibility (keywords are self-documenting)
- ✅ Marketplace compatibility (metadata types)
- ✅ RTFS execution compatibility (keywords can be resolved at runtime)

## Where to Make This Change

In `parse_preferences_via_llm()` or `synthesize_capability_via_llm()`, when generating the capability, convert parameter types to keywords:

```rust
// In capability generation
for (param_name, param) in &prefs.parameters {
    // Convert to keyword type format
    let type_keyword = format!(":{}", param.param_type);  // ":string" ":currency" etc
    // Use in capability definition
    println!(":{} {}", param_name, type_keyword);
}
```

This ensures the synthesized capability uses proper RTFS type syntax that's compatible with the type system while preserving semantic information.



