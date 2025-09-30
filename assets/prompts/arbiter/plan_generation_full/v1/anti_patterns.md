# Full Plan Anti-Patterns

## ❌ WRONG: Let has no body

```lisp
(plan
  :body (do
    (step "Bad" 
      (let [name (call :ccos.user.ask "Name?")]))))  ; Missing body expression!
```

**Why**: `let` requires a body expression after the bindings.

## ❌ WRONG: Variables out of scope across steps

```lisp
(plan
  :body (do
    (step "Get" 
      (let [n (call :ccos.user.ask "Name?")] n))
    (step "Use" 
      (call :ccos.echo {:message n}))))  ; n not in scope here!
```

**Fix**: Combine into one step:
```lisp
(plan
  :body (do
    (step "Get and Use"
      (let [n (call :ccos.user.ask "Name?")]
        (call :ccos.echo {:message n})))))
```

## ❌ WRONG: Including kernel-managed fields

```lisp
(plan
  :plan_id "123"                    ; Will be ignored/overwritten
  :status "pending"                 ; Will be ignored/overwritten
  :capabilities_required [:ccos.echo]  ; Will be ignored/overwritten
  :body (do ...))
```

**Why**: These fields are managed by the kernel and will be overwritten.

**Fix**: Omit them entirely:
```lisp
(plan
  :name "my_plan"
  :body (do ...))
```

## ❌ WRONG: Invalid capability call

```lisp
(plan
  :body (do
    (step "Add" 
      (call :ccos.math.add {:a 2 :b 3}))))  ; math.add takes positional args!
```

**Fix**:
```lisp
(plan
  :body (do
    (step "Add" 
      (call :ccos.math.add 2 3))))
```

## ❌ WRONG: Missing :message for echo

```lisp
(plan
  :body (do
    (step "Print" 
      (call :ccos.echo "hello"))))  ; echo requires {:message "..."}
```

**Fix**:
```lisp
(plan
  :body (do
    (step "Print" 
      (call :ccos.echo {:message "hello"}))))
```

## ❌ WRONG: Complex annotation values

```lisp
(plan
  :annotations {:priority 1 :tags ["user" "admin"]})  ; Must be strings!
```

**Fix**:
```lisp
(plan
  :annotations {:priority "high" :tags "user,admin"})
```

## ❌ WRONG: Using non-whitelisted capabilities

```lisp
(plan
  :body (do
    (step "Read" 
      (call :ccos.file.read "data.txt"))))  ; Not in whitelist!
```

**Why**: Only `:ccos.echo`, `:ccos.math.add`, `:ccos.user.ask` are allowed.
