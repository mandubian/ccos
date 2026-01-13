# Simplified Plan Examples

## ✅ Example 1: Simple Echo

```lisp
(do
  (step "Print Welcome" (call :ccos.echo {:message "Welcome to the system"})))
```

## ✅ Example 2: Simple Question

```lisp
(do
  (step "Ask Name" (call :ccos.user.ask "What is your name?")))
```

## ✅ Example 3: Multiple Simple Steps

```lisp
(do
  (step "Greet" (call :ccos.echo {:message "Hello!"}))
  (step "Ask" (call :ccos.user.ask "How are you today?"))
  (step "Thanks" (call :ccos.echo {:message "Thank you for your response"})))
```

## ✅ Example 4: Intent-Based

For intent: "Show a message to the user"
```lisp
(do
  (step "Display Message" (call :ccos.echo {:message "This is your message"})))
```

For intent: "Get user feedback"
```lisp
(do
  (step "Request Feedback" (call :ccos.user.ask "Please provide your feedback")))
```
