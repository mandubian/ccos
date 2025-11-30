# Full Plan RTFS Grammar

## Plan Structure

```lisp
(plan
  :name "descriptive_name"          ; optional
  :language rtfs20                  ; optional
  :body (do <step> <step> ...)      ; required
  :annotations {:key "val"}         ; optional
)
```

## Reduced Step/Call Grammar (inside :body)

```lisp
(step "Descriptive Name" (<expr>))
(call :cap.namespace.op <args...>)
(if <condition> <then> <else>)      ; for binary yes/no choices
(match <value> <pattern1> <result1> <pattern2> <result2> ...)  ; for multiple choices
(let [var1 expr1 var2 expr2] <body>)
(str <arg1> <arg2> ...)
(= <arg1> <arg2>)
```

## Allowed Arguments

- **Strings**: `"..."`
- **Numbers**: `1 2 3`
- **Simple maps** with keyword keys: `{:key "value"}`

## Capability Whitelist

Use ONLY these capability ids in `:body`:
- `:ccos.echo` - for printing/logging messages
- `:ccos.math.add` - for adding numbers
- `:ccos.user.ask` - for prompting user for input

Do NOT invent or use other capability ids.

## Capability Signatures (STRICT)

### :ccos.echo
- Must be called with a single map `{:message "..."}`
- Example: `(call :ccos.echo {:message "hello"})`

### :ccos.math.add
- Must be called with exactly two positional numbers
- Example: `(call :ccos.math.add 2 3)`
- Map arguments are NOT allowed for this capability

### :ccos.user.ask
- Must be called with 1-2 string arguments: prompt, optional default
- Returns user's string response
- **Important**: To capture and reuse the response, use `(let ...)` with BOTH the prompt AND the action IN THE SAME STEP. Let bindings do NOT cross step boundaries!


