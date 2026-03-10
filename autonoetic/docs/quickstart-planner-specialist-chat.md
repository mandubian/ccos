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

## 2b) Researcher and web search (required for "search today's weather" etc.)

The researcher can use native `web.search` and `web.fetch` only if its runtime SKILL has a **NetConnect** capability that allows the target hosts (e.g. DuckDuckGo, or `*` for all).

- If you see errors when the researcher runs goals like "search today's weather", the runtime researcher may have been created from an older bundle without NetConnect. Re-bootstrap so it gets the current researcher (with `NetConnect` and `hosts: ["*"]`):

  ```bash
  cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --overwrite
  ```

- To confirm the runtime researcher can use web search, check that its SKILL includes NetConnect:

  ```bash
  grep -A1 "NetConnect" /tmp/autonoetic-demo/agents/researcher.default/SKILL.md
  ```

  You should see `hosts: ["*"]` (or at least hosts that include `duckduckgo.com` and any other search/fetch targets).

- **If NetConnect is present but the researcher still doesn't use web search** (e.g. for "search today's weather"): the model may be answering from training data instead of calling the tool. Re-bootstrap so the researcher gets the latest instructions (which tell it to always call `web.search` first for current/live info), then restart the gateway and try again. You can also inspect the planner/researcher trace to see whether `web.search` was in the tool list and whether the model requested it:

  ```bash
  cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace show demo-session --agent researcher.default
  ```

## 2c) Optional: add MCP web tools (native web tools already available)

You can still add MCP web tools for richer provider-specific search/fetch behavior.

To enable Google provider in native `web.search`, export:

```bash
export AUTONOETIC_GOOGLE_SEARCH_API_KEY="..."
export AUTONOETIC_GOOGLE_SEARCH_ENGINE_ID="..."
```

Then the researcher can call `web.search` with either explicit Google or auto fallback:

```json
{ "query": "rust async runtime", "provider": "google" }
```

```json
{
  "query": "rust async runtime",
  "provider": "auto",
  "cache_ttl_secs": 120
}
```

`provider: "auto"` tries Google first when credentials are available, then falls back to DuckDuckGo on missing credentials, errors, or empty Google results.
`cache_ttl_secs` controls in-memory response caching (0 disables cache, max 3600 seconds).

**If `web.search` returns `result_count: 0`** (e.g. for "weather in Paris"): DuckDuckGo's API often returns no results for weather and similar instant-answer queries. The tool call still succeeds (`ok: true`); the researcher just gets an empty result set. For better coverage on live/weather queries, set up the Google provider (see above) and use `provider: "auto"` or `"google"` so the researcher can use Google Custom Search when available.

Additional `web.search` options for advanced setups:

- `duckduckgo_engine_url`: override DuckDuckGo endpoint for local/mock engines.
- `google_engine_url`: override Google endpoint for local/mock engines.
- `google_api_key_env`: env var name for API key (default `AUTONOETIC_GOOGLE_SEARCH_API_KEY`).
- `google_engine_id_env`: env var name for Custom Search Engine ID (default `AUTONOETIC_GOOGLE_SEARCH_ENGINE_ID`).

Response metadata now includes:

- `requested_provider`: provider asked by caller (`auto`, `google`, or `duckduckgo`).
- `attempted_providers`: providers tried in execution order.
- `fallback_reason`: why fallback occurred (present when fallback is used).
- `cache_hit`: whether response came from cache.

Example registration:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml \
  mcp add web --command /path/to/your-web-mcp-server -- --stdio
```

Then verify MCP availability:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway status
```

If you want stricter network policy, narrow `researcher.default` `NetConnect.hosts` in your runtime agent bundle instead of using `["*"]`.

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

**Where to look:**

- **Gateway causal chain** — `agents/.gateway/history/causal_chain.jsonl` — records every ingress (top-level `event.ingest` when you chat) and every **delegation** (each `agent.spawn` from planner → researcher, coder, etc.). One place to see the full delegation tree for a session.
- **Per-agent causal chains** — `agents/<agent_id>/history/causal_chain.jsonl` — record that agent’s lifecycle, LLM calls, and tool invocations (including `agent.spawn` requests and results as seen by that agent).

```bash
# Gateway log (all delegations for the session)
cat /tmp/autonoetic-demo/agents/.gateway/history/causal_chain.jsonl

# Per-agent traces
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace sessions --agent planner.default
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace show demo-session --agent planner.default
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace sessions --agent researcher.default
```

You should see:

- gateway log: `event.ingest.requested` / `event.ingest.completed` for the chat, then `agent.spawn.requested` / `agent.spawn.completed` for each delegation (researcher, architect, coder, etc.);
- planner session activity for `demo-session`;
- tool usage including `agent.spawn` in planner trace;
- specialist session activity tied to the same request lineage.

**Why is `result_preview` truncated in causal_chain.jsonl?**  
Tool results in the causal chain are intentionally limited to 256 characters so log lines stay readable and bounded. The payload still has `result_len` and `result_sha256`. To get full tool output in logs, set `AUTONOETIC_EVIDENCE_MODE=full` when starting the gateway; then each tool_invoke completed entry gets an `evidence_ref` pointing to a file under the agent's `history/evidence/<session_id>/` with the full result.

## Common Pitfall

If `--config` points to a missing file, bootstrap now fails fast by design.

Fix:

1. create the config file first (step 1)
2. rerun bootstrap
