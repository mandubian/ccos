# Specialized Builder Example

This example promotes the builder flow into a real runnable agent.

It starts a builder agent that can install durable specialist child agents from normal terminal chat. The first built-in demo is the exact Fibonacci worker request:

`schedule every 20sec next fibonacci series element from previous element computed in last turn`

That request is part of the demo harness, not part of the builder agent's embedded task logic. The sample builder skill is intended to stay generic and infer recurring-worker structure from the user's request.

Because the builder is generic, the installed child agent id and file layout are model-derived. The demo script discovers the installed child agent after the chat turn and prints whatever state and history files that worker actually created.

What it proves:

- terminal chat reaches the gateway through `event.ingest`
- the builder agent uses `agent.install`
- a durable child agent is written to disk
- the background scheduler picks up the child agent automatically
- the child agent executes sandboxed code and persists state between ticks

What it does not prove by itself:

- broad generalization across many worker types
- model robustness beyond this single demo workload

That broader proof has to come from varying the input tasks and keeping the builder skill generic.

## Run

From `autonoetic/`:

```bash
bash examples/specialized_builder/run.sh
```

Default mode is `demo_fibonacci`, which will:

1. Install the builder agent into an isolated `/tmp` workspace.
2. Start the gateway with the background scheduler enabled.
3. Send the Fibonacci scheduling prompt through terminal chat.
4. Detect the child agent that the builder installed.
5. Wait briefly for the first worker tick.
6. Wait for two scheduler ticks (`background.should_wake.completed`) and validate that the measured cadence is approximately 20 seconds.
7. Print the installed worker's actual state and history files.

The demo now fails fast if cadence is outside the expected range.

Default validation window:

- expected interval: `20s`
- tolerance: `+/- 6s`

Override via environment variables when needed:

```bash
AUTONOETIC_EXPECTED_INTERVAL_SECS=20 AUTONOETIC_INTERVAL_TOLERANCE_SECS=8 bash examples/specialized_builder/run.sh
```

If your machine is slow and the check times out before two ticks are observed, increase the wait window:

```bash
AUTONOETIC_CADENCE_WAIT_TIMEOUT_SECS=150 bash examples/specialized_builder/run.sh
```

If you want to wait for more scheduler cycles (for example 20 ticks), use:

```bash
AUTONOETIC_REQUIRED_SCHEDULER_TICKS=20 bash examples/specialized_builder/run.sh
```

You can also run:

```bash
bash examples/specialized_builder/run.sh /tmp/autonoetic-specialized-builder builder_agent manual
```

That starts the gateway and drops you into a live terminal chat with the builder agent so you can type your own specialization requests.

## Modes

- `demo_fibonacci`: scripted Fibonacci-worker install and first tick verification
- `manual`: interactive terminal chat with the builder agent
- `smoke`: local startup/exit check using `ollama`

## Environment

For `demo_fibonacci` and `manual`, set:

```bash
export OPENROUTER_API_KEY=...
```

The script patches the sample builder agent to use OpenRouter `google/gemini-3-flash-preview` by default.

## Inspect Results

The installed worker will appear under a derived child directory such as:

```text
agents/<child-agent-id>/
```

Useful files:

- `agents/<child-agent-id>/state/*`
- `agents/<child-agent-id>/history/*`
- `agents/.gateway/history/causal_chain.jsonl`

Useful trace commands:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-specialized-builder/config.yaml trace sessions --agent <child-agent-id>
cargo run -p autonoetic -- --config /tmp/autonoetic-specialized-builder/config.yaml trace show background::<child-agent-id> --agent <child-agent-id>
```
