# RTFS Plan Grammar

## Plan Structure

```lisp
(plan
  :name "descriptive_name"           ; optional but recommended
  :language rtfs20                   ; optional (defaults to rtfs20)
  :body (do <step> <step> ...)       ; required - contains the steps
  :annotations {:key "value"}        ; optional - metadata
)
```

## Allowed Forms (inside :body)

```lisp
(do <step> <step> ...)                                    ; sequential execution block
(step "Descriptive Name" (<expr>))                        ; named step (name must be quoted string)
(call :capability.namespace.op <args...>)                 ; capability invocation (ID must start with :)
(if <condition> <then> <else>)                            ; conditional (use for yes/no)
(match <value> <pattern1> <result1> <pattern2> <result2> ...) ; pattern matching (use for multiple choices)
(let [var1 expr1 var2 expr2 ...] <body>)                 ; local bindings within step
(str <arg1> <arg2> ...)                                   ; string concatenation
(= <arg1> <arg2>)                                         ; equality comparison
```

## Allowed Arguments

- **Strings**: `"..."`
- **Numbers**: `1`, `2`, `3.14`
- **Keywords**: `:key`, `:trip/dates`
- **Maps**: `{:key "value" :a 1 :b 2}`
- **Lists**: `[1 2 3]`, `["a" "b" "c"]`
- **Context Values**: Use actual values from previous plan executions directly (not placeholder syntax)

## Available Capabilities

- **`:ccos.echo`** - Print message to output
  - Signature: `(call :ccos.echo {:message "text"})`
  
- **`:ccos.user.ask`** - Prompt user for input
  - Signature: `(call :ccos.user.ask "prompt text")`
  - Returns: String value with user's response
  
- **`:ccos.math.add`** - Add two numbers
  - Signature: `(call :ccos.math.add num1 num2)`
  - Returns: Sum of the two numbers
  
- **`:ccos.math.subtract`** - Subtract two numbers
  - Signature: `(call :ccos.math.subtract num1 num2)`
  
- **`:ccos.math.multiply`** - Multiply two numbers
  - Signature: `(call :ccos.math.multiply num1 num2)`
  
- **`:ccos.math.divide`** - Divide two numbers
  - Signature: `(call :ccos.math.divide num1 num2)`

- **`"ccos.network.http-fetch"`** - Make HTTP requests
  - Map format: `(call "ccos.network.http-fetch" {:url "https://..." :method "GET" :headers {...} :body "..."})`
  - List format: `(call "ccos.network.http-fetch" :url "https://..." :method "GET" :headers {...} :body "...")`
  - Simple format: `(call "ccos.network.http-fetch" "https://...")`  ; for GET requests
  - Returns: `{:status 200 :body "..." :headers {...}}`

## Critical Rules

### Variable Scoping
**CRITICAL**: `let` bindings are LOCAL to a single step. Variables CANNOT cross step boundaries.

✅ **CORRECT** - capture and reuse within single step:
```lisp
(step "Greet User"
  (let [name (call :ccos.user.ask "What is your name?")]
    (call :ccos.echo {:message (str "Hello, " name "!")})))
```

✅ **CORRECT** - multiple variables in same let binding:
```lisp
(step "Plan Trip"
  (let [destination (call :ccos.user.ask "Where to?")
        duration (call :ccos.user.ask "How many days?")
        budget (call :ccos.user.ask "What's your budget?")]
    (call :ccos.echo {:message (str "Planning " duration "-day trip to " destination " with " budget " budget")})
    {:trip/destination destination :trip/duration duration :trip/budget budget}))
```

❌ **WRONG** - variables out of scope across steps:
```lisp
(step "Get" (let [n (call :ccos.user.ask "Name?")] n))
(step "Use" (call :ccos.echo {:message n}))  ; ERROR: n not in scope!
```

❌ **WRONG** - referencing undefined variables:
```lisp
(step "Plan Trip"
  (let [destination (call :ccos.user.ask "Where to?")]
    (call :ccos.echo {:message (str "Planning trip to " destination " for " duration " days")})  ; ERROR: duration not defined!
    {:trip/destination destination}))
```

### Structured Results
The **final step** should return a map capturing key values for downstream reuse:

✅ **CORRECT** - final step returns structured map:
```lisp
(step "Collect Trip Details"
  (let [dates (call :ccos.user.ask "What dates will you travel?")
        duration (call :ccos.user.ask "How many days?")
        interests (call :ccos.user.ask "What activities interest you?")]
    {:trip/dates dates
     :trip/duration duration
     :trip/interests interests}))
```

You may echo a human-readable summary in an earlier step, but the final step MUST evaluate to a structured map.

### Let Binding Body
❌ **WRONG** - let without body expression:
```lisp
(step "Bad" (let [name (call :ccos.user.ask "Name?")]))  ; Missing body!
```

✅ **CORRECT** - let with body:
```lisp
(step "Good" 
  (let [name (call :ccos.user.ask "Name?")] 
    (call :ccos.echo {:message name})))
```