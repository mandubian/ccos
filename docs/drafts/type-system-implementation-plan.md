# RTFS Type System: Implementation Plan

## 📋 Current Status Analysis

**Date**: 2026-01-06
**Current Implementation**: Runtime validation only (`type_validator.rs`)
**Formal Specification**: Complete type theory with proofs (`docs/rtfs-2.0/specs/13-type-system.md`)
**Gap**: Major - missing formal subtyping, type inference, intersection types, compile-time checking

## 📊 Current vs. Formal Specification Gap Analysis

| Feature | Formal Spec | Current Implementation | Gap Size |
|---------|-------------|----------------------|----------|
| **Subtyping System** | 12 axioms + proofs | ✅ Complete implementation (IR) | ✅ **Done** |
| **Type Inference** | Bidirectional algorithm | ✅ Basic inference + type_meet/join | ✅ **Done** |
| **Intersection Types** | Full implementation | ✅ Full IR implementation + docs | ✅ **Done** |
| **Compile-Time Checking** | Parse-time validation | ⚠️ Partial (IR type checking) | **Moderate** |
| **Union Types** | Full with subtyping | ✅ Complete implementation | ✅ **Done** |
| **Refinement Types** | Full predicate logic | ✅ 22 predicates working | ✅ **Done** |
| **Collection Types** | Full with subtyping | ✅ Complete implementation | ✅ **Done** |

## 🎯 Priority Implementation Roadmap

### **Phase 1: Core Subtyping & Inference (Highest Priority)** ✅ **COMPLETED**

#### 1.1 Implement Subtyping Relation (12 Axioms) ✅ **DONE**
**Goal**: Replace basic numeric coercion with formal subtyping system
**Completed**:
- ✅ Reflexivity, transitivity, top/bottom rules (S-Refl, S-Trans, S-Top, S-Bot)
- ✅ Union type subtyping rules (S-Union-L, S-Union-R)
- ✅ Function subtyping with contravariance (S-Fun)
- ✅ Collection subtyping (S-Vector, S-Map, S-Tuple)
- ✅ **Intersection type subtyping** (S-Intersection-L, S-Intersection-R)

**Files modified**:
- ✅ `rtfs/src/ir/type_checker.rs` → Complete subtyping implementation
- ✅ Enhanced union-intersection interaction logic
- ✅ Fixed failing intersection type tests

#### 1.2 Add Type Environment & Context ✅ **PARTIAL**
**Goal**: Create type context for inference and checking
**Completed**:
- ✅ Basic type environment in IR type checker
- ✅ Type context for inference operations
- ⚠️ Type variable scoping (needs generics implementation)

**Files created/modified**:
- ✅ Enhanced `rtfs/src/ir/type_checker.rs` with type context support
- ✅ Type inference functions with context awareness

#### 1.3 Implement Bidirectional Type Checking ✅ **COMPLETED**
**Goal**: Add synthesis/checking judgments
**Completed**:
- ✅ Type synthesis: `Γ ⊢ e ⇒ τ` via `infer_type()`
- ✅ Type checking: `Γ ⊢ e ⇐ τ` via `type_check_ir()`
- ✅ Inference rules for core expression types
- ✅ Bidirectional checking with subtyping integration

**Files created/modified**:
- ✅ `rtfs/src/ir/type_checker.rs` → Complete bidirectional checking
- ✅ `infer_type()` function for type synthesis
- ✅ `type_check_ir()` function for type verification

### **Phase 2: Advanced Types & Features**

#### 2.1 Implement Intersection Types ✅ **COMPLETED**
**Goal**: Real validation for `TypeExpr::Intersection`
**Completed**:
- ✅ Intersection validation logic in IR type checker
- ✅ Meet/join operations (`type_meet`, `type_join`)
- ✅ `[:and TypeA TypeB]` syntax support in parser
- ✅ Complete subtyping rules (S-Intersection-L, S-Intersection-R)
- ✅ Intersection simplification (flattening, Any-removal, de-dup, Never-shortcut)
- ✅ Comprehensive documentation and examples

**Files modified**:
- ✅ `rtfs/src/ir/type_checker.rs` → Complete intersection implementation
- ✅ `rtfs/src/parser/types.rs` → Intersection syntax parsing
- ✅ `rtfs/src/runtime/type_validator.rs` → Runtime validation
- ✅ Enhanced documentation with examples and use cases

#### 2.2 Add Generic Type Variables
**Goal**: Support parametric polymorphism
**Missing**:
- Type variables (α, β, γ...)
- Type variable unification algorithm
- Generic type constraints

**Files to create**:
- `rtfs/src/type_checking/unification.rs` → Unification algorithm
- `rtfs/src/type_checking/variables.rs` → Type variable management
- `rtfs/src/type_checking/generics.rs` → Generic type support

#### 2.3 Implement Type Classes/Traits
**Goal**: Add ad-hoc polymorphism
**Missing**:
- Type class definitions
- Instance declarations
- Constraint solving

**Files to create**:
- `rtfs/src/type_checking/classes.rs` → Type class system
- `rtfs/src/type_checking/constraints.rs` → Constraint solving
- `rtfs/src/type_checking/instances.rs` → Instance management

### **Phase 3: Compile-Time Integration** ✅ **PARTIALLY COMPLETE**

#### 3.1 Integrate with Parser ⚠️ **CLARIFIED**
**Status**: Type checking happens at IR level, not parse level
**Current Architecture**:
- ✅ Parser creates AST with type annotations
- ✅ AST → IR conversion preserves type information
- ✅ IR type checker validates types at compile time
- ✅ This is the correct design (separation of concerns)

**Why this is good**:
- Parser handles syntax, not semantics
- IR type checker handles validation after semantic analysis
- Clean separation between parsing and type checking

#### 3.2 Add Type Annotations to Grammar ✅ **ALREADY IMPLEMENTED**
**Status**: Type annotations are already fully supported in the grammar
**Completed**:
- ✅ Function parameter type annotations: `(fn [x: Int y: Float] :Number (+ x y))`
- ✅ Let-binding type annotations: `(let [x: Int 42] x)`
- ✅ Return type declarations: `(fn [] :String "hello")`
- ✅ Complex type expressions: unions, intersections, collections

**Files already implemented**:
- ✅ `rtfs/src/rtfs.pest` → Complete type annotation grammar
- ✅ `rtfs/src/parser/types.rs` → Full type annotation parsing
- ✅ `rtfs/src/ast.rs` → TypeExpr with all type constructs

#### 3.3 Implement Type-Directed Optimizations ⚠️ **LOWER PRIORITY**
**Status**: Current IR type checker provides solid foundation
**Current State**:
- ✅ IR type checker already implemented and working
- ✅ Used in compiler pipeline for validation
- ✅ Provides type safety guarantees

**Future Enhancements** (when needed):
- Type-based function specialization
- Type-directed inlining
- Type-based dead code elimination

**Files that exist**:
- ✅ `rtfs/src/ir/type_checker.rs` → Complete IR type checking
- ✅ Integrated in `rtfs/src/bin/rtfs_compiler.rs`

### **Phase 4: Formal Verification & Testing**

#### 4.1 Prove Soundness Theorems
**Goal**: Formal type safety guarantees
**Missing**:
- Progress theorem proof
- Preservation theorem proof
- Type system metatheory

**Files to create**:
- `docs/proofs/progress-theorem.md` → Progress proof
- `docs/proofs/preservation-theorem.md` → Preservation proof
- `docs/proofs/type-safety.md` → Complete type safety proof

#### 4.2 Add Comprehensive Testing
**Goal**: Ensure correctness of implementation
**Missing**:
- Subtyping relation tests
- Type inference tests
- Edge case validation

**Files to create**:
- `tests/type_checking/subtyping_tests.rs` → Subtyping tests
- `tests/type_checking/inference_tests.rs` → Inference tests
- `tests/type_checking/integration_tests.rs` → End-to-end tests

#### 4.3 Error Reporting & Diagnostics
**Goal**: Better developer experience
**Missing**:
- Type error location tracking
- Error suggestions and explanations
- Type visualization tools

**Files to create**:
- `rtfs/src/diagnostics/type_errors.rs` → Error reporting
- `rtfs/src/diagnostics/suggestions.rs` → Error suggestions
- `rtfs/src/diagnostics/visualization.rs` → Type visualization

## 🔧 Implementation Details

### Current Architecture
```
rtfs/src/
├── ast.rs                    # TypeExpr enum, TypePredicate enum
├── runtime/
│   └── type_validator.rs     # Runtime validation only
└── parser/                   # No type checking integration
```

### Target Architecture
```
rtfs/src/
├── ast.rs                    # Type expressions extended
├── type_checking/
│   ├── subtyping.rs          # 12 subtyping axioms
│   ├── synthesis.rs          # Type inference (Γ ⊢ e ⇒ τ)
│   ├── checking.rs           # Type verification (Γ ⊢ e ⇐ τ)
│   ├── context.rs            # Type environment (Γ)
│   ├── unification.rs        # Type variable unification
│   ├── generics.rs           # Generic type support
│   ├── classes.rs            # Type classes/traits
│   └── constraints.rs        # Constraint solving
├── compiler/
│   ├── type_checking.rs      # Compile-time type checking
│   └── optimizations/
│       └── type_based.rs     # Type-driven optimizations
├── diagnostics/
│   ├── type_errors.rs        # Error reporting
│   ├── suggestions.rs        # Error suggestions
│   └── visualization.rs      # Type visualization
└── parser/
    └── type_annotations.rs   # Parse-time type checking
```

## 💡 Key Design Decisions Needed

### Decision 1: Compile-time vs Runtime Type Checking
**Option A**: Hybrid approach (current + compile-time)
- Keep runtime validation for dynamic code
- Add compile-time checking for annotated code
- **Pros**: Backward compatible, gradual adoption
- **Cons**: Dual implementation, potential inconsistency

**Option B**: Full compile-time checking
- Move all type checking to compile time
- Remove runtime `TypeValidator`
- **Pros**: Single implementation, better performance
- **Cons**: Breaking change, requires all code to be type-checkable

**Recommended**: **Option A** - Hybrid approach for gradual migration

### Decision 2: Formal Subtyping Implementation
**Option A**: Complete 12 axioms
- Implement full formal subtyping system
- Include all union/intersection rules
- **Pros**: Matches specification, complete correctness
- **Cons**: Complex implementation, potential performance impact

**Option B**: Pragmatic subset
- Implement essential subtyping rules only
- Focus on common cases (Int→Float, collections)
- **Pros**: Simpler, faster implementation
- **Cons**: Incomplete, may limit advanced type features

**Recommended**: **Option A** - Complete implementation for long-term value

### Decision 3: Type Inference Strategy
**Option A**: Complete inference (Hindley-Milner)
- Full HM type inference with let-generalization
- **Pros**: Powerful, minimal annotations needed
- **Cons**: Complex implementation, potential inference ambiguities

**Option B**: Local inference only
- Infer types within expressions but not across let-bindings
- Require annotations for function parameters
- **Pros**: Simpler, predictable behavior
- **Cons**: More annotations required

**Recommended**: **Option B** - Local inference for RTFS use cases (LLM-generated code often has explicit types)

## 🎯 Parametric Map Design (New Section)

### Current State: Structural Typing
```clojure
; Structural maps (explicit keys)
[:map [:name :string] [:age :integer]]
[:map {:name :string :age :integer}]
```

**Strengths**:
- ✅ Explicit contracts, self-documenting
- ✅ Flexible for scripting and ad-hoc data
- ✅ Handles both keyword and string keys
- ✅ Runtime validation works well

### Proposed: Hybrid Approach
```clojure
; Structural maps (keep as default)
[:map [:name :string] [:age :integer]]

; Parametric maps (new, string-keyed dictionaries)
[:map-of :string :any]    ; Concrete: Map<String, Any>
[:map-of :keyword :any]   ; Concrete: Map<Keyword, Any>
[:map-of K V]             ; Generic: Map<K, V> where K ≤ (String | Keyword)
```

**Design Decisions**:
- ✅ **Syntax**: `[:map-of K V]` (clear and distinct)
- ✅ **Key Type**: String or Keyword (or union) (matches RTFS MapKey and JSON boundary use-cases)
- ✅ **Constraints**: Equality + upper bounds only (tractable)
- ✅ **Inference**: Explicit arguments + local inference (conservative)
- ✅ **Keyword Support**: First-class (`[:map-of :keyword V]` supported)

**Rationale**:
- Keeps structural typing for scripting (excellent for ad-hoc data)
- Adds parametric polymorphism for advanced use cases
- Avoids complexity (no complex constraint solving)
- Matches RTFS philosophy (deterministic, good errors)

### Implementation Plan

**Phase 1: Foundation**
```rust
// Add type variables to IrType
enum IrType {
    TypeVar(String),
    ParametricMap {
        key_type: Box<IrType>,   // Must be ≤ String
        value_type: Box<IrType>,
    },
    // ... existing variants
}
```

**Phase 2: Unification**
```rust
// Simple unification algorithm
fn unify(t1: &IrType, t2: &IrType) -> Result<Substitution> {
    match (t1, t2) {
        (TypeVar(_), _) => /* bind variable */,
        (_, TypeVar(_)) => /* bind variable */,
        // Simple cases only
    }
}
```

**Phase 3: Parser Integration**
```rust
// Add to grammar
map_of_type = { "[" ~ ":map-of" ~ WHITESPACE* ~ type_expr ~ WHITESPACE* ~ type_expr ~ WHITESPACE* ~ "]" }
```

**Phase 4: Type Checking**
```rust
// Add to subtyping
match (sub, sup) {
    (ParametricMap(k1, v1), ParametricMap(k2, v2)) => {
        // Check k1 ≤ String and k2 ≤ String
        // Check k1 ≤ k2 (contravariant for keys?)
        // Check v1 ≤ v2 (covariant for values)
    }
}
```

## 📅 Estimated Timeline

### Phase 1: Core Subtyping & Inference
- **Weeks 1-2**: Implement subtyping relation (12 axioms)
- **Weeks 3-4**: Add type environment and context
- **Weeks 5-6**: Implement bidirectional checking
- **Week 7**: Testing and bug fixes

### Phase 2: Advanced Types
- **Weeks 8-9**: Implement intersection types
- **Weeks 10-11**: Add generic type variables
- **Weeks 12-13**: Implement type classes
- **Week 14**: Integration testing

### Phase 3: Compile-Time Integration
- **Weeks 15-16**: Integrate with parser
- **Weeks 17-18**: Add type annotations to grammar
- **Weeks 19-20**: Implement type-directed optimizations
- **Week 21**: Performance testing

### Phase 4: Formal Verification
- **Weeks 22-23**: Prove soundness theorems
- **Weeks 24-25**: Add comprehensive testing
- **Weeks 26-27**: Error reporting and diagnostics
- **Week 28**: Documentation and final polish

**Total**: ~7 months for complete implementation

## 🚀 Quick Wins (First 4 Weeks)

1. **Week 1**: Implement basic subtyping (Refl, Trans, Top, Bot)
2. **Week 2**: Add union subtyping rules (S-Union-L, S-Union-R)
3. **Week 3**: Implement function subtyping (S-Fun)
4. **Week 4**: Add collection subtyping (S-Vector, S-Map, S-Tuple)

These would immediately improve type checking for common cases while building toward the full system.

## 🔗 Related Files & Dependencies

### Core Implementation Files
- `rtfs/src/runtime/type_validator.rs` (1140 lines) → Extend with subtyping
- `rtfs/src/ast.rs` (lines 194-260) → TypeExpr and TypePredicate enums
- `rtfs/src/parser/` → Grammar integration

### Dependencies to Add
- Possibly a unification library for type variables
- Graph library for constraint solving
- Testing framework for formal proofs

### Migration Path
1. Extend `TypeValidator` with subtyping methods
2. Create new type checking modules alongside runtime validation
3. Gradually migrate validation to compile time
4. Eventually deprecate runtime-only validation for type-annotated code

## 📝 Success Criteria

### Phase 1 Complete When: ⚠️ **PARTIALLY COMPLETE**
- [x] Core IR subtyping rules implemented and tested (union, intersection, functions, collections)
- [ ] Type environment (Γ) with proper scoping (needed earlier than IR, for real inference)
- [x] IR-level checking for core expressions (application, let-annotations, structural traversal)
- [x] No regression in existing runtime validation

### Phase 2 Complete When: ⚠️ **PARTIALLY COMPLETE**
- [x] Intersection types fully functional
- [ ] Generic type variables with unification
- [ ] Type classes with constraint solving
- [ ] All type features from formal specification implemented

### Phase 3 Complete When:
- [ ] Compile-time type checking integrated with parser
- [ ] Type annotations supported in grammar
- [ ] Type-directed optimizations providing measurable performance gains
- [ ] Backward compatibility maintained

### Phase 4 Complete When:
- [ ] Progress and preservation theorems formally documented
- [ ] Comprehensive test suite with 95%+ coverage
- [ ] Error reporting with helpful diagnostics
- [ ] Complete documentation for new type system

## 🎯 Final Goal

A **production-ready type system** that:
1. **Matches the formal specification** in capabilities
2. **Provides compile-time safety** for RTFS code
3. **Enables advanced type features** for LLM-generated code
4. **Maintains backward compatibility** with existing runtime validation
5. **Delivers practical value** through better error messages and optimizations

---

**Last Updated**: 2024-02-20
**Status**: ✅ **Phase 1 & 2 Partially Complete**
**Completed**:
- ✅ All 12 subtyping axioms implemented and tested
- ✅ Complete intersection type implementation with documentation
- ✅ Bidirectional type checking (synthesis + checking)
- ✅ Type meet/join operations for intersection types
- ✅ Fixed all failing intersection type tests
- ✅ Comprehensive documentation and examples

**Next Step**: Begin Phase 2 implementation (generic type variables)

**Key Clarifications After Analysis**:
- ✅ Type annotations already fully supported in grammar (not missing)
- ✅ IR type checker already exists and works (not missing)
- ✅ Compile-time integration already working via IR checking (not missing)
- ⚠️ Real gaps: Generic type variables, type classes, parametric polymorphism

**Revised Understanding**:
- Current system is stronger than initially assessed
- Foundation is solid (IR type checker, grammar support)
- Focus on real gaps: parametric polymorphism and type abstraction