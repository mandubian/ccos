# RTFS Type Checking: Quick Reference Guide

**For**: Developers using RTFS  
**See Also**: [Full Formal Specification](../specs/13-type-system.md)

---

## Quick Start

### Enable Type Checking (Default)

```bash
rtfs_compiler --file mycode.rtfs
```

Type checking is **ON by default**. It validates your code before execution.

### Disable Type Checking (Not Recommended)

```bash
rtfs_compiler --file mycode.rtfs --no-type-check
```

Only use this for debugging or legacy code.

---

## Common Type Patterns

### 1. Numeric Operations

✅ **Allowed**: Mixed Int and Float
```lisp
(+ 1 2.5)        ; ✓ Result: Float(3.5)
(* 10 3.14)      ; ✓ Result: Float(31.4)
(/ 10 3)         ; ✓ Result: Float(3.333...)
```

**Type**: `Number = Int | Float`

❌ **Not Allowed**: Non-numeric operands
```lisp
(+ 1 "string")   ; ✗ Type error: expected Number, got String
```

### 2. Conditional Expressions

✅ **Union Types**: Different branch types create unions
```lisp
(if condition 
    42           ; Int
    3.14)        ; Float
; Result type: Number (Int | Float)
```

✅ **Any Type**: Use for maximum flexibility
```lisp
(if (complex-check)
    "success"
    {:status "error"})
; Result type: String | Map
```

### 3. Function Calls

✅ **Subtyping**: Arguments can be subtypes
```lisp
(defn process-number [x : Number] ...)

(process-number 42)      ; ✓ Int ≤ Number
(process-number 3.14)    ; ✓ Float ≤ Number
```

❌ **Type Mismatch**: Arguments must satisfy parameter types
```lisp
(defn process-number [x : Number] ...)

(process-number "text")  ; ✗ String ⊈ Number
```

### 4. Collections

✅ **Homogeneous Vectors**: All elements same type (precise inference)
```lisp
[1 2 3]          ; Vector<Int>       ✓ All integers
[1.0 2.5 3.14]   ; Vector<Float>     ✓ All floats
[1 2.5 3]        ; Vector<Number>    ✓ Mixed Int/Float → Number
```

✅ **Heterogeneous Vectors**: Automatic union types (NEW!)
```lisp
[1 "text" true]  ; Vector<Int | String | Bool>  ✓ Union type inferred!
[[1 2] [3 4]]    ; Vector<Vector<Int>>          ✓ Nested vectors
[]               ; Vector<Any>                   ✓ Empty vector
```

**Type Inference**: RTFS computes the **least upper bound** (join) of element types:
- Same type → precise type (e.g., `Vector<Int>`)
- Mixed numeric → `Number = Int | Float`
- ≤ 5 different types → union type
- > 5 different types → `Any` (too heterogeneous)

✅ **Maps**: Key-value pairs with types
```lisp
{:name "Alice" :age 30}
; Map<Keyword, String | Int>
```

---

## Type Annotations

### When to Annotate

1. **Function Parameters** (recommended):
```lisp
(defn add [x : Number, y : Number] : Number
  (+ x y))
```

2. **Let Bindings** (when type unclear):
```lisp
(let [x : Number (if condition 42 3.14)]
  (+ x 1))
```

3. **Explicit Conversions**:
```lisp
(let [x : Any (dynamic-load)]
  ...)  ; Treat as Any for now
```

### When Not to Annotate

- Literals: Type is obvious
- Local variables: Inferred from initialization
- Return types: Usually inferred

---

## Common Errors and Solutions

### Error: Type Mismatch

```
❌ Type error: expected Number, got String
```

**Solution**: Check argument types match function expectations
```lisp
; Before:
(+ x "5")

; After:
(+ x 5)  ; Use numeric literal
; OR
(+ x (parse-int "5"))  ; Parse string to number
```

### Error: Function Call Type Mismatch

```
❌ Type error in call to 'process' parameter 2:
   expected Vector<Number>, got Vector<String>
```

**Solution**: Ensure collection element types match
```lisp
; Before:
(process data ["1" "2" "3"])

; After:
(process data [1 2 3])
; OR
(process data (map parse-int ["1" "2" "3"]))
```

### Error: Non-Function Called

```
❌ Attempted to call non-function of type Int
```

**Solution**: Verify variable is actually a function
```lisp
; Before:
(let [x 42]
  (x 10))  ; x is Int, not a function

; After:
(let [f (fn [y] (+ y 10))]
  (f 42))
```

---

## Subtyping Rules (Simplified)

### Basic Subtyping

```
Int      ≤  Number     ✓
Float    ≤  Number     ✓
Number   ≤  Any        ✓
Never    ≤  τ          ✓  (for any type τ)
τ        ≤  τ          ✓  (everything is subtype of itself)
```

### Union Types

```
Int      ≤  Int | Float     ✓  (member of union)
String   ≤  String | Int    ✓
Int | Float  ≤  Number      ✓  (both members are subtypes)
```

### Functions (Contravariant Arguments!)

```
(Number → Int)  ≤  (Int → Number)  ✗

Why? Function subtyping is:
- Contravariant in arguments (flipped!)
- Covariant in return type (same direction)

Correct:
(Int → Number)  ≤  (Number → Int)  ✓
  because: Number ≤ Int (arg - flipped)
       and Int ≤ Number (return - same)
```

### Collections (Covariant)

```
Vector<Int>    ≤  Vector<Number>    ✓
List<Float>    ≤  List<Number>      ✓
```

---

## Advanced Patterns

### 1. Gradual Typing with Any

Use `Any` when type is unknown or too complex:

```lisp
(defn process-dynamic [data : Any] : Any
  ; Process unknown data structure
  ...)
```

**Note**: `Any` bypasses most type checks. Use sparingly!

### 2. Union Types for Variants

```lisp
(defn handle-result [r : {:ok Any} | {:error String}]
  (if (contains? r :ok)
      (get r :ok)
      (log-error (get r :error))))
```

### 3. Numeric Tower

RTFS follows the **numeric tower**:

```
Int + Int   → Int     (stays integer)
Int + Float → Float   (promotes to float)
Float + any → Float   (stays float)
```

Examples:
```lisp
(+ 2 3)      ; Int(5)
(+ 2 3.0)    ; Float(5.0)
(+ 2.5 1.5)  ; Float(4.0)
```

---

## Type Checking Flags

### Compiler Options

```bash
# Default: type checking ON
rtfs_compiler --file code.rtfs

# Verbose: show type check timing
rtfs_compiler --file code.rtfs --verbose
# Output: ✅ Type checking completed in 1.2ms

# Disable: skip type validation
rtfs_compiler --file code.rtfs --no-type-check

# With execution
rtfs_compiler --file code.rtfs --execute

# Multiple features
rtfs_compiler --file code.rtfs --dump-ir --type-check --execute
```

### In REPL

Type checking is **always enabled** in REPL for safety.

---

## Best Practices

### DO ✅

1. **Annotate function parameters**
   ```lisp
   (defn calculate [x : Number, y : Number] ...)
   ```

2. **Use specific types when possible**
   ```lisp
   ; Good
   (defn sum-ints [nums : Vector<Int>] ...)
   
   ; Less precise
   (defn sum-ints [nums : Any] ...)
   ```

3. **Let type inference work**
   ```lisp
   (let [x (+ 1 2)]  ; x inferred as Int
     ...)
   ```

4. **Handle union types explicitly**
   ```lisp
   (let [result (if cond 42 "error")]
     (if (int? result)
         (process-int result)
         (log-error result)))
   ```

### DON'T ❌

1. **Don't overuse `Any`**
   ```lisp
   ; Bad
   (defn process [x : Any] : Any ...)
   
   ; Better
   (defn process [x : Number | String] : Result ...)
   ```

2. **Don't ignore type errors**
   ```lisp
   ; Bad: using --no-type-check to hide errors
   
   ; Good: fix the actual type mismatch
   ```

3. **Don't fight the type system**
   ```lisp
   ; Bad: excessive type coercions
   (+ (any-to-int x) (any-to-int y))
   
   ; Good: proper types from the start
   (+ x y)  ; where x, y : Number
   ```

---

## Debugging Type Errors

### 1. Use --dump-ir to See Inferred Types

```bash
rtfs_compiler --file code.rtfs --dump-ir
```

Shows the IR with full type information:
```
Apply {
  function: + : (Number, Number*) → Number
  arguments: [
    Literal { value: 1, ir_type: Int },
    Literal { value: 2.5, ir_type: Float }
  ]
  ir_type: Number
}
```

### 2. Add Explicit Type Annotations

```lisp
; Before: type unclear
(let [x (complex-computation)]
  ...)

; After: explicit annotation helps debugging
(let [x : Number (complex-computation)]
  ...)
```

### 3. Check Subtype Relationships

If you get "type mismatch", verify subtyping:

```
Error: expected Vector<Number>, got Vector<Int>
```

This is actually **OK** because `Vector<Int> ≤ Vector<Number>`.

If you see this error, it might be a bug in the type checker—please report it!

---

## Performance Notes

### Type Checking Cost

- **Parsing**: ~1-5ms for typical files
- **Type Checking**: ~0.5-3ms additional
- **Total Overhead**: < 5-10% of compilation time

**Recommendation**: Keep type checking enabled. The safety benefits far outweigh the minimal performance cost.

### When to Disable

Only disable type checking for:
1. **Debugging type checker itself**
2. **Legacy code migration** (temporarily)
3. **Benchmarking runtime performance** (exclude compile time)

---

## Examples from Real Code

### Example 1: Numeric Calculator

```lisp
(defn calculate [op : String, x : Number, y : Number] : Number
  (cond
    (= op "+") (+ x y)
    (= op "-") (- x y)
    (= op "*") (* x y)
    (= op "/") (/ x y)
    :else (error "Unknown operation")))

(calculate "+" 10 20)      ; ✓ Int(30)
(calculate "*" 2.5 4)      ; ✓ Float(10.0)
(calculate "/" 10 3)       ; ✓ Float(3.333...)
```

### Example 2: Data Processing Pipeline

```lisp
(defn process-data [items : Vector<{:name String, :value Number}>]
                    : Vector<Number>
  (map (fn [item] (get item :value)) items))

(process-data [{:name "a" :value 10}
               {:name "b" :value 20.5}])
; ✓ Result: Vector<Number> = [10, 20.5]
```

### Example 3: Error Handling

```lisp
(defn safe-divide [x : Number, y : Number] 
                   : {:ok Number} | {:error String}
  (if (= y 0)
      {:error "Division by zero"}
      {:ok (/ x y)}))

(let [result (safe-divide 10 2)]
  (if (contains? result :ok)
      (print (get result :ok))
      (print-error (get result :error))))
; ✓ Properly typed union handling
```

---

## Further Reading

- **[Full Formal Specification](../specs/13-type-system.md)**: Complete type theory with soundness proofs
- **[RTFS 2.0 Specifications](../specs/)**: Complete language specifications
- **[RTFS Language Overview](../specs/01-language-overview.md)**: General RTFS syntax

---

## FAQ

**Q: Why does `(+ 1 2)` return `Int` but `(+ 1 2.0)` return `Float`?**

A: RTFS follows the **numeric tower**. Integer operations stay integers for precision. Mixed operations promote to Float.

**Q: Can I disable type checking for just one function?**

A: Not currently. Type checking is file-level. Use `Any` types for dynamic code within a function.

**Q: What's the difference between `Any` and union types?**

A: `Any` is the **top type** (accepts everything, no checks). Union types like `Int | String` are **precise** and type-checked.

**Q: Why do I get errors with `--type-check` but code runs fine with `--no-type-check`?**

A: Runtime type checks are more permissive than static checks. Static checking is **conservative** (rejects some valid programs) to guarantee safety.

**Q: How do I annotate variadic functions?**

A: Use `...` in the signature:
```lisp
(defn sum [first : Number, rest : Number...] : Number
  (reduce + first rest))
```

---

**Last Updated**: 2025-11-01

