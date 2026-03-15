---
name: "fibonacci.worker.default"
description: "Deterministic background worker that computes Fibonacci numbers."
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "fibonacci.worker.default"
      name: "Fibonacci Worker"
      description: "Computes the next Fibonacci number in the sequence and persists state."
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.0
    capabilities:
      - type: "MemoryRead"
        scopes: ["*"]
      - type: "MemoryWrite"
        scopes: ["self.*"]
    background:
      enabled: true
      interval_secs: 20
      mode: "deterministic"
      wake_predicates: ["timer"]
    io:
      accepts:
        type: object
        properties:
          compute_next:
            type: boolean
            description: "Whether to compute the next Fibonacci number"
      returns:
        type: object
        properties:
          sequence_index:
            type: integer
          current_value:
            type: integer
          previous_value:
            type: integer
    middleware:
      pre_process: "python3 scripts/compute_fibonacci.py"
    execution_mode: "script"
    script_entry: "scripts/compute_fibonacci.py"
---
# Fibonacci Worker

A deterministic background worker that computes the Fibonacci sequence.

## Behavior

This agent runs on a 20-second cadence via the gateway scheduler. Each tick:
1. Reads the current Fibonacci state from the **content store** (or local fallback)
2. Computes the next number in the sequence
3. Writes the updated state back to the **content store**
4. Appends the result to `history/fib.log`

## State Persistence

The worker uses the **content store** via the autonoetic SDK (always available in sandbox):

```python
import autonoetic_sdk as sdk
_sdk = sdk.init()

# Read from content store
result = _sdk.files.read("fib_state.json")
state = json.loads(result["content"])

# Write to content store (returns handle)
result = _sdk.files.write("fib_state.json", state_json)
handle = result["handle"]  # e.g., "sha256:abc123..."

# Persist so it survives session cleanup
_sdk.files.persist(handle)
```

State is stored as `fib_state.json` in the content store. Other agents can retrieve it:
- `content.read("fib_state.json")` (by name, same session)
- `content.read("sha256:abc123...")` (by handle, any session)

## Directory Structure

```
fibonacci.worker.default/
├── SKILL.md           # Agent manifest
├── runtime.lock       # Dependencies
└── scripts/
    └── compute_fibonacci.py
```

No local state directory needed - all state lives in the gateway's content store.

## State Format

```json
{
  "previous": 1,
  "current": 1,
  "index": 2,
  "sequence": [1, 1, 2, 3, 5, 8]
}
```

## Deterministic Execution

This agent uses `execution_mode: script` so it runs directly in the sandbox
without invoking the LLM. This makes it:
- Fast (~100ms per tick)
- Cheap (no token usage)
- Deterministic (same input → same output)
- Reliable (no hallucination risk)

## Causal Tracing

Each tick produces causal chain entries with:
- `sequence_index` - Current Fibonacci index
- `current_value` - The newly computed number
- `storage` - Where state was persisted (content_store handle or local_file)
