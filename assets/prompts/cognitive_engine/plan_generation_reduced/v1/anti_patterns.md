# Plan Generation Anti-Patterns

## ❌ WRONG: Let has no body

```lisp
(step "Bad" 
  (let [name (call :ccos.user.ask "Name?")]))  ; ERROR: missing body!
```

**Why**: `let` requires a body expression that uses the bound variables.

## ❌ WRONG: Variables out of scope across steps

```lisp
(do
  (step "Get Name" 
    (let [name (call :ccos.user.ask "Name?")] 
      name))
  (step "Use Name" 
    (call :ccos.echo {:message name})))  ; ERROR: name not in scope!
```

**Why**: Variables defined in one step are not available in other steps.

**Fix**: Put both operations in the same step:
```lisp
(do
  (step "Get and Use Name"
    (let [name (call :ccos.user.ask "Name?")]
      (call :ccos.echo {:message name}))))
```

## ❌ WRONG: Invalid capability call (map args for math.add)

```lisp
(call :ccos.math.add {:a 2 :b 3})  ; ERROR: math.add takes positional args
```

**Fix**:
```lisp
(call :ccos.math.add 2 3)
```

## ❌ WRONG: Missing :message key for echo

```lisp
(call :ccos.echo "hello")  ; ERROR: echo requires map with :message key
```

**Fix**:
```lisp
(call :ccos.echo {:message "hello"})
```

## ❌ WRONG: Inventing capabilities

```lisp
(call :ccos.file.read "data.txt")  ; ERROR: capability not in whitelist
```

**Why**: Only use capabilities from the whitelist: `:ccos.echo`, `:ccos.math.add`, `:ccos.user.ask`

## ❌ WRONG: Including plan wrapper in reduced mode

```lisp
(plan
  :body (do
    (step "Echo" (call :ccos.echo {:message "hi"}))))
```

**Why**: Reduced mode requires ONLY the `(do ...)` body, no wrapper.

**Fix**:
```lisp
(do
  (step "Echo" (call :ccos.echo {:message "hi"})))
```
