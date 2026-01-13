# Simplified Plan Strategy (Final Attempt)

## Keep It Simple

This is your final attempt after previous failures. The key is **simplicity**.

## Approach

1. **One action per step** - Don't combine multiple operations
2. **Use only basic capabilities** - `:ccos.echo` and `:ccos.user.ask`
3. **No complex logic** - No conditionals, no pattern matching, no variables
4. **Direct and straightforward** - If in doubt, make it simpler

## When to Use What

- **Need to show something to user?** → Use `:ccos.echo`
- **Need to get input from user?** → Use `:ccos.user.ask`
- **Complex requirement?** → Break it into multiple simple steps

## Example Simplification

❌ Too complex:
```lisp
(step "Process" 
  (let [x (call :ccos.user.ask "Value?")]
    (if (= x "yes") 
      (call :ccos.echo {:message "Good"})
      (call :ccos.echo {:message "Bad"}))))
```

✅ Simplified:
```lisp
(step "Ask User" (call :ccos.user.ask "Do you want to proceed?"))
(step "Show Message" (call :ccos.echo {:message "Thank you for your input"}))
```
