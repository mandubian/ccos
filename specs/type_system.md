# RTFS - Type System Specification (Draft)

This document specifies the type system for the standalone RTFS language, designed to be **gradual** and **structural**.

## 1. Goals

- **Safety:** Catch type errors early, ideally before runtime, especially for AI-generated code.
- **Clarity:** Make intent and data shapes explicit through annotations and schemas.
- **Flexibility (Gradual Typing):** Allow mixing typed and untyped code. Type annotations are optional but encouraged. The system ensures safe interaction between typed and untyped parts, potentially via runtime checks at boundaries.
- **Tooling:** Enable better static analysis, autocompletion, and refactoring support.
- **Contracts:** Provide the formal language for defining `:input-schema` and `:output-schema` in task contracts.

## 2. Core Concepts

- **Gradual Typing:** Annotations are optional. Where present, they are checked statically (by a type checker) or dynamically (at runtime boundaries). Unannotated code behaves dynamically.
- **Structural Typing:** Type compatibility is determined by shape/structure, not by nominal type names (similar to TypeScript or Go interfaces). A map `{:a :int}` is compatible with a required type `{:a :int :b :string?}` (where `?` means optional) if the required fields match.
- **Type Inference:** The system should attempt to infer types where annotations are missing, reducing annotation burden.

## 3. Type Syntax

Type annotations typically follow the value or parameter name, preceded by a colon (`:`).

```acl
(def x :int 10)
(let [y :string "hello"] ...)
(fn [a :bool b :any] :int ...) ;; :any represents dynamic type
```

## 4. Primitive Types

- `:int` (Arbitrary precision integer)
- `:float` (64-bit floating point)
- `:string` (UTF-8 string)
- `:bool` (`true` or `false`)
- `:nil` (The type of the `nil` value)
- `:keyword` (Keyword type, e.g., `:my-key`)
- `:symbol` (Symbol type, e.g., `'my-var`)
- `:any` (Top type, equivalent to untyped/dynamic)
- `:never` (Bottom type, for functions that never return)

## 5. Composite Types

- **Vector/List:** `[:vector T]` or `[:list T]` (Type `T` for all elements). Example: `[:vector :int]`
- **Tuple:** `[:tuple T1 T2 ... Tn]` (Fixed size, specific type for each element). Example: `[:tuple :string :int :bool]`
- **Map:** `[:map [:key1-kw Type1] [:key2-kw Type2?] ...]` (Defines required and optional (`?`) key-value pairs). Keys **must** be keywords. Example: `[:map [:name :string] [:age :int?]]`
- **Struct/Record:** (Alternative to Map for more nominal typing if needed - TBD). `[:struct name [:field1 Type1] ...]`
- **Union:** `[:union Type1 Type2 ...]` (Value can be one of the specified types). Example: `[:union :int :string]`
- **Intersection/Refinement:** `[:and Type PredicateSchema]` (Value must match Type and satisfy PredicateSchema). Example: `[:and :int [:> 0]]` (Positive integer)

## 6. Function Types

- Syntax: `[:=> [:cat ParamType1 ParamType2 ...] ReturnType]`
- Example: `[:=> [:cat :int :string] :bool]` (Function taking int, string, returning bool)
- Variadic functions: `[:=> [:cat FixedParam1 FixedParam2 ... [:* VarParamType]] ReturnType]`
- Named parameters: Handled by defining functions that accept a single map argument conforming to a specific `[:map ...]` schema. Destructuring within the function definition can be used for convenient access to map values.
  - _(Note: This approach was chosen over dedicated keyword argument syntax to maintain consistency with the data-oriented nature of RTFS, leverage existing map structures and type definitions, and avoid adding extra complexity to the core grammar and type system for function calls. It relies on a robust destructuring mechanism within function definitions.)_

## 7. Resource Types

- Special types representing opaque handles to external resources (e.g., `FileHandle`, `SocketHandle`, `DatabaseConnectionHandle`, `ApiSessionHandle`).
- These types are also used for complex, externally managed data structures common in AI, such as `TensorHandle` or `ModelHandle`. RTFS manages the handles, while external tools operate on the underlying data.
- These types are central to the resource management system (see `resource_management.md`) and may have specific rules related to ownership and consumption (simulating linear types).

## 8. Schema Definition Language

- The type syntax described above (using vectors and keywords) serves as the schema definition language used within `:input-schema` and `:output-schema` contracts.
- This allows contracts to be represented directly as RTFS data, facilitating validation and manipulation.
- **Map Schemas:**
  - `[:map [:key1-kw Type1] [:key2-kw Type2?] ...]` defines a map structure.
  - Keys **must** be keywords.
  - A `?` suffix on the type indicates the key is optional (e.g., `[:age :int?]`).
  - By default, maps are **closed**: only the specified keys are allowed.
  - To define an **open map** (allowing additional keys of a specific type), add a final entry `[:* ValueType]`. `[:* :any]` allows any extra keys. Example: `[:map [:name :string] [:* :any]]` allows a `:name` key and any other keys.
- **Predicates and Refinements:**

  - `[:and Type PredicateSchema ...]` requires the value to conform to `Type` _and_ satisfy all `PredicateSchema`s.
  - `[:union Type1 Type2 ...]` allows the value to conform to _any_ of the specified types. (_Renamed from `:or`_)
  - **Predicate Schema Syntax:** A predicate schema is a list `[PredicateName Arg1 Arg2 ...]`.
    - `PredicateName`: A keyword or symbol identifying the validation function (e.g., `:>`, `:string-starts-with`, `:count`).
    - `Arg1, Arg2, ...`: Literal values passed as arguments to the predicate function _after_ the value being validated. The value being validated is implicitly the first argument to the underlying predicate logic.
  - **Common Predicates (Examples):**
    - **Comparison:**
      - `[:= literal]` (Equal to literal)
      - `[:!= literal]` (Not equal to literal)
      - `[:> number]` (Greater than number)
      - `[:>= number]` (Greater than or equal to number)
      - `[:< number]` (Less than number)
      - `[:<= number]` (Less than or equal to number)
    - **String Operations:**
      - `[:string-length number]` (String length equals number)
      - `[:string-min-length number]` (String length >= number)
      - `[:string-max-length number]` (String length <= number)
      - `[:string-starts-with prefix-string]`
      - `[:string-ends-with suffix-string]`
      - `[:string-contains substring]`
      - `[:string-matches-regex regex-string]`
    - **Collection Operations:**
      - `[:count number]` (Collection size equals number)
      - `[:min-count number]` (Collection size >= number)
      - `[:max-count number]` (Collection size <= number)
      - `[:contains literal]` (Collection contains the literal value)
      - `[:contains-all [lit1 lit2 ...]]`
      - `[:contains-any [lit1 lit2 ...]]`
    - **Type Checks (Less common in `[:and]` but possible):**
      - `[:is-int?]`
      - `[:is-string?]`
      - `[:is-map?]`
      - `[:is-vector?]`
    - **Enum:**
      - `[:enum lit1 lit2 ...]` (Value must be one of the listed literals)
      - This is conceptually equivalent to `[:union [:val Val1] [:val Val2] ...]`
  - **Predicate Resolution:** The runtime/typechecker resolves the `PredicateName` to a specific validation function. The set of available standard predicates should be defined by the RTFS standard library/runtime. Custom predicates might be possible via extensions.

- **Example Contract Schemas:**

  ```acl
  ;; Input requires a URL string (must be http/https) and an optional non-negative integer for retries.
  :input-schema [:map
                   [:url [:and :string
                            [:union [:string-starts-with "http://"] [:string-starts-with "https://"]]]]
                   [:retries [:and :int [:>= 0]]?]] ;; Optional key, non-negative int

  ;; Output guarantees a map with a non-empty summary string and word count integer.
  ;; The map is open, allowing other keys.
  :output-schema [:map
                    [:summary [:and :string [:string-min-length 1]]]
                    [:word-count :int]
                    [:* :any]] ;; Allow extra keys

  ;; Input requires either an integer ID or a map with name and email.
  :input-user-schema [:union ;; Renamed from :or
                        [:and :int [:> 0]] ;; Positive integer ID
                        [:map [:name :string] [:email [:and :string [:string-contains "@"]]]]] ;; Map with name and basic email check
  ```

## 9. Type Checking and Runtime Interaction

- **Static Checking:** A type checker (part of the RTFS toolchain) analyzes annotated code, performs inference, and reports type errors before execution/transpilation.
- **Runtime Checks:** At the boundary between typed and untyped code (or when validating contracts), runtime checks may be inserted by the compiler/transpiler to ensure type safety. If a check fails, a runtime type error is raised.
- **Type Errors:** Both static and runtime type errors should be structured and informative.

## 10. Type Inference (Basic Rules)

While RTFS supports explicit type annotations, the type system should also perform inference to reduce boilerplate and improve usability. Here are some basic inference rules:

- **Literals:** The type of a literal is directly inferred (e.g., `42` is `:int`, `"hello"` is `:string`).
- **`let` Bindings:**
  - If a `let` binding `[name expr]` has no type annotation, the type of `name` is inferred to be the type of `expr`.
  - Example: `(let [x 10] ...)` infers `x` to be `:int`.
  - Example: `(let [y (tool:get-string)] ...)` infers `y` to be the return type specified in the signature of `tool:get-string`.
- **Function Return Types:**
  - If an `fn` or `defn` has no return type annotation, its return type is inferred from the type of the _last expression_ in its body.
  - Example: `(fn [a :int] (+ a 1))` infers a return type of `:int`.
  - If the body is empty or the last expression's type cannot be determined (e.g., it's `:any` or depends on unannotated parameters), the inferred return type might default to `:any`.
- **`if` Expressions:**
  - The type of an `if` expression is the _common supertype_ of its `then` and `else` branches. If the types are incompatible and have no common supertype other than `:any`, the result is `:any`.
  - Example: `(if flag "a" "b")` infers `:string`.
  - Example: `(if flag 1 "b")` infers `[:union :int :string]` (or potentially `:any` depending on how strict the supertype calculation is).
- **`do` Expressions:** The type of a `do` block is the type of its _last expression_.
- **`match` Expressions:** The type of a `match` expression is the _common supertype_ of the last expressions of all its clause bodies.
- **`try/catch` Expressions:** The type of a `try/catch` expression is the _common supertype_ of the last expression of the `try` body and the last expressions of all `catch` bodies.
- **Collections:** The types of literal collections (`vector`, `map`) can be inferred based on their contents, potentially resulting in union types if elements differ (e.g., `[1 "a"]` infers `[:vector [:union :int :string]]`).

**Interaction with `:any`:**

- If an expression involves operands of type `:any`, the result is often inferred as `:any` unless the operation itself imposes a specific type constraint (e.g., boolean operators might still yield `:bool`).
- Type inference aims to provide the most specific type possible but falls back to `:any` when insufficient information is available.

_(This specification needs further refinement, particularly around generics, polymorphism, detailed inference rules, and the exact schema language syntax.)_

## 11. Type Definitions

### 11.1. Primitive Types

- `:int`: Arbitrary-precision integers.
- `:float`: Floating-point numbers (e.g., IEEE 754 double precision).
- `:string`: Unicode strings.
- `:bool`: Boolean values (`true`, `false`).
- `:nil`: The null or absence value.
- `:keyword`: Namespaced keyword identifiers (e.g., `:user/id`, `:status`).
- `:symbol`: Symbolic identifiers, often used for code representation or names.
- `:any`: The top type, compatible with any other type. Used for dynamic typing.
- `:never`: The bottom type, representing computations that never return normally (e.g., throwing an error, infinite loop). Subtype of all types.

### 11.2. Collection Types

- **Vector:** `[:vector ElementType Shape1D?]`

  - Ordered, indexed collection.
  - `ElementType`: The type of elements in the vector.
  - `Shape1D`: Optional. A list containing a single dimension (`integer` or `?`). If present, specifies the required size. `?` indicates unknown/dynamic size.
  - Example: `[:vector :int]`, `[:vector :float [10]]`, `[:vector :string [?]]`

- **Array/Tensor:** `[:array ElementType Shape]`

  - Multi-dimensional, indexed collection, suitable for tensors or matrices.
  - `ElementType`: The type of elements in the array.
  - `Shape`: Required. A list of dimensions (`integer` or `?`). `?` indicates an unknown/dynamic dimension.
  - Example: `[:array :float [100 100 3]]` (100x100x3 float tensor), `[:array :int [? 128]]` (Nx128 integer matrix).

- **List:** `[:list ElementType]`

  - Ordered collection, potentially optimized for sequential access (if distinct from vector).
  - Example: `[:list :any]`

- **Tuple:** `[:tuple Type1 Type2 ...]`

  - Ordered collection with a fixed number of elements, each potentially having a different type.
  - Example: `[:tuple :string :int :bool]`

- **Map:** `[:map [KeyType1 ValueType1 Opt1?] [KeyType2 ValueType2 Opt2?] ... Wildcard?]`
  - Unordered collection of key-value pairs.
  - Keys must be keywords.
  - `Opt?`: Optional marker `?` indicates the key is optional.
  - `Wildcard`: Optional `[:* ValueType]` allows additional keys not explicitly listed, all mapping to `ValueType`.
  - Example: `[:map [:user-id :int] [:name :string?] [:* :any]]` (Requires `:user-id`, optional `:name`, allows other keys of any type).

### 11.3. Function Types

- `[:=> [ParamType1 ParamType2 ...] ReturnType]`
- `[:=> [ParamType1 ...] [:* VariadicParamType] ReturnType]` (Variadic)
  - Represents the signature of a function.
  - Example: `[:=> [:int :string] :bool]`, `[:=> [:number] [:* :number] :number]`

### 11.4. Resource Types

- `[:resource ResourceName]`
  - Represents an opaque handle to an external resource managed by the runtime.
  - `ResourceName`: A symbol identifying the kind of resource (e.g., `FileHandle`, `DatabaseConnectionHandle`).
  - Example: `[:resource FileHandle]`

### 11.5. Union Types

- `[:union Type1 Type2 ...]` (_Renamed from `:or` for clarity_)
  - Represents a value that can be one of the specified types.
  - Example: `[:union :string :int]`

### 11.6. Intersection Types (Refinement Types)

- `[:and BaseType Predicate1 Predicate2 ...]`
  - Represents a value that must conform to `BaseType` _and_ satisfy all predicates.
  - `Predicate`: `[PredicateName Arg1 Arg2 ...]`, where `PredicateName` is a function symbol/keyword and `Args` are literals.
  - Example: `[:and :int [:> 0] [:< 100]]` (An integer greater than 0 and less than 100).

### 11.7. Literal Types

- `[:val LiteralValue]`
  - Represents a type inhabited by only a single literal value.
  - Example: `[:val :success]`, `[:val 42]`

### 11.8. Enum Types (Syntactic Sugar)

- `[:enum Val1 Val2 ...]`
  - Syntactic sugar for `[:union [:val Val1] [:val Val2] ...]`, where `Val`s are typically keywords or strings.
  - Example: `[:enum :pending :running :completed]`

### 11.9. Standard Type Aliases (Conceptual)

While RTFS might not have a formal `deftype` mechanism initially, certain common patterns can be treated as standard aliases in documentation and potentially by tooling:

- **`ErrorMap`**: Represents the standard error structure.
  - Alias for: `[:map [:type :keyword] [:message :string] [:details :map?]]`
- **`Result<T>`**: Represents a value that can be either a success (`[:ok T]`) or a standard error (`[:error ErrorMap]`).
  - Alias for: `[:union [:tuple [:val :ok] T] [:tuple [:val :error] ErrorMap]]`
  - This uses tuples with literal types (`[:val :ok]`, `[:val :error]`) as tags, which is a common pattern in functional languages and aligns well with `match`.

Using these aliases helps standardize how functions and tools signal success or failure.

## 12. Type Compatibility & Subtyping

- **Primitive Types:** Subtyping follows the natural hierarchy (e.g., `:int` is a subtype of `:float` if implicit conversion is allowed).
- **Union Types:** `T1` is a subtype of `[:union T1 T2 ...]`.
- **Intersection Types:** `[:and T1 T2 ...]` is a subtype of each `Ti`.
- **Function Types:** Covariant in return type, contravariant in parameter types.
- **Collections:**
  - `[:vector T1 S1?]` is a subtype of `[:vector T2 S2?]` if `T1` is a subtype of `T2` and `S1` is compatible with `S2` (see Shape Compatibility below).
  - `[:array T1 Shape1]` is a subtype of `[:array T2 Shape2]` if `T1` is a subtype of `T2` and `Shape1` is compatible with `Shape2` (see Shape Compatibility below).
  - Map subtyping is structural (width and depth subtyping): A map type `M1` is a subtype of `M2` if `M1` includes all required keys of `M2` with compatible value types, and if `M2` has a wildcard, `M1`'s wildcard (or implicit extra keys) must be compatible.
  - Tuple subtyping is based on element types at corresponding positions.

## 13. Shape Compatibility (for Vectors and Arrays)

Shape information in `[:vector T [Dim]]` and `[:array T [Dim1 Dim2 ...]]` allows for optional static checking of dimensions.

- **Rank:** The number of dimensions in a shape.
- **Dimension Value:** An integer size or `?` (unknown).

Two shapes, `Shape1` and `Shape2`, are considered **compatible** if:

1.  They have the same rank (same number of dimensions).
2.  For each corresponding dimension `d1` in `Shape1` and `d2` in `Shape2`:
    - If both `d1` and `d2` are integers, they must be equal (`d1 == d2`).
    - If either `d1` or `d2` (or both) is `?`, they are considered compatible at that dimension.

**Example:**

- `[100 100 3]` is compatible with `[100 100 3]`.
- `[100 ? 3]` is compatible with `[100 100 3]`.
- `[? ? ?]` is compatible with `[100 100 3]`.
- `[100 100 3]` is **not** compatible with `[100 50 3]`.
- `[100 100]` is **not** compatible with `[100 100 3]` (different rank).

**Level of Checking (Implementation Defined):**

- The RTFS specification defines the syntax and semantics for dimensioned types.
- However, the _extent_ to which a specific RTFS implementation (type checker, compiler, runtime) performs static shape checking is **implementation-defined and potentially configurable**.
- **Baseline Recommendation:** Implementations _should_ at least check for rank compatibility when shapes are provided for both types being compared (e.g., in assignments, function arguments).
- **Optional Strict Checking:** Implementations _may_ offer stricter modes that perform full compatibility checks, including matching integer dimensions, and report mismatches as static errors.
- **Runtime Behavior:** Even if static checking is minimal, runtimes may still perform dimension checks before executing operations like matrix multiplication, raising runtime errors if shapes are incompatible.

This approach allows the type system to express shape information for clarity and potential static analysis, without mandating complex, potentially undecidable static shape inference or dependent types in all implementations.

## 14. Type Alias Definition

While RTFS uses structural typing for compatibility checking, it supports **type aliases** to create named types that improve code readability and enable better error messages.

### 14.1. Basic Type Alias Syntax

Type aliases are defined using the `deftype` special form:

```acl
(deftype AliasName TypeExpression)
```

**Examples:**

```acl
;; Simple aliases for primitive types
(deftype UserId :int)
(deftype EmailAddress :string)

;; Alias for a user map type
(deftype User [:map [:name :string] [:age :int?]])

;; Alias for a complex nested type
(deftype ApiResponse [:map
                      [:status [:enum :success :error :pending]]
                      [:data :any]
                      [:metadata [:map [:timestamp :string] [:version :string]]?]])

;; Alias for a function type
(deftype UserValidator [:=> [User] :bool])

;; Alias with refinement constraints
(deftype PositiveInt [:and :int [:> 0]])
(deftype ValidEmail [:and :string [:string-contains "@"] [:string-min-length 5]])
```

### 14.2. Using Type Aliases

Once defined, type aliases can be used anywhere a type expression is expected:

```acl
;; In variable definitions
(def current-user User {:name "Alice" :age 30})

;; In function parameters and return types
(defn process-user [user User] User
  (assoc user :processed true))

;; In let bindings
(let [response ApiResponse (api:fetch-data)]
  (:data response))

;; In complex type expressions
(deftype UserList [:vector User])
(deftype UserDatabase [:map [:* User]])  ;; Map with any keyword keys, User values
```

### 14.3. Scoping and Modules

Type aliases follow the same scoping rules as other definitions:

```acl
(module my.domain
  :exports [User process-user]  ;; Export both the type alias and function

  (deftype User [:map [:name :string] [:age :int?]])  ;; Private to module
  (deftype InternalId :int)  ;; Private to module

  (defn process-user [user User] User
    (assoc user :id (generate-id))))

;; In another module
(import my.domain :as domain)

(def user domain/User {:name "Bob" :age 25})  ;; Using qualified type name
(domain/process-user user)
```

### 14.4. Type Alias Resolution

- **Structural Compatibility**: Type aliases are **transparent** during type checking. `User` and `[:map [:name :string] [:age :int?]]` are considered identical types.
- **Error Messages**: The type checker should prefer showing the alias name in error messages when possible for better readability.
- **Recursive Types**: Simple recursive type aliases are supported for self-referential data structures:

```acl
;; Tree structure
(deftype TreeNode [:map
                   [:value :any]
                   [:children [:vector TreeNode]]?])

;; Linked list
(deftype LinkedList [:union
                     :nil
                     [:map [:head :any] [:tail LinkedList]]])
```

### 14.5. Standard Library Type Aliases

The RTFS standard library provides common type aliases:

```acl
;; Result types for error handling
(deftype ErrorMap [:map [:type :keyword] [:message :string] [:details :any]?])
(deftype Result<T> [:union
                    [:map [:ok T]]
                    [:map [:error ErrorMap]]])

;; Common collection types
(deftype StringMap [:map [:* :string]])
(deftype IntVector [:vector :int])
(deftype AnyMap [:map [:* :any]])
```

**Answer to the specific question:**

To define a type alias for `[:map [:name :string] [:age :int?]]`, you would write:

```acl
(deftype User [:map [:name :string] [:age :int?]])
```

Then use it in your code:

```acl
(def alice User {:name "Alice" :age 30})
(def bob User {:name "Bob"})  ;; age is optional

(defn greet-user [user User] :string
  (str "Hello, " (:name user) "!"))
```
