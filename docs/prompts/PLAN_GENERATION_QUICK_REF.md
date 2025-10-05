# Plan Generation Quick Reference

**Active Prompts**: `assets/prompts/arbiter/plan_generation/v1/`

## 🎯 Core Rules

### 1. Plan Structure
```lisp
(plan
  :name "descriptive_name"
  :language rtfs20
  :body (do
    (step "Step Name" <expr>)
    ...
  )
  :annotations {:key "value"}
)
```

### 2. Variable Scoping ⚠️ CRITICAL
```lisp
✅ CORRECT - all in one step:
(step "Action"
  (let [x (call :ccos.user.ask "X?")]
    (call :ccos.echo {:message x})))

❌ WRONG - variables across steps:
(step "Get" (let [x (call :ccos.user.ask "X?")] x))
(step "Use" (call :ccos.echo {:message x}))  ; x not in scope!
```

**Rule**: `let` bindings are LOCAL to a single step. They CANNOT cross step boundaries.

### 3. Structured Returns
```lisp
✅ Final step returns map:
(step "Collect"
  (let [name (call :ccos.user.ask "Name?")
        age (call :ccos.user.ask "Age?")]
    {:user/name name :user/age age}))
```

**Rule**: Final step should return a map with keyword keys for downstream reuse.

### 4. Let Binding Body
```lisp
❌ WRONG - no body:
(let [x 5])

✅ CORRECT - with body:
(let [x 5]
  (call :ccos.echo {:message (str x)}))
```

**Rule**: `let` must always have a body expression after the bindings.

## 🔧 Available Capabilities

| Capability | Signature | Returns |
|------------|-----------|---------|
| `:ccos.echo` | `(call :ccos.echo {:message "text"})` | nil |
| `:ccos.user.ask` | `(call :ccos.user.ask "prompt")` | String |
| `:ccos.math.add` | `(call :ccos.math.add num1 num2)` | Number |
| `:ccos.math.subtract` | `(call :ccos.math.subtract num1 num2)` | Number |
| `:ccos.math.multiply` | `(call :ccos.math.multiply num1 num2)` | Number |
| `:ccos.math.divide` | `(call :ccos.math.divide num1 num2)` | Number |

## 📝 Common Patterns

### Single Prompt
```lisp
(step "Get Name"
  (call :ccos.user.ask "What is your name?"))
```

### Prompt + Echo + Return
```lisp
(step "Greet"
  (let [name (call :ccos.user.ask "Name?")]
    (call :ccos.echo {:message (str "Hello, " name)})
    {:user/name name}))
```

### Multiple Prompts
```lisp
(step "Survey"
  (let [name (call :ccos.user.ask "Name?")
        age (call :ccos.user.ask "Age?")
        hobby (call :ccos.user.ask "Hobby?")]
    {:user/name name :user/age age :user/hobby hobby}))
```

### Conditional (if)
```lisp
(step "Check"
  (let [answer (call :ccos.user.ask "Yes or no?")]
    (if (= answer "yes")
      (call :ccos.echo {:message "Affirmative"})
      (call :ccos.echo {:message "Negative"}))
    {:answer answer}))
```

### Multiple Choice (match)
```lisp
(step "Choose"
  (let [lang (call :ccos.user.ask "rust, python, or javascript?")]
    (match lang
      "rust" (call :ccos.echo {:message "Rust chosen"})
      "python" (call :ccos.echo {:message "Python chosen"})
      "javascript" (call :ccos.echo {:message "JS chosen"})
      _ (call :ccos.echo {:message "Unknown"}))
    {:language lang}))
```

### Math Operation
```lisp
(step "Calculate"
  (let [result (call :ccos.math.add 5 3)]
    (call :ccos.echo {:message (str "Sum: " result)})
    {:result result}))
```

## 🚫 Anti-Patterns

| ❌ Don't | ✅ Do |
|---------|-------|
| Use variables across steps | Keep all related ops in one step |
| Forget let body | Always include body expression |
| Return raw values | Return structured maps |
| Use non-whitelisted capabilities | Use only listed capabilities |
| Output JSON/markdown | Output raw RTFS only |
| Use `(edge ...)` in plans | Use sequential `(do ...)` |
| Forget `:` prefix on capabilities | Always use `:ccos.echo` format |

## 🧪 Testing

```bash
# Build
cd rtfs_compiler && cargo build --example user_interaction_progressive_graph

# Run with delegation
cd rtfs_compiler && cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose

# Test specific goal
cd rtfs_compiler && cargo run --example user_interaction_progressive_graph -- --enable-delegation --verbose --goal "plan a trip to paris"
```

## 📚 Full Documentation

- **Detailed Guide**: `docs/prompts/PLAN_GENERATION_CONSOLIDATION.md`
- **Prompt Files**: `assets/prompts/arbiter/plan_generation/v1/`
  - `task.md` - Task definition
  - `grammar.md` - Complete grammar reference
  - `few_shots.md` - Examples (simple to complex)
  - `strategy.md` - Strategic guidance
  - `anti_patterns.md` - Common mistakes

## 🔄 Prompt Loading

The delegating arbiter loads prompts via:
```rust
self.prompt_manager.render("plan_generation", "v1", &vars)
```

This combines all `.md` files in the directory into a single prompt for the LLM.
