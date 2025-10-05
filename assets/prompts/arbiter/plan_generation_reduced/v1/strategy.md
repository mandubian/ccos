# Plan Generation Strategy

## Plan Structure
1. **Create Plan Wrapper**: Use `(plan :name "..." :language rtfs20 :body (do ...) :annotations {...})`
2. **Break Down the Goal**: Decompose complex intents into logical steps within the :body
3. **Name Steps Descriptively**: Use clear, action-oriented names in quotes
4. **Sequence Steps Logically**: Order steps by dependencies
5. **Minimize Steps**: But don't compromise clarity
6. **Use Appropriate Control Flow**: `if` for binary choices, `match` for multiple options
7. **Return Values for Reuse**: Include a final expression in the :body to return structured data (captured variables, maps, or computed results) that can be used by other plans or the calling system

## Variable Scoping Rules

**CRITICAL**: Let bindings are LOCAL to a single step. Variables do NOT cross step boundaries.

### When to Use `let`

- Capturing user input for reuse within the same step
- Computing intermediate values used multiple times in one step
- Composing complex expressions from simpler parts

### Step Boundaries

Each `(step ...)` creates a new scope. Variables defined in one step are NOT available in subsequent steps.

## Capability Selection

- **Printing/Logging**: Use `:ccos.echo`
- **Math Operations**: Use `:ccos.math.add`
- **User Input**: Use `:ccos.user.ask`
- **Do NOT** invent capability names - use only those from the whitelist
