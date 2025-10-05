# Plan Generation Strategy

## Core Principles

1. **Identify the goal**: Understand what the intent is trying to achieve
2. **Minimize steps**: Use as few steps as possible - prefer combining related operations in a single step with `let` bindings
3. **Sequential execution**: Steps execute in order within the `(do ...)` block
4. **Capture and return**: Always return structured data (maps with keyword keys) from the final step for downstream reuse
5. **Scope awareness**: Remember that `let` bindings are LOCAL to a single step - plan accordingly

## Step-by-Step Approach

### 1. Analyze the Intent
- What information needs to be collected?
- What computations need to be performed?
- What should be returned for downstream use?

### 2. Design the Plan Structure
- **For simple tasks**: Use a single step with `let` bindings
- **For complex tasks**: Break into logical steps, but keep them minimal
- **For interactive tasks**: Collect all related user inputs in one step when possible

### 3. Handle Data Flow
- **Within a step**: Use `let` bindings to capture and reuse values
- **Across steps**: NOT POSSIBLE - each step is independent
- **CRITICAL**: All variables used in expressions must be defined in the same `let` binding
- **For final result**: Last expression in final step becomes the plan's return value

**Common mistake**: Referencing variables that aren't defined in the current `let` binding
```lisp
❌ WRONG - undefined variable:
(let [destination (call :ccos.user.ask "Where?")]
  (call :ccos.echo {:message (str "Going to " destination " for " duration " days")}))  ; duration not defined!

✅ CORRECT - all variables defined:
(let [destination (call :ccos.user.ask "Where?")
      duration (call :ccos.user.ask "How many days?")]
  (call :ccos.echo {:message (str "Going to " destination " for " duration " days")}))
```

### 4. Choose Control Flow
- **Binary choice**: Use `(if condition then else)`
- **Multiple choices**: Use `(match value pattern1 result1 pattern2 result2 ...)`
- **Sequential operations**: Chain them in `let` bindings

### 5. Return Structured Data
- Always return a map with keyword keys from the final step
- Use namespaced keywords for clarity: `:trip/destination`, `:user/name`
- Include all collected or computed values that might be useful downstream

## Common Patterns

### Pattern 1: Single Prompt with Echo
```lisp
(step "Action"
  (let [value (call :ccos.user.ask "prompt")]
    (call :ccos.echo {:message (str "You said: " value)})
    {:result/value value}))
```

### Pattern 2: Multiple Prompts with Summary
```lisp
(step "Collect Data"
  (let [a (call :ccos.user.ask "First?")
        b (call :ccos.user.ask "Second?")
        c (call :ccos.user.ask "Third?")]
    (call :ccos.echo {:message (str "Summary: " a ", " b ", " c)})
    {:data/a a :data/b b :data/c c}))
```

### Pattern 3: Computation with Result
```lisp
(step "Calculate"
  (let [result (call :ccos.math.add 5 3)]
    (call :ccos.echo {:message (str "Result: " result)})
    {:math/result result}))
```

### Pattern 4: Conditional Logic
```lisp
(step "Branch"
  (let [choice (call :ccos.user.ask "Yes or no?")]
    (if (= choice "yes")
      (call :ccos.echo {:message "Affirmative"})
      (call :ccos.echo {:message "Negative"}))
    {:choice/value choice}))
```

## Anti-Patterns to Avoid

❌ **Don't** try to use variables across step boundaries
❌ **Don't** create `let` bindings without a body expression
❌ **Don't** forget to return structured data from the final step
❌ **Don't** use capabilities not in the whitelist
❌ **Don't** output JSON, markdown, or prose - only RTFS s-expressions