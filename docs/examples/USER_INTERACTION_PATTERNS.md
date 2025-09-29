# CCOS User Interaction Patterns

This document demonstrates progressive patterns of human-in-the-loop interaction with CCOS, from simple input prompts to complex intent-driven dialog systems.

## Overview

The `ccos.user.ask` capability enables CCOS plans to interact with end-users in real-time, creating dynamic, conversational workflows where the system adapts based on user responses.

## Capability: ccos.user.ask

**Signature:**
```clojure
(call :ccos.user.ask prompt-string [optional-default])
```

**Arguments:**
- `prompt-string`: The question or prompt to display to the user
- `optional-default`: (Optional) Default value if user just hits Enter

**Returns:** String value with the user's response

**Security:** Requires `:io` and `:user-interaction` permissions

## Pattern 1: Basic User Input

**Goal:** Ask a single question and use the answer.

**Example Intent:**
```
"ask the user for their name and greet them personally"
```

**Generated Plan (conceptual):**
```clojure
(do
  (step "Ask Name" 
    (let [name (call :ccos.user.ask "What is your name? ")]
      (call :ccos.echo {:message (str "Hello, " name "! Nice to meet you.")})))
)
```

**Use Cases:**
- Simple data collection
- Configuration prompts
- User confirmation dialogs

**Example:** See `examples/user_interaction_basic.rs`

**Note:** While the stub arbiter can generate simple plans, **delegation is highly recommended** for conversational user interactions. The stub arbiter uses predetermined patterns that may not properly handle dynamic user input flows. Enable delegation for best results:

```bash
export CCOS_ENABLE_DELEGATION=1
export OPENAI_API_KEY=your_key
cargo run --example user_interaction_basic
```

## Pattern 2: Multi-Step Form / Survey

**Goal:** Collect multiple pieces of information sequentially.

**Example Intent:**
```
"conduct a mini survey: ask for name, age, and hobby, then summarize"
```

**Generated Plan (conceptual):**
```clojure
(do
  (step "Collect Name"
    (let [name (call :ccos.user.ask "Your name: ")]
      name))
  (step "Collect Age"
    (let [age (call :ccos.user.ask "Your age: ")]
      age))
  (step "Collect Hobby"
    (let [hobby (call :ccos.user.ask "Your hobby: ")]
      hobby))
  (step "Summarize"
    (call :ccos.echo {:message "Thank you for sharing!"}))
)
```

**Key Features:**
- Sequential data collection
- State accumulation across steps
- Summary/confirmation at end

**Intent Graph:** Linear chain of user input intents

## Pattern 3: Dynamic Intent Graph Building

**Goal:** Create new intents based on user responses.

**Example Intent:**
```
"ask the user if they want to plan a trip, and if yes, ask for destination and dates"
```

**Flow:**
1. Initial intent: "gather travel interest"
2. User says "yes" → Spawn child intent: "plan trip to [destination]"
3. Child intent asks for dates → Spawn another child: "book hotels for [dates]"

**Intent Graph Structure:**
```
root-intent (gather interest)
  ├─→ child-intent-1 (plan trip)
  │     ├─→ child-intent-2 (book hotels)
  │     └─→ child-intent-3 (book flights)
  └─→ alternate (user declined)
```

**Key Features:**
- Branching based on user input
- Dynamic intent creation
- Parent-child intent relationships
- State passed between intents

**Status:** To be implemented in example (TODO)

## Pattern 4: Intent Re-evaluation & Recursion

**Goal:** Plans that analyze user input and decide to create entirely new intents, potentially triggering new planning cycles.

**Example Intent:**
```
"have a conversation with the user about their project needs and create appropriate work intents"
```

**Flow:**
1. Initial intent: "understand project needs"
2. Plan asks: "What type of project?" → User: "web app"
3. Plan *creates child intent*: "design web app architecture"
4. Arbiter generates *new plan* for child intent
5. New plan asks more specific questions about the web app
6. Based on answers, creates *grandchild intents*: "setup database", "create API", "build frontend"

**Intent Graph Structure:**
```
root (understand needs)
  └─→ child (design architecture)
        ├─→ grandchild-1 (setup database)
        ├─→ grandchild-2 (create API)
        └─→ grandchild-3 (build frontend)
              └─→ great-grandchild (implement component X)
```

**Key Features:**
- Recursive intent spawning
- Multi-level intent hierarchies
- Adaptive planning based on accumulated context
- Causal chain tracking across intent generations

**Status:** To be implemented (TODO)

## Implementation Notes

### Capability Registration

The `ccos.user.ask` capability is registered in `stdlib.rs`:
- Prompts on **stdout** with clear ❓ emoji prefix for visibility
- Auto-adds spacing for better formatting
- Flushes output to ensure prompts appear immediately
- Reads from stdin
- Supports optional default values
- Returns trimmed string response

### Security Considerations

User interaction capabilities require:
- `SecurityLevel::Controlled` or higher
- Explicit capability allowlist including "ccos.user.ask"
- Effects: `:io`, `:user-interaction`

### LLM Arbiter Integration

The capability is documented in LLM prompts (`llm_provider.rs`):
- Clear signature and examples
- Integrated into capability whitelist
- Usage patterns explained to the model

### Testing

Interactive examples require:
- TTY (terminal) environment
- User present to respond to prompts
- Enable delegation for LLM-generated plans

**Environment-based configuration:**
```bash
export CCOS_ENABLE_DELEGATION=1
export OPENAI_API_KEY=your_key
export CCOS_DELEGATING_MODEL=gpt-4o-mini
cargo run --example user_interaction_basic
```

**CLI-based configuration:**
```bash
cargo run --example user_interaction_basic -- \
  --enable-delegation \
  --llm-provider openai \
  --llm-model gpt-4o-mini \
  --llm-api-key $OPENAI_API_KEY
```

**Config file (JSON/TOML with profiles and model sets):**
```bash
cargo run --example user_interaction_basic -- --config path/to/agent_config.json
```

**Auto-select model by budget:**
```bash
cargo run --example user_interaction_basic -- \
  --config path/to/agent_config.json \
  --model-auto-prompt-budget 0.001 \
  --model-auto-completion-budget 0.003
```

**See what's happening behind the scenes:**
```bash
cargo run --example user_interaction_basic -- --verbose
# Shows: Intent analysis → LLM delegation → Plan compilation → Execution
# Great for understanding the CCOS workflow!

cargo run --example user_interaction_basic -- --enable-delegation --verbose
# Full visibility into LLM-based plan generation
```

**Debug mode (shows LLM prompts):**
```bash
cargo run --example user_interaction_basic -- --debug
```

**Offline mode (stub arbiter, no LLM):**
```bash
cargo run --example user_interaction_basic
# Or explicitly:
cargo run --example user_interaction_basic -- \
  --llm-provider stub \
  --llm-model deterministic-stub-model
```

## Future Enhancements

1. **Validation:** Add schema validation for expected response types
2. **Retries:** Auto-retry on invalid input with helpful error messages
3. **UI Integration:** Alternative input sources (GUI, web forms, etc.)
4. **Async Prompts:** Non-blocking prompts for concurrent workflows
5. **Rich Prompts:** Support for multiple-choice, checkboxes, etc.

## Examples

- **Basic:** `cargo run --example user_interaction_basic`
- **Graph (TODO):** `cargo run --example user_interaction_graph`
- **Recursive (TODO):** `cargo run --example user_interaction_recursive`

## See Also

- [CCOS Architecture](../ccos/specs/000-ccos-architecture-new.md)
- [Intent Graph](../ccos/specs/001-intent-graph-and-dependencies.md)
- [Capabilities](../ccos/specs/004-capabilities-and-marketplace.md)
- [Live Interactive Assistant](../../rtfs_compiler/examples/live_interactive_assistant.rs)
