# Quickstart: Planner to Specialist Chat

This quickstart verifies the full implicit-routing flow:

1. terminal chat ingress with no explicit agent target
2. gateway routes to `planner.default`
3. planner delegates to a specialist via `agent.spawn`
4. specialist result returns in the same session

It also includes the required config-file step so `agent bootstrap` does not fall back to unintended defaults.

## Prerequisites

- workspace root available
- Rust toolchain installed
- OpenRouter key available in your environment (`OPENROUTER_API_KEY`)

## 1) Create config first (required)

```bash
mkdir -p /tmp/autonoetic-demo
cat > /tmp/autonoetic-demo/config.yaml <<'EOF'
agents_dir: "/tmp/autonoetic-demo/agents"
port: 4000
ofp_port: 4200
tls: false
default_lead_agent_id: "planner.default"
max_concurrent_spawns: 4
max_pending_spawns_per_agent: 4
background_scheduler_enabled: false
EOF
```

## 2) Bootstrap reference bundles into runtime agents

From `autonoetic/`:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap
```

Optional:

```bash
# Force replacement of existing runtime agent dirs
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --overwrite

# Use an explicit bundle source directory
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --from /path/to/autonoetic/agents
```

## 3) Configure agents for OpenRouter + Gemini Flash Lite

After bootstrap, patch the runtime bundles in `/tmp/autonoetic-demo/agents`:

```bash
for f in /tmp/autonoetic-demo/agents/*/SKILL.md; do
  sed -i 's/provider: ".*"/provider: "openrouter"/' "$f"
  sed -i 's/model: ".*"/model: "google\/gemini-2.0-flash-lite-001"/' "$f"
done
```

## 4) Start gateway

From `autonoetic/`:

```bash
AUTONOETIC_NODE_ID=demo \
AUTONOETIC_NODE_NAME=demo \
AUTONOETIC_SHARED_SECRET=demo-secret \
OPENROUTER_API_KEY=... \
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway start
```

Do not set `AUTONOETIC_LLM_API_KEY` when using provider-specific keys. It is a global override.

If you previously exported overrides in your shell, clear them before starting the gateway:

```bash
unset AUTONOETIC_LLM_API_KEY AUTONOETIC_LLM_BASE_URL
```

## 5) Open terminal chat with implicit routing

In a second terminal, from `autonoetic/`:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml chat --session-id demo-session
```

Do not pass an `agent_id`. This exercises implicit routing to the session/default lead.

## 6) Trigger delegation

In chat, send a request that should require specialist work, for example:

```text
Research Rust JSON-RPC libraries and summarize tradeoffs.
```

Expected behavior:

- gateway ingress resolves to `planner.default`
- planner uses `agent.spawn` to call an appropriate specialist (for example `researcher.default`)
- planner synthesizes and returns response

## 7) Verify traces

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace sessions --agent planner.default
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace show demo-session --agent planner.default
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace sessions --agent researcher.default
```

You should see:

- planner session activity for `demo-session`
- tool usage including `agent.spawn` in planner trace
- specialist session activity tied to the same request lineage

## Common Pitfall

If `--config` points to a missing file, bootstrap now fails fast by design.

Fix:

1. create the config file first (step 1)
2. rerun bootstrap
