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

Legacy aliases are also accepted for compatibility:

```bash
export GOOGLE_SEARCH_API_KEY="..."
export GOOGLE_SEARCH_ENGINE_ID="..."  # or GOOGLE_SEARCH_CX
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
  sed -i 's/model: ".*"/model: "google\/gemini-2.5-flash-lite"/' "$f"
done
```

## 4) Start gateway

From `autonoetic/`:

```bash
AUTONOETIC_NODE_ID=demo \
AUTONOETIC_NODE_NAME=demo \
AUTONOETIC_SHARED_SECRET=demo-secret \
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

## 7) Memory and state model (current)

Current runtime behavior is a hybrid:

- Tier 1 local state lives under each agent directory (`state/`) and is suitable for deterministic, near-term continuity.
- Tier 2 durable memory is gateway-managed (`memory.db`) and should be used for reusable/cross-session facts.
- Gateway injects compact session context for same-session continuity; this is not yet a full automatic `state/summary.md` pipeline.

For multi-step tasks that benefit from explicit textual state, prefer these conventions:

- `state/task.md` -> active checklist and next action.
- `state/scratchpad.md` -> short-lived notes/intermediate reasoning.
- `state/handoff.md` -> concise progress/blockers/next-step handoff.

## 8) Verify traces

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

## Adapter specialist docs

For schema/behavior wrapper generation via `agent-adapter.default`, including
details of `schema_diff.py` and `generate_wrapper.py`, see:

- `docs/agent-adapter-specialist.md`

**Why is `result_preview` truncated in causal_chain.jsonl?**  
Tool results in the causal chain are intentionally limited to 256 characters so log lines stay readable and bounded. The payload still has `result_len` and `result_sha256`. To get full tool output in logs, set `AUTONOETIC_EVIDENCE_MODE=full` when starting the gateway; then each tool_invoke completed entry gets an `evidence_ref` pointing to a file under the agent's `history/evidence/<session_id>/` with the full result.

**Does `causal_chain.jsonl` rotate?**  
Not yet. Current logs append to a single file per history location (`agents/.gateway/history/causal_chain.jsonl` and `agents/<agent_id>/history/causal_chain.jsonl`). Rotation/segmentation is planned.

## Common Pitfall

If `--config` points to a missing file, bootstrap now fails fast by design.

Fix:

1. create the config file first (step 1)
2. rerun bootstrap

If planner installs ad-hoc agents like `researcher` or returns unvalidated code (instead of routing to `*.default` specialists with evaluator checks), your runtime state likely drifted.

Fix:

1. stop gateway/chat processes for this config
2. remove drifted runtime agents (for example `/tmp/autonoetic-demo/agents/researcher` and generated throwaway agents)
3. re-bootstrap with overwrite
4. restart gateway and use a new session id

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --overwrite
```

Then verify only canonical specialist IDs are present before testing:

```bash
ls -1 /tmp/autonoetic-demo/agents
```

## Approvals (agent.install and scheduled actions)

When `agent_install_approval_policy` is `always` or `risk_based` and an install is high-risk, `agent.install` returns `approval_required: true` and a `request_id`. The install does not proceed until an operator approves.

**List pending approval requests:**

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals list
```

**Approve or reject a request:**

```bash
# Approve (then the caller can retry agent.install with the same payload and promotion_gate.install_approval_ref set to this request_id)
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals approve <request_id> --reason "Reviewed; OK to install"

# Reject
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals reject <request_id> --reason "Out of scope"
```

**Effect on install:** For `agent_install`-type requests, the gateway does not run the install when you approve. The gateway automatically stores the original install payload when approval is requested. When the caller retries with `promotion_gate.install_approval_ref` set to the approved `request_id`, the gateway uses the stored payload (ensuring fingerprint match). The caller does not need to recreate the exact same payload - the gateway handles this automatically.

**Payload storage details:**
- Stored at: `.gateway/scheduler/approvals/pending/<request_id>_payload.json`
- Persists across gateway restarts
- Cleaned up automatically after successful install
- Enables deterministic retry even if the LLM generates a different payload

**Rejected requests** are not retried; the caller sees the rejection and should report to the user.

**Machine-readable list:** Use `--json` for JSON output:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals list --json
```

## Troubleshooting: agent.install and memory writes

**"memory write denied by policy" / "scheduled file write denied by MemoryWrite policy"**

- The agent (or the child being installed) has a `MemoryWrite` capability with `scopes` that do not include the path you are writing to.
- **Fix:** In the agent’s SKILL (or in `agent.install` `capabilities` for the child), add a `MemoryWrite` with `scopes` that cover the path, e.g. `["skills/*", "state/*"]`. Paths must be under the agent (or child) directory; do not use absolute or `..` paths. For installed agents, prefer putting files under `skills/*` (e.g. `skills/helper.md`, `skills/script.py`) so they are clearly in scope.

**"Invalid JSON arguments for 'agent.install'" / capabilities validation errors**

- The `agent.install` payload has invalid or missing fields. Common causes: `capabilities` entries without a `type` field, or with wrong field names (e.g. `capability` instead of `type`, or missing `hosts` for `NetConnect`, `scopes` for `MemoryWrite`).
- **Fix:** The tool error includes a `repair_hint`. Use it to correct the payload: each capability must have `type` and the required fields for that type (see specialized_builder SKILL “Capability shapes”). Then retry `agent.install` with the corrected payload. Do not switch to writing files at the planner/coder root as a workaround; keep using specialized_builder and fix the payload.
