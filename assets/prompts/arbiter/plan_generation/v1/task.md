# Task: Translate Intent to RTFS Plan

You translate an RTFS intent into a concrete RTFS plan.

## Output Format

Return ONLY a single well-formed RTFS plan structure:

```lisp
(plan
  :name "descriptive_name"
  :language rtfs20
  :body (do
    <step>
    <step>
    ...
  )
  :annotations {:key "value"}
)
```

## Requirements

- **No prose, no JSON, no markdown fences** - just the raw RTFS s-expression
- Use ONLY the forms from the grammar section
- Keep plans minimal and focused
- Final step should return structured data (map) for reuse by downstream intents
- All capability IDs must start with a colon (`:ccos.echo`, `:ccos.user.ask`, etc.)

## Key Constraints

1. **Variable scoping**: `let` bindings are LOCAL to a single step - you CANNOT use variables across step boundaries
2. **Variable references**: ALL variables used in expressions must be defined in the same `let` binding
3. **Sequential execution**: Steps in `(do ...)` execute in order
4. **Structured results**: Final step should evaluate to a map with keyword keys capturing collected values
5. **Capability whitelist**: Only use capabilities from the provided list

## Critical Rule: Variable Scope

**NEVER reference a variable that isn't defined in the current `let` binding!**

❌ **WRONG** - undefined variable:
```lisp
(let [destination (call :ccos.user.ask "Where?")]
  (call :ccos.echo {:message (str "Going to " destination " for " duration " days")}))  ; ERROR: duration not defined!
```

✅ **CORRECT** - all variables defined:
```lisp
(let [destination (call :ccos.user.ask "Where?")
      duration (call :ccos.user.ask "How many days?")]
  (call :ccos.echo {:message (str "Going to " destination " for " duration " days")}))
```

## Important: Plan Independence

**Each plan execution is independent** - you cannot reference variables or results from previous plan executions. If you need multiple pieces of information, collect them all in a single plan using multiple `call :ccos.user.ask` operations within the same `let` binding.

❌ **WRONG** - trying to reference previous execution results:
```lisp
; This assumes 'duration', 'arrival', 'departure', 'budget' were set in a previous plan
(call :ccos.echo {:message (str "Planning your " duration "-day cultural trip to Paris from " arrival " to " departure " with " budget " budget")})
```

✅ **CORRECT** - collect all needed data in current plan:
```lisp
(let [destination (call :ccos.user.ask "Where would you like to travel?")
      duration (call :ccos.user.ask "How many days will you stay?")
      arrival (call :ccos.user.ask "What's your arrival date?")
      departure (call :ccos.user.ask "What's your departure date?")
      budget (call :ccos.user.ask "What's your total budget?")]
  (call :ccos.echo {:message (str "Planning your " duration "-day cultural trip to " destination " from " arrival " to " departure " with " budget " budget")})
  {:trip/destination destination :trip/duration duration :trip/arrival arrival :trip/departure departure :trip/budget budget})
```