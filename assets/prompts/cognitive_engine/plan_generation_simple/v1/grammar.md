# Simplified RTFS Grammar (Final Attempt Only)

## Basic Forms Only

```lisp
(do <step> <step> ...)
(step "Descriptive Name" (call :capability <args>))
(call :ccos.echo {:message "text"})
(call :ccos.user.ask "question text")
```

## No Complex Features

❌ Do NOT use:
- `let` bindings
- `if` conditionals
- `match` expressions
- Complex nested expressions
- Multiple capabilities in one step

## Available Capabilities (Limited)

- `:ccos.echo` - Takes a map with `:message` key
- `:ccos.user.ask` - Takes a string question

## Examples

Simple echo:
```lisp
(do
  (step "Print Message" (call :ccos.echo {:message "Hello"})))
```

Simple question:
```lisp
(do
  (step "Ask Name" (call :ccos.user.ask "What is your name?")))
```

Multiple simple steps:
```lisp
(do
  (step "Welcome" (call :ccos.echo {:message "Welcome!"}))
  (step "Ask Question" (call :ccos.user.ask "How can I help?")))
```
