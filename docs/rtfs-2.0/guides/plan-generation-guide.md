# RTFS 2.0 Plan Generation Guide

This guide provides a reference for generating **RTFS** (Reason about The Functional Spec) plans, specifically refined for LLMs and autonomous agents to maintain auditability and security.

## 🎯 Core Rules

### 1. Plan Structure
Plans are wrapped in a `(plan ...)` metadata block. The logic resides in the `:body` using a `(do ...)` form containing one or more `(step ...)` expressions.

```clojure
(plan
  :name "descriptive_name"
  :language rtfs20
  :body (do
    (step "Find User" 
      (call "db.query" {:name "Alice"}))
    (step "Notify" 
      (call "email.send" {:to "alice@example.com" :msg "Found you!"})))
  :annotations {:author "Agent-X"})
```

### 2. Variable Scoping ⚠️ CRITICAL
**`let` bindings are LOCAL to a single step.** Variables defined in a step **cannot** cross step boundaries.

```clojure
;; ✅ CORRECT - usage is within the same step
(step "Action"
  (let [x (call "ccos.user.ask" "X?")]
    (call "ccos.echo" {:message x})))

;; ❌ WRONG - variables across steps will fail
(step "Get" (let [x (call "ccos.user.ask" "X?")] x))
(step "Use" (call "ccos.echo" {:message x}))  ; Error: 'x' is not in scope!
```

### 3. Structured Returns
The final expression in a plan should return a **Map** with keyword keys. This allows CCOS to extract context and pass it to downstream intents.

```clojure
(step "Collect Data"
  (let [name (call "ccos.user.ask" "Name?")
        age (call "ccos.user.ask" "Age?")]
    {:user/name name :user/age age}))
```

### 4. Let Binding Body
A `let` expression must always have a body expression following the bindings.

```clojure
;; ❌ WRONG - missing body
(let [x 5])

;; ✅ CORRECT
(let [x 5]
  (call "ccos.echo" (str "Value: " x)))
```

## 🔧 Commonly Used Capabilities

| Capability | Signature Example | Returns |
|------------|-----------|---------|
| `ccos.echo` | `(call "ccos.echo" {:message "text"})` | `nil` |
| `ccos.user.ask` | `(call "ccos.user.ask" "prompt")` | `String` |
| `ccos.math.add` | `(call "ccos.math.add" 10 20)` | `Number` |

*Note: Capability names can be passed as strings or keywords.*

## 📝 Common Patterns

### Prompt + Echo + Return
```clojure
(step "Greet"
  (let [name (call "ccos.user.ask" "Name?")]
    (call "ccos.echo" (str "Hello, " name))
    {:user/name name}))
```

### Sequential Data Collection
Using sequential bindings in a single `let` is the cleanest way to collect multiple inputs.
```clojure
(step "Survey"
  (let [name (call "ccos.user.ask" "Name?")
        age (call "ccos.user.ask" "Age?")
        hobby (call "ccos.user.ask" "Hobby?")]
    {:user/name name :user/age age :user/hobby hobby}))
```

### Conditional Logic
```clojure
(step "Check Choice"
  (let [answer (call "ccos.user.ask" "Proceed? (yes/no)")]
    (if (= answer "yes")
      (call "ccos.echo" "Proceeding...")
      (call "ccos.echo" "Aborting."))))
```
