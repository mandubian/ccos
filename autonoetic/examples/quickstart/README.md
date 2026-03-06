# Quickstart Example

This example gives you a runnable out-of-the-box smoke flow for the current CLI:

- creates an isolated config in `/tmp`
- initializes an agent scaffold
- can run a real OpenRouter model call using your known-good profile:
  - `provider = "openrouter"`
  - `model = "google/gemini-3-flash-preview"`

## Run

From `autonoetic/`:

```bash
bash examples/quickstart/run.sh
```

Optional args:

```bash
bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id openrouter_gfl
bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id smoke
```

Behavior on existing agent directory:

- Reuses existing `agents/<agent_id>` by default.
- To reset and reinitialize, set:

```bash
AUTONOETIC_QUICKSTART_RESET=1 bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id openrouter_gfl
```

Modes:

- `openrouter_gfl` (default): uses OpenRouter + `google/gemini-3-flash-preview` and performs a real headless model invocation.
- `smoke`: switches provider to `ollama` and only validates interactive startup/exit (`/exit`).

For `openrouter_gfl`, set:

```bash
export OPENROUTER_API_KEY=...
```

## What it verifies

- `agent init` creates:
  - `SKILL.md`
  - `runtime.lock` (with `dependencies: []`)
  - `state/`, `history/`, `skills/`, `scripts/`
- `openrouter_gfl`: `agent run ... --headless` can invoke your known-good OpenRouter model
- `smoke`: `agent run --interactive` starts and exits on `/exit`
- lifecycle writes a causal trace at `agents/<agent_id>/history/causal_chain.jsonl`

Inspect trace:

```bash
cat /tmp/autonoetic-quickstart/agents/<agent_id>/history/causal_chain.jsonl
cargo run -p autonoetic -- --config /tmp/autonoetic-quickstart/config.yaml trace sessions --agent <agent_id>
cargo run -p autonoetic -- --config /tmp/autonoetic-quickstart/config.yaml trace show <session_id> --agent <agent_id>
```

Capture full redacted evidence blobs (optional):

```bash
export AUTONOETIC_EVIDENCE_MODE=full
bash examples/quickstart/run.sh /tmp/autonoetic-quickstart <agent_id> openrouter_gfl
```

When enabled, causal entries include `evidence_ref` pointers to files under:

```text
agents/<agent_id>/history/evidence/<session_id>/*.json
```

Session semantics in top-level causal fields:

- `session_id`: stable across one CLI invocation (`agent run ...`)
- `turn_id`: increments per agent turn (`turn-000001`, ...)
- `event_seq`: monotonic event sequence within the session

## Config alignment

The default `openrouter_gfl` mode matches your config intent:
- `provider = "openrouter"`
- `model = "google/gemini-3-flash-preview"`
