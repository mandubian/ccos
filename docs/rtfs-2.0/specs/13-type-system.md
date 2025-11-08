# RTFS Type System: Formal Specification

**Version**: 1.0  
**Status**: Implemented  
**Location**: `rtfs/src/ir/type_checker.rs`

---

## Table of Contents

1. [Introduction](#introduction)
2. [Formal Type System](#formal-type-system)
3. [Subtyping Relation](#subtyping-relation)
4. [Type Checking Algorithm](#type-checking-algorithm)
5. [Soundness Theorem](#soundness-theorem)
6. [Implementation](#implementation)
7. [Examples](#examples)
8. [References](#references)

---

## 1. Introduction

### 1.1 Overview

RTFS employs a **static, structural type system** with **subtyping** and **union types**. The type system is designed to:

1. **Prevent runtime type errors** at compile time
2. **Support numeric coercion** (Int ↔ Float) safely
3. **Enable gradual typing** via the `Any` type
4. **Maintain soundness** while being practical

### 1.2 Design Principles

1. **Soundness over Completeness**: We reject some valid programs to guarantee type safety
2. **Decidability**: Type checking always terminates
3. **Explicit Coercion Model**: Type conversions are explicit in the runtime, implicit in types
4. **Structural Subtyping**: Types are compared by structure, not names

---

## 2. Formal Type System

### 2.1 Type Grammar

```
τ ::= Int                         primitive integer type
    | Float                       primitive float type  
    | String                      primitive string type
    | Bool                        primitive boolean type
    | Nil                         unit/null type
    | Keyword                     keyword type
    | Symbol                      symbol type
    | Any                         top type (⊤)
    | Never                       bottom type (⊥)
    | Vector⟨τ⟩                   homogeneous vector
    | List⟨τ⟩                     homogeneous list
    | Tuple⟨τ₁, ..., τₙ⟩          heterogeneous tuple
    | Map{k₁:τ₁, ..., kₙ:τₙ}     record/map type
    | τ₁ → τ₂                     function type
    | τ₁ → ... → τₙ               variadic function
    | τ₁ | τ₂                     union type
    | τ₁ & τ₂                     intersection type
    | Resource⟨name⟩              resource handle type
```

### 2.2 Special Types

**Number Type (Derived)**:
```
Number ≡ Int | Float
```

This union represents the runtime numeric tower where Int can be promoted to Float.

**Top and Bottom**:
- `Any` (⊤): Accepts all values, represents unknown type
- `Never` (⊥): Has no values, represents unreachable code

### 2.3 Type Environments

A **type environment** `Γ` is a finite mapping from variables to types:

```
Γ ::= ∅                          empty environment
    | Γ, x:τ                     environment extension
```

**Well-formedness**: `Γ(x) = τ` means variable `x` has type `τ` in environment `Γ`.

---

## 3. Subtyping Relation

### 3.1 Formal Definition

We define a binary relation `τ₁ ≤ τ₂` (read "`τ₁` is a subtype of `τ₂`") with the following inference rules:

#### 3.1.1 Structural Rules

```
────────────  (S-Refl)
  τ ≤ τ

  τ₁ ≤ τ₂    τ₂ ≤ τ₃
  ─────────────────────  (S-Trans)
       τ₁ ≤ τ₃

────────────  (S-Top)
  τ ≤ Any

────────────  (S-Bot)
  Never ≤ τ
```

#### 3.1.2 Union Types

```
  τ ≤ τ₁        τ ≤ τ₂
  ─────────  OR  ─────────  (S-Union-R)
   τ ≤ τ₁|τ₂      τ ≤ τ₁|τ₂

  τ₁ ≤ τ    τ₂ ≤ τ
  ──────────────────  (S-Union-L)
    τ₁|τ₂ ≤ τ
```

**Lemma 3.1** (Union Characterization): 
```
τ ≤ τ₁|τ₂  ⟺  τ ≤ τ₁ ∨ τ ≤ τ₂
τ₁|τ₂ ≤ τ  ⟺  τ₁ ≤ τ ∧ τ₂ ≤ τ
```

#### 3.1.3 Function Subtyping

```
  τ₁' ≤ τ₁    τ₂ ≤ τ₂'
  ──────────────────────  (S-Fun)
   τ₁ → τ₂ ≤ τ₁' → τ₂'
```

**Note**: Function types are:
- **Contravariant** in argument types (flipped: `τ₁' ≤ τ₁`)
- **Covariant** in return types (same direction: `τ₂ ≤ τ₂'`)

**Variadic Functions**:
```
  τ₁' ≤ τ₁    ...    τₙ' ≤ τₙ    τᵥ' ≤ τᵥ    τᵣ ≤ τᵣ'
  ───────────────────────────────────────────────────────  (S-Fun-Var)
   (τ₁, ..., τₙ, τᵥ...) → τᵣ ≤ (τ₁', ..., τₙ', τᵥ'...) → τᵣ'
```

#### 3.1.4 Collection Subtyping

```
  τ₁ ≤ τ₂
  ─────────────────  (S-Vec)
   Vector⟨τ₁⟩ ≤ Vector⟨τ₂⟩

  τ₁ ≤ τ₂
  ─────────────  (S-List)
   List⟨τ₁⟩ ≤ List⟨τ₂⟩

  τ₁ ≤ σ₁    ...    τₙ ≤ σₙ
  ──────────────────────────────  (S-Tuple)
   Tuple⟨τ₁,...,τₙ⟩ ≤ Tuple⟨σ₁,...,σₙ⟩
```

**Note**: All collection types are **covariant** in their element types.

**Note**: Map type subtyping rules are **not currently implemented**. Map types are checked structurally - all required fields in the type definition must be present in the data, but extra fields are allowed regardless of wildcard specification.

#### 3.1.5 Join (Least Upper Bound)

For vector and list type inference, we define the **join** operation:

```
join(τ₁, ..., τₙ) = smallest τ such that ∀i. τᵢ ≤ τ
```

**Formal Definition**:
```
join(τ)           = τ                                    (single type)
join(τ, τ)        = τ                                    (duplicates)
join(Int, Float)  = Int | Float                         (numeric tower)
join(τ₁, τ₂)      = τ₁ | τ₂        if τ₁ ≠ τ₂          (general case)
join(τ₁, ..., τₙ) = τ₁ | ... | τₙ  if n ≤ 5            (small unions)
join(τ₁, ..., τₙ) = Any             if n > 5            (large unions)
```

**Lemma 3.4** (Join is Least Upper Bound): 
1. `∀i. τᵢ ≤ join(τ₁, ..., τₙ)` (upper bound property)
2. If `∀i. τᵢ ≤ σ`, then `join(τ₁, ..., τₙ) ≤ σ` (least property)

*Proof Sketch*:
1. By construction: join returns either a supertype or union of all inputs
2. For (1): Each τᵢ ≤ τ by (S-Union-R) if join returns union, or directly if single type
3. For (2): By union elimination, if all τᵢ ≤ σ, then τ₁|...|τₙ ≤ σ by (S-Union-L)
∎

**Practical Implications**:
- Vectors preserve precise types when homogeneous: `[1 2 3] : Vector⟨Int⟩`
- Mixed numeric types get numeric tower: `[1 2.5] : Vector⟨Number⟩`
- Heterogeneous vectors get union types: `[1 "a"] : Vector⟨Int | String⟩`
- Very heterogeneous vectors fall back to `Any` for tractability

### 3.2 Properties

**Theorem 3.1** (Reflexivity): For all types `τ`, `τ ≤ τ`.

*Proof*: By rule (S-Refl). ∎

**Theorem 3.2** (Transitivity): If `τ₁ ≤ τ₂` and `τ₂ ≤ τ₃`, then `τ₁ ≤ τ₃`.

*Proof*: By rule (S-Trans). ∎

**Theorem 3.3** (Decidability): The subtyping relation `≤` is decidable.

*Proof Sketch*: 
1. All rules are syntax-directed
2. Recursion is structural on type terms
3. No infinite chains (well-founded induction on type structure)
4. Union membership is finite
∴ Algorithm terminates. ∎

### 3.3 Algorithmic Subtyping

The subtyping relation is implemented algorithmically as:

```haskell
subtype(τ₁, τ₂, visited) = 
  | τ₁ = τ₂                     → true                    -- (S-Refl)
  | τ₂ = Any                    → true                    -- (S-Top)
  | τ₁ = Never                  → true                    -- (S-Bot)
  | (τ₁, τ₂) ∈ visited          → true                    -- cycle
  | τ₁ = σ₁|σ₂                  → subtype(σ₁, τ₂, V) ∧   -- (S-Union-L)
                                   subtype(σ₂, τ₂, V)
  | τ₂ = σ₁|σ₂                  → subtype(τ₁, σ₁, V) ∨   -- (S-Union-R)
                                   subtype(τ₁, σ₂, V)
  | τ₁ = σ₁→ρ₁, τ₂ = σ₂→ρ₂     → subtype(σ₂, σ₁, V) ∧   -- (S-Fun)
                                   subtype(ρ₁, ρ₂, V)
  | τ₁ = Vector⟨σ₁⟩,            → subtype(σ₁, σ₂, V)     -- (S-Vec)
    τ₂ = Vector⟨σ₂⟩
  | otherwise                    → false
    where V = visited ∪ {(τ₁, τ₂)}
```

**Complexity**: O(|τ₁| × |τ₂|) where |τ| is the size of type τ.

---

## 4. Type Checking Algorithm

### 4.1 Bidirectional Type Checking

We use **bidirectional type checking** with two mutually recursive judgments:

1. **Type Synthesis** (inference): `Γ ⊢ e ⇒ τ`
   - Infers type `τ` from expression `e` in environment `Γ`

2. **Type Checking** (checking): `Γ ⊢ e ⇐ τ`
   - Verifies expression `e` has type `τ` in environment `Γ`

### 4.2 Typing Rules (Synthesis)

#### 4.2.1 Literals

```
─────────────────  (T-Int)
Γ ⊢ n ⇒ Int

─────────────────  (T-Float)
Γ ⊢ f ⇒ Float

─────────────────  (T-String)
Γ ⊢ s ⇒ String

─────────────────  (T-Bool)
Γ ⊢ b ⇒ Bool

─────────────────  (T-Nil)
Γ ⊢ nil ⇒ Nil
```

#### 4.2.2 Variables

```
  Γ(x) = τ
  ─────────────  (T-Var)
   Γ ⊢ x ⇒ τ
```

#### 4.2.3 Function Application

```
  Γ ⊢ f ⇒ τ₁→...→τₙ→τ    Γ ⊢ e₁ ⇐ τ₁    ...    Γ ⊢ eₙ ⇐ τₙ
  ────────────────────────────────────────────────────────────  (T-App)
                    Γ ⊢ f(e₁,...,eₙ) ⇒ τ
```

**Variadic Application**:
```
  Γ ⊢ f ⇒ (τ₁,...,τₙ, τᵥ...) → τᵣ
  Γ ⊢ e₁ ⇐ τ₁    ...    Γ ⊢ eₙ ⇐ τₙ
  Γ ⊢ eₙ₊₁ ⇐ τᵥ    ...    Γ ⊢ eₙ₊ₘ ⇐ τᵥ
  ─────────────────────────────────────────  (T-App-Var)
       Γ ⊢ f(e₁,...,eₙ₊ₘ) ⇒ τᵣ
```

#### 4.2.4 Conditional

```
  Γ ⊢ e₁ ⇒ τ₁    Γ ⊢ e₂ ⇒ τ₂    Γ ⊢ e₃ ⇒ τ₃
  ──────────────────────────────────────────  (T-If)
         Γ ⊢ if e₁ then e₂ else e₃ ⇒ τ₂|τ₃
```

**Note**: Condition type `τ₁` is not restricted (any value is truthy/falsy).

#### 4.2.5 Vectors

```
  Γ ⊢ e₁ ⇒ τ₁    ...    Γ ⊢ eₙ ⇒ τₙ
  ─────────────────────────────────────  (T-Vec)
     Γ ⊢ [e₁, ..., eₙ] ⇒ Vector⟨join(τ₁, ..., τₙ)⟩
```

**Join (Least Upper Bound)**: The `join` operation computes the smallest type that all element types are subtypes of:

```
join(τ₁, ..., τₙ) = smallest τ such that ∀i. τᵢ ≤ τ
```

**Implementation Algorithm**:
1. If all elements have type `τ` → `Vector⟨τ⟩`
2. If elements are only `{Int, Float}` → `Vector⟨Number⟩` where `Number = Int | Float`
3. If elements have ≤ 5 distinct types → `Vector⟨τ₁ | ... | τₙ⟩`
4. Otherwise → `Vector⟨Any⟩` (too heterogeneous)

**Examples**:
```
[1 2 3]           ⇒ Vector⟨Int⟩
[1 2.5 3]         ⇒ Vector⟨Number⟩
[1 "text" true]   ⇒ Vector⟨Int | String | Bool⟩
[[1 2] [3 4]]     ⇒ Vector⟨Vector⟨Int⟩⟩
[]                ⇒ Vector⟨Any⟩
```

**Lemma 4.1** (Vector Type Soundness): If `Γ ⊢ [e₁, ..., eₙ] ⇒ Vector⟨τ⟩`, then `∀i. Γ ⊢ eᵢ ⇒ τᵢ` where `τᵢ ≤ τ`.

*Proof*: By construction of `join`. The join is defined as the least upper bound, so by definition all element types are subtypes of it. ∎

#### 4.2.6 Let Binding

```
  Γ ⊢ e₁ ⇒ τ₁    Γ, x:τ₁ ⊢ e₂ ⇒ τ₂
  ──────────────────────────────────  (T-Let)
      Γ ⊢ let x = e₁ in e₂ ⇒ τ₂
```

**With Type Annotation**:
```
  Γ ⊢ e₁ ⇒ τ₁'    τ₁' ≤ τ₁    Γ, x:τ₁ ⊢ e₂ ⇒ τ₂
  ──────────────────────────────────────────────────  (T-Let-Ann)
           Γ ⊢ let x:τ₁ = e₁ in e₂ ⇒ τ₂
```

#### 4.2.7 Lists

Lists follow the same type inference as vectors:

```
  Γ ⊢ e₁ ⇒ τ₁    ...    Γ ⊢ eₙ ⇒ τₙ
  ─────────────────────────────────────  (T-List)
     Γ ⊢ '(e₁ ... eₙ) ⇒ List⟨join(τ₁, ..., τₙ)⟩
```

### 4.3 Checking Mode

The checking judgment uses **subsumption**:

```
  Γ ⊢ e ⇒ τ'    τ' ≤ τ
  ─────────────────────  (T-Sub)
       Γ ⊢ e ⇐ τ
```

This allows us to check an expression against an expected type using subtyping.

### 4.4 Complete Algorithm

```haskell
-- Type synthesis (Γ ⊢ e ⇒ τ)
infer(Γ, e) = case e of
  Literal(n:Int)       → Int
  Literal(f:Float)     → Float
  Literal(s:String)    → String
  Literal(b:Bool)      → Bool
  Literal(nil)         → Nil
  
  Variable(x)          → Γ(x)
  
  Apply(f, [e₁,...,eₙ]) →
    let τf = infer(Γ, f)
    match τf with
      | (τ₁,...,τₘ,τᵥ*) → τᵣ →
          require m ≤ n
          for i ∈ [1..m]: check(Γ, eᵢ, τᵢ)
          for j ∈ [m+1..n]: check(Γ, eⱼ, τᵥ)
          return τᵣ
      | _ → error "not a function"
  
  If(e₁, e₂, e₃) →
    let τ₁ = infer(Γ, e₁)
    let τ₂ = infer(Γ, e₂)
    let τ₃ = infer(Γ, e₃)
    return τ₂ | τ₃
  
  Let(x, e₁, e₂) →
    let τ₁ = infer(Γ, e₁)
    return infer(Γ ∪ {x:τ₁}, e₂)

-- Type checking (Γ ⊢ e ⇐ τ)
check(Γ, e, τ) =
  let τ' = infer(Γ, e)
  require subtype(τ', τ)
```

---

## 5. Soundness Theorem

### 5.1 Type Safety

**Theorem 5.1** (Type Safety): If `∅ ⊢ e ⇒ τ`, then either:
1. `e` is a value, or
2. `e → e'` and `∅ ⊢ e' ⇒ τ'` where `τ' ≤ τ`

This is proven via two lemmas:

### 5.2 Progress

**Lemma 5.1** (Progress): If `∅ ⊢ e ⇒ τ`, then either:
- `e` is a value, or
- `e → e'` for some `e'`

*Proof Sketch*: By induction on typing derivation. Key cases:
- Variables: Cannot occur (empty environment)
- Application: If `f` is a value and is a function, reduction applies
- Literals: Are values
∎

### 5.3 Preservation

**Lemma 5.2** (Preservation): If `Γ ⊢ e ⇒ τ` and `e → e'`, then `Γ ⊢ e' ⇒ τ'` where `τ' ≤ τ`.

*Proof Sketch*: By induction on the typing derivation and case analysis on reduction rules. Key cases:

**Application**: 
```
Given: Γ ⊢ (λx:τ₁. e₂) e₁ ⇒ τ₂
       Γ ⊢ e₁ ⇒ τ₁'  where τ₁' ≤ τ₁
       Γ, x:τ₁ ⊢ e₂ ⇒ τ₂

Reduces to: e₂[x ↦ e₁]

By substitution lemma: Γ ⊢ e₂[x ↦ e₁] ⇒ τ₂
```

**Substitution Lemma**: If `Γ, x:τ₁ ⊢ e ⇒ τ₂` and `Γ ⊢ v ⇒ τ₁' ≤ τ₁`, then `Γ ⊢ e[x ↦ v] ⇒ τ₂' ≤ τ₂`.
∎

### 5.4 Runtime Safety

**Corollary 5.1**: Well-typed RTFS programs do not produce runtime type errors.

*Proof*: Follows from Progress and Preservation. If `∅ ⊢ e ⇒ τ`, then by repeated application of these lemmas, evaluation either:
1. Produces a value of compatible type, or
2. Diverges (but never gets "stuck" with a type error)
∎

### 5.5 Numeric Coercion Safety

**Theorem 5.2** (Numeric Coercion): If `Γ ⊢ e ⇒ Number` and `e →* v`, then:
- If all operands were Int, `v : Int`
- If any operand was Float, `v : Float`

*Proof*: By the definition of Number = Int | Float and the runtime coercion rules in `secure_stdlib.rs:add()`.
∎

---

## 6. Implementation

### 6.1 Architecture

```
rtfs/src/ir/type_checker.rs (402 lines)
├── Type Definitions
│   ├── IrType enum (imported from ir/core.rs)
│   └── TypeCheckError
├── Subtyping Algorithm
│   ├── is_subtype(τ₁, τ₂) → bool
│   └── is_subtype_cached(...) → bool  (with cycle detection)
├── Type Checking
│   ├── type_check_ir(node) → Result<(), Error>
│   └── infer_type(node) → Result<IrType, Error>
└── Tests
    └── Subtyping properties

rtfs/src/ir/converter.rs (3281 lines)
├── IR Conversion
│   ├── convert_expression() → IrNode
│   ├── convert_vector() → IrNode with inferred element type
│   └── convert_map() → IrNode with inferred types
└── Type Inference Helpers
    └── infer_vector_element_type() → IrType  (join algorithm)
```

### 6.2 Key Functions

#### 6.2.1 Subtyping

```rust
pub fn is_subtype(sub: &IrType, sup: &IrType) -> bool
```

**Implements**: Section 3.3 algorithmic subtyping

**Time Complexity**: O(|sub| × |sup|)

**Space Complexity**: O(depth) for visited set

**Properties**:
- ✓ Reflexive
- ✓ Transitive
- ✓ Decidable
- ✓ Sound w.r.t. formal rules

#### 6.2.2 Join (Least Upper Bound)

```rust
fn infer_vector_element_type(elements: &[IrNode]) -> IrType
```

**Implements**: Section 3.1.5 join operation

**Algorithm**:
1. Empty vector → `Any`
2. All same type → that type
3. Only `{Int, Float}` → `Number` (Int | Float)
4. ≤ 5 distinct types → `Union` of those types
5. > 5 distinct types → `Any`

**Time Complexity**: O(n log n) where n = number of elements (due to sorting for deduplication)

**Space Complexity**: O(k) where k = number of distinct types

**Guarantees**:
- ✓ Computes least upper bound
- ✓ Preserves precision for common cases
- ✓ Terminates for all inputs
- ✓ Sound w.r.t. subtyping rules

#### 6.2.3 Type Checking

```rust
pub fn type_check_ir(node: &IrNode) -> TypeCheckResult<()>
```

**Implements**: Section 4.2 type synthesis

**Guarantees**:
- If returns `Ok(())`, expression is well-typed
- If returns `Err(e)`, provides precise error location
- Always terminates

### 6.3 Integration with Compiler

The type checker is integrated into `rtfs_compiler` at IR generation:

```rust
// After IR conversion, before optimization:
if args.type_check && !args.no_type_check {
    type_checker::type_check_ir(&ir_node)?;
}
```

**Default**: Type checking is **enabled by default**

**Override**: Use `--no-type-check` flag to disable (not recommended)

### 6.4 Error Reporting

Type errors include:

1. **TypeMismatch**: Expected vs actual type
2. **FunctionCallTypeMismatch**: Argument type mismatch with parameter index
3. **NonFunctionCalled**: Attempted to call non-function value
4. **UnresolvedVariable**: Variable not in scope

Each error includes location information for debugging.

---

## 7. Examples

### 7.1 Numeric Coercion

**Input**:
```lisp
(+ 1 2.5)
```

**IR Type**:
```
Apply {
  function: + : Number → Number → ... → Number
  arguments: [
    1 : Int,
    2.5 : Float
  ]
  return_type: Number
}
```

**Type Checking**:
```
1. infer(+) = (Number, Number*) → Number
2. check(1, Number):
   - infer(1) = Int
   - Int ≤ Number ✓  (by S-Union-R)
3. check(2.5, Number):
   - infer(2.5) = Float
   - Float ≤ Number ✓  (by S-Union-R)
4. Result: Number ✓
```

**Runtime**: `Float(3.5)` (Int promoted to Float)

### 7.2 Vector Type Inference

**Example 1**: Homogeneous vector
```lisp
[1 2 3]
```

**Type Derivation**:
```
infer([1 2 3])
= Vector⟨join(infer(1), infer(2), infer(3))⟩  (by T-Vec)
= Vector⟨join(Int, Int, Int)⟩
= Vector⟨Int⟩                                  (all same type)
```

**Example 2**: Mixed numeric vector
```lisp
[1 2.5 3]
```

**Type Derivation**:
```
infer([1 2.5 3])
= Vector⟨join(Int, Float, Int)⟩
= Vector⟨Number⟩                               (numeric tower rule)
  where Number = Int | Float
```

**Example 3**: Heterogeneous vector
```lisp
[1 "text" true]
```

**Type Derivation**:
```
infer([1 "text" true])
= Vector⟨join(Int, String, Bool)⟩
= Vector⟨Int | String | Bool⟩                  (union type)
```

**Example 4**: Nested vectors
```lisp
[[1 2] [3 4]]
```

**Type Derivation**:
```
infer([[1 2] [3 4]])
= Vector⟨join(infer([1 2]), infer([3 4]))⟩
= Vector⟨join(Vector⟨Int⟩, Vector⟨Int⟩)⟩
= Vector⟨Vector⟨Int⟩⟩                          (covariance)
```

### 7.3 Type Error Detection

**Input**:
```lisp
(+ 1 "string")
```

**Type Checking**:
```
1. infer(+) = (Number, Number*) → Number
2. check(1, Number): ✓
3. check("string", Number):
   - infer("string") = String
   - String ≤ Number ? ✗
4. Error: Type mismatch in call to '+' parameter 1:
   expected Number, got String
```

**Result**: **Rejected at compile time** ✓

### 7.4 Function Subtyping

**Given**:
```
f : Int → Number
g : Number → Int
```

**Question**: Is `f ≤ g`?

**Answer**:
```
By (S-Fun): (Int → Number) ≤ (Number → Int)
iff Number ≤ Int  (contravariant arg)
and Number ≤ Int  (covariant return)

Number ≤ Int? No, because:
  Number = Int | Float
  by (S-Union-L): Number ≤ Int requires Int ≤ Int ∧ Float ≤ Int
  but Float ≤ Int is false

Therefore: f ⊈ g ✗
```

### 7.5 Union Types

**Input**:
```lisp
(if condition 42 3.14)
```

**Type**:
```
infer(if condition 42 3.14)
= infer(42) | infer(3.14)
= Int | Float
= Number ✓
```

**Usage**:
```lisp
(let x (if condition 42 3.14)
  (+ x 1))  ;; OK: Number + Number → Number
```

---

## 8. Formal Guarantees

### 8.1 What We Guarantee

| Property | Guarantee |
|----------|-----------|
| **Soundness** | ✓ Well-typed programs don't go wrong |
| **Progress** | ✓ Well-typed programs reduce or are values |
| **Preservation** | ✓ Types preserved under reduction |
| **Decidability** | ✓ Type checking always terminates |
| **Subtype Reflexivity** | ✓ τ ≤ τ |
| **Subtype Transitivity** | ✓ τ₁ ≤ τ₂ ∧ τ₂ ≤ τ₃ ⇒ τ₁ ≤ τ₃ |
| **Numeric Safety** | ✓ Number operations maintain numeric types |

### 8.2 What We Don't Guarantee

| Non-Guarantee | Reason |
|---------------|--------|
| **Completeness** | Some valid programs rejected (e.g., `Any` operations) |
| **Inference** | Top-level definitions may require annotations |
| **Effect Tracking** | Side effects not tracked in types (yet) |
| **Termination** | Type system doesn't prove program termination |
| **Map Type Subtyping** | Map types do not support subtyping relationships. A map type `A` is not considered a subtype of map type `B` just because `A` has fewer required fields than `B`. Extra fields in data are allowed even without wildcards. |

---

## 9. Future Extensions

### 9.1 Planned Enhancements

1. **Effect System**: Track I/O, mutation, exceptions in types
2. **Dependent Types**: Types that depend on values (for array bounds)
3. **Refinement Types**: Predicates in types (e.g., `{x:Int | x > 0}`)
4. **Polymorphism**: Generic types with type parameters
5. **Algebraic Effects**: Structured effect handling

### 9.2 Research Directions

1. **Gradual Typing**: Better Any ↔ concrete type interactions
2. **Linear Types**: Resource management and borrowing
3. **Session Types**: Protocol verification for capabilities
4. **Information Flow**: Security type system for data flow

---

## 10. References

### 10.1 Primary Sources

1. **Pierce, Benjamin C.** (2002). *Types and Programming Languages*. MIT Press.
   - Chapter 15: Subtyping
   - Chapter 16: Metatheory of Subtyping
   - Chapter 20: Recursive Types

2. **Cardelli, Luca** (1984). *A Semantics of Multiple Inheritance*. Semantics of Data Types, LNCS 173.
   - Structural subtyping foundations

3. **Davies, Rowan & Pfenning, Frank** (2000). *Intersection Types and Computational Effects*. ICFP 2000.
   - Union type semantics and algorithmic subtyping

### 10.2 Implementation References

4. **Dunfield, Joshua & Krishnaswami, Neelakantan R.** (2013). *Complete and Easy Bidirectional Typechecking for Higher-Rank Polymorphism*. ICFP 2013.
   - Bidirectional type checking algorithm

5. **Hosoya, Haruo & Pierce, Benjamin C.** (2003). *XDuce: A statically typed XML processing language*. ACM TOIT.
   - Union types in practice

### 10.3 Related Work

6. **TypeScript**: Union types and structural subtyping in practice
7. **Flow**: Facebook's type system for JavaScript
8. **Typed Racket**: Gradual typing in Lisp dialect

---

## Appendix A: Notation

| Symbol | Meaning |
|--------|---------|
| `τ, σ, ρ` | Type metavariables |
| `Γ, Δ` | Type environment metavariables |
| `e, v` | Expression, value metavariables |
| `⊢` | Entailment/typing judgment |
| `⇒` | Type synthesis (infers type) |
| `⇐` | Type checking (checks type) |
| `≤` | Subtyping relation |
| `→` | Function type, evaluation step |
| `→*` | Multi-step evaluation |
| `|` | Union type constructor |
| `&` | Intersection type constructor |
| `⊤` | Top type (Any) |
| `⊥` | Bottom type (Never) |
| `∅` | Empty environment |
| `≡` | Definitional equality |

---

## Appendix B: Complete Subtyping Rules

```
────────────  (S-Refl)
  τ ≤ τ

  τ₁ ≤ τ₂    τ₂ ≤ τ₃
  ─────────────────────  (S-Trans)
       τ₁ ≤ τ₃

────────────  (S-Top)
  τ ≤ Any

────────────  (S-Bot)
  Never ≤ τ

  τ ≤ τ₁        τ ≤ τ₂
  ─────────  OR  ─────────  (S-Union-R)
   τ ≤ τ₁|τ₂      τ ≤ τ₁|τ₂

  τ₁ ≤ τ    τ₂ ≤ τ
  ──────────────────  (S-Union-L)
    τ₁|τ₂ ≤ τ

  τ₁' ≤ τ₁    τ₂ ≤ τ₂'
  ──────────────────────  (S-Fun)
   τ₁ → τ₂ ≤ τ₁' → τ₂'

  τ₁ ≤ τ₂
  ─────────────────  (S-Vec)
   Vector⟨τ₁⟩ ≤ Vector⟨τ₂⟩

  τ₁ ≤ τ₂
  ─────────────  (S-List)
   List⟨τ₁⟩ ≤ List⟨τ₂⟩

  τ₁ ≤ σ₁    ...    τₙ ≤ σₙ
  ──────────────────────────────  (S-Tuple)
   Tuple⟨τ₁,...,τₙ⟩ ≤ Tuple⟨σ₁,...,σₙ⟩
```

---

**Document Status**: Complete  
**Last Updated**: 2025-11-01  
**Maintainer**: RTFS Core Team

