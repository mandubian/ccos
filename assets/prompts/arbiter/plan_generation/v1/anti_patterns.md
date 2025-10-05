# Anti-Patterns

## Output Format Violations

❌ **JSON output**
```json
{"plan": "greet_user", "steps": [...]}
```

❌ **Markdown fences**
```markdown
```lisp
(plan ...)
` ``
```

❌ **Prose or explanations**
```
Here's a plan to greet the user:
(plan ...)
This plan will ask for their name...
```

✅ **CORRECT** - Raw RTFS only:
```lisp
(plan
  :name "greet_user"
  :language rtfs20
  :body (do ...))
```

## Variable Scoping Violations

❌ **Variables across step boundaries**
```lisp
(do
  (step "Get" (let [n (call :ccos.user.ask "Name?")] n))
  (step "Use" (call :ccos.echo {:message n})))  ; n not in scope!
```

✅ **CORRECT** - All in one step:
```lisp
(do
  (step "Get and Use"
    (let [n (call :ccos.user.ask "Name?")]
      (call :ccos.echo {:message n}))))
```

## Let Binding Violations

❌ **Let without body**
```lisp
(step "Bad" (let [x (call :ccos.user.ask "X?")]))  ; Missing body!
```

❌ **Empty let body**
```lisp
(step "Bad" (let [x 5]))  ; No expression after bindings!
```

✅ **CORRECT** - Let with body:
```lisp
(step "Good" 
  (let [x (call :ccos.user.ask "X?")] 
    (call :ccos.echo {:message x})))
```

## Return Value Violations

❌ **No structured return**
```lisp
(step "Collect" 
  (let [name (call :ccos.user.ask "Name?")]
    (call :ccos.echo {:message name})))  ; Returns echo result, not structured data!
```

✅ **CORRECT** - Return structured map:
```lisp
(step "Collect" 
  (let [name (call :ccos.user.ask "Name?")]
    (call :ccos.echo {:message name})
    {:user/name name}))  ; Explicit structured return
```

## Capability Violations

❌ **Capabilities not in whitelist**
```lisp
(call :ccos.file.read "data.txt")  ; Not in whitelist!
```

❌ **Missing colon prefix**
```lisp
(call ccos.echo {:message "hi"})  ; Must be :ccos.echo
```

❌ **Wrong signature**
```lisp
(call :ccos.math.add {:a 5 :b 3})  ; Should be positional: (call :ccos.math.add 5 3)
```

✅ **CORRECT** - Whitelisted with proper signature:
```lisp
(call :ccos.echo {:message "hi"})
(call :ccos.math.add 5 3)
```

## Structure Violations

❌ **Multiple (do ...) blocks**
```lisp
(plan
  :body (do ...)
  :body (do ...))  ; Only one :body allowed!
```

❌ **Missing (plan ...) wrapper**
```lisp
(do
  (step "X" ...))  ; Must be wrapped in (plan ...)
```

❌ **Unused variables or dangling references**
```lisp
(let [x 5 y 10]
  (call :ccos.echo {:message (str x)}))  ; y is unused
```

✅ **CORRECT** - Clean structure:
```lisp
(plan
  :name "clean"
  :language rtfs20
  :body (do
    (step "Action" 
      (let [x 5]
        (call :ccos.echo {:message (str x)})))))
```