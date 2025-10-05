# Reduced RTFS Plan Grammar

## Plan Structure
```lisp
(plan
  :name "descriptive_name"           ; optional but recommended
  :language rtfs20                   ; optional (will be set to rtfs20 if missing)
  :body (do <step> <step> ...)       ; required - contains the actual steps
  :annotations {:key "value"}        ; optional - metadata
)
```

## Allowed Forms (inside :body)

```lisp
(do <step> <step> ...)
(step "Descriptive Name" (<expr>))  ; name must be a double-quoted string
(call :cap.namespace.op <args...>)  ; capability ids MUST be RTFS keywords starting with a colon
(if <condition> <then> <else>)      ; conditional execution (use for binary yes/no)
(match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; pattern matching (use for multiple choices)
(let [var1 expr1 var2 expr2] <body>)  ; local bindings
(str <arg1> <arg2> ...)               ; string concatenation
(= <arg1> <arg2>)                     ; equality comparison
```

## Allowed Arguments

- **Strings**: `"..."`
- **Numbers**: `1 2 3`
- **Simple maps** with keyword keys: `{:key "value" :a 1 :b 2}`

## Capability Signatures (STRICT)

### :ccos.echo
- **Signature**: Must be called with a single map argument containing `:message` string
- **Example**: `(call :ccos.echo {:message "hello"})`

### :ccos.math.add
- **Signature**: Must be called with exactly two positional number arguments. Do NOT use map arguments for this capability.
- **Example**: `(call :ccos.math.add 2 3)`

### :ccos.user.ask
- **Signature**: Takes 1-2 string arguments: prompt, optional default
- **Returns**: String value with user's response
- **Important**: To capture and reuse the response, use `(let ...)` with BOTH the prompt AND the action that uses it IN THE SAME STEP. Let bindings do NOT cross step boundaries!
