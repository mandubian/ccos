# Plan Generation Strategy

## Multi-Step Approach

1. **Break Down the Goal**: Decompose complex intents into logical steps
2. **Name Steps Descriptively**: Use clear, action-oriented names in quotes
3. **Sequence Steps Logically**: Order steps by dependencies
4. **Minimize Steps**: But don't compromise clarity
5. **Use Appropriate Control Flow**: `if` for binary choices, `match` for multiple options

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
