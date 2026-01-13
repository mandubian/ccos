# Simplified Plan Anti-Patterns

## ❌ Do NOT Use Complex Features

### WRONG: Using let bindings
```lisp
(step "Bad" (let [x value] (call :ccos.echo {:message x})))
```
**Why**: This is a final simplified attempt - no variable bindings allowed.

### WRONG: Using conditionals
```lisp
(step "Bad" (if condition (call :ccos.echo {:message "yes"}) (call :ccos.echo {:message "no"})))
```
**Why**: This is too complex for final attempt - keep it simple.

### WRONG: Using match expressions
```lisp
(step "Bad" (match value "a" (call :ccos.echo {:message "A"}) "b" (call :ccos.echo {:message "B"})))
```
**Why**: Pattern matching is too complex - use simple sequential steps.

### WRONG: Multiple operations in one step
```lisp
(step "Bad" 
  (call :ccos.echo {:message "First"})
  (call :ccos.echo {:message "Second"}))
```
**Why**: One action per step only.

### WRONG: Using unavailable capabilities
```lisp
(step "Bad" (call :ccos.math.add 1 2))
```
**Why**: Only `:ccos.echo` and `:ccos.user.ask` are available in simplified mode.

## ✅ Correct Simplified Approach

Break everything into the simplest possible steps:
```lisp
(do
  (step "Step 1" (call :ccos.echo {:message "Message 1"}))
  (step "Step 2" (call :ccos.user.ask "Question?"))
  (step "Step 3" (call :ccos.echo {:message "Message 2"})))
```
