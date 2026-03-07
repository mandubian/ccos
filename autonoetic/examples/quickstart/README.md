# Quickstart Example

This quickstart now ships with a concrete sample agent, `Field Journal`, meant to exercise the real CLI path:

- terminal chat
- gateway JSON-RPC ingress
- agent execution
- session context continuity
- memory write/read
- causal trace inspection

The sample agent is intentionally simple: it stores one fact when you ask it to remember something, uses same-session continuity for immediate follow-ups, and can still read the stored fact back in a fresh later session.

## Run

From `autonoetic/`:

```bash
bash examples/quickstart/run.sh
```

Optional args:

```bash
bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id openrouter_gfl
bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id openrouter_gfl_manual
bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id smoke
```

Behavior on existing agent directory:

- Reuses existing `agents/<agent_id>` by default.
- To reset and reinitialize, set:

```bash
AUTONOETIC_QUICKSTART_RESET=1 bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id openrouter_gfl
```

Modes:

- `openrouter_gfl` (default): patches the sample agent to use OpenRouter + `google/gemini-3-flash-preview`, starts the gateway, then drives two real terminal chat conversations:
  - `Remember that my project codename is Atlas.`
  - `What did I just ask you to remember?`
  - then a new session asks `What is my project codename?`
- `openrouter_gfl_manual`: uses the same OpenRouter setup, starts the gateway, then leaves you in a live `autonoetic chat` session so you can type your own messages.
- `smoke`: patches the sample agent to `ollama`, starts the gateway, and validates that the terminal chat client can connect and cleanly exit on `/exit`.

For `openrouter_gfl`, set:

```bash
export OPENROUTER_API_KEY=...
```

## Sample agent

The quickstart agent lives under:

```text
examples/quickstart/sample_agent/
```

It is designed for reality, not just scaffolding:

- it has `MemoryWrite` and `MemoryRead` capabilities
- it can benefit from gateway-provided session context on same-session follow-ups
- it tells the model exactly when to store a user fact
- it tells the model to read before answering recall questions
- it stays small enough that failures are easy to debug

## What it verifies

- the gateway starts and accepts JSON-RPC chat ingress
- `autonoetic chat <agent_id>` routes through `event.ingest`
- the agent writes to `state/` on the first turn
- same-session follow-up can use injected session context
- a fresh later session can still succeed by reading durable memory
- gateway and agent traces are written under the same terminal session
- the sample is inspectable afterwards with `trace sessions` / `trace show`

Inspect trace:

```bash
cat /tmp/autonoetic-quickstart/agents/<agent_id>/history/causal_chain.jsonl
cargo run -p autonoetic -- --config /tmp/autonoetic-quickstart/config.yaml trace sessions --agent <agent_id>
cargo run -p autonoetic -- --config /tmp/autonoetic-quickstart/config.yaml trace show quickstart-session-<agent_id> --agent <agent_id>
cargo run -p autonoetic -- --config /tmp/autonoetic-quickstart/config.yaml trace show quickstart-session-<agent_id>-new --agent <agent_id>
cat /tmp/autonoetic-quickstart/agents/<agent_id>/state/latest_fact.txt
cat /tmp/autonoetic-quickstart/agents/<agent_id>/state/latest_fact_label.txt
ls /tmp/autonoetic-quickstart/agents/<agent_id>/state/sessions
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

- `session_id`: stable across one terminal chat conversation (`chat ...`)
- `turn_id`: increments per agent turn (`turn-000001`, ...)
- `event_seq`: monotonic event sequence within the session

Session semantics in the quickstart:

- first chat run uses `quickstart-session-<agent_id>` to demonstrate same-session continuity
- second chat run uses `quickstart-session-<agent_id>-new` to demonstrate durable-memory recall without prior session context

## Config alignment

The default `openrouter_gfl` mode matches your config intent:
- `provider = "openrouter"`
- `model = "google/gemini-3-flash-preview"`
