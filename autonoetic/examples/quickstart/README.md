# Quickstart Example

This example gives you a runnable out-of-the-box smoke flow for the current CLI:

- creates an isolated config in `/tmp`
- initializes an agent scaffold
- rewrites the scaffolded provider to `ollama` (so no API key is required)
- starts interactive mode and exits cleanly

## Run

From `autonoetic/`:

```bash
bash examples/quickstart/run.sh
```

Optional args:

```bash
bash examples/quickstart/run.sh /tmp/autonoetic-smoke my_agent_id
```

## What it verifies

- `agent init` creates:
  - `SKILL.md`
  - `runtime.lock` (with `dependencies: []`)
  - `state/`, `history/`, `skills/`, `scripts/`
- `agent run --interactive` starts and exits on `/exit`

## Optional local-model run

If you have Ollama running locally with a model:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-quickstart/config.yaml agent run <agent_id> "Say hello" --headless
```
