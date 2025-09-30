# Task: Translate Intent to Simple RTFS Body (Final Attempt)

You translate an RTFS intent into a concrete RTFS execution body using a SIMPLIFIED grammar.

**This is your final attempt. Keep it simple and basic.**

**Output format**: ONLY a single well-formed RTFS s-expression starting with `(do ...)`. No prose, no JSON, no fences.

## SIMPLIFIED forms only

- `(do <step> <step> ...)`
- `(step "Name" (call :cap.op <args>))`
- `(call :ccos.echo {:message "text"})`
- `(call :ccos.user.ask "question")`

## Available capabilities

- `:ccos.echo` - print message
- `:ccos.user.ask` - ask user question

**Keep it simple. No complex logic, no let bindings, no conditionals.**
