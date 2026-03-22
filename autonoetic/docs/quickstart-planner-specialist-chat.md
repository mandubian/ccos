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

## 1) Create config (quick method)

Use `agent init-config` to generate a config with LLM presets:

```bash
mkdir -p /tmp/autonoetic-demo
cargo run -p autonoetic -- agent init-config --output /tmp/autonoetic-demo/config.yaml --overwrite
```

This creates a config with:
- Gateway settings (ports, limits)
- LLM presets (agentic, coding, research, fallback)
- Template-to-preset mappings for automatic LLM selection

### Alternative: Manual config

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

# LLM presets for role-specific model selection
llm_presets:
  agentic:
    provider: "openrouter"
    model: "google/gemini-2.5-flash-lite"
    temperature: 0.2
  coding:
    provider: "openrouter"
    model: "google/gemini-2.5-flash-lite"
    temperature: 0.1
  research:
    provider: "openrouter"
    model: "google/gemini-2.5-flash-lite"
    temperature: 0.3

# Template → Preset mapping
llm_preset_mapping:
  planner: agentic
  researcher: research
  architect: agentic
  coder: coding
  debugger: coding
  auditor: agentic
  specialized_builder: agentic
  default: agentic
EOF
```

## 2) Bootstrap reference bundles into runtime agents

From `autonoetic/`:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap
```

Bootstrap automatically applies LLM presets from config:
- If `llm_preset_mapping` exists, each template uses its mapped preset
- If no mapping, templates use role-specific defaults (planner → agentic, coder → coding, etc.)
- Override with `--preset` flag when creating individual agents

Optional:

```bash
# Force replacement of existing runtime agent dirs
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --overwrite

# Use an explicit bundle source directory
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --from /path/to/autonoetic/agents
```

## 2b) Check LLM presets

After bootstrap, verify the LLM configuration:

```bash
# List configured presets and template mappings
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent presets
```

This shows available presets and which template uses which preset.

## 2c) Researcher and web search (required for "search today's weather" etc.)

The researcher can use native `web.search` and `web.fetch` only if its runtime SKILL has a **NetworkAccess** capability that allows the target hosts (e.g. DuckDuckGo, or `*` for all).

- If you see errors when the researcher runs goals like "search today's weather", the runtime researcher may have been created from an older bundle without NetworkAccess. Re-bootstrap so it gets the current researcher (with `NetworkAccess` and `hosts: ["*"]`):

  ```bash
  cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent bootstrap --overwrite
  ```

- To confirm the runtime researcher can use web search, check that its SKILL includes NetworkAccess:

  ```bash
  grep -A1 "NetworkAccess" /tmp/autonoetic-demo/agents/researcher.default/SKILL.md
  ```

  You should see `hosts: ["*"]` (or at least hosts that include `duckduckgo.com` and any other search/fetch targets).

- **If NetworkAccess is present but the researcher still doesn't use web search** (e.g. for "search today's weather"): the model may be answering from training data instead of calling the tool. Re-bootstrap so the researcher gets the latest instructions (which tell it to always call `web.search` first for current/live info), then restart the gateway and try again. You can also inspect the planner/researcher trace to see whether `web.search` was in the tool list and whether the model requested it:

  ```bash
  cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml trace show demo-session --agent researcher.default
  ```

## 2d) Optional: add MCP web tools (native web tools already available)

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

## 3) Create new agents with LLM presets

After bootstrap, create additional agents with specific LLMs:

```bash
# Using preset name from config
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml \
  agent init weather_agent --template coder --preset coding

# Using direct provider/model override
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml \
  agent init search_agent --template researcher \
  --provider anthropic --model claude-sonnet-4-20250514

# List available presets
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml agent presets
```

### Agent Template Defaults

When no preset is specified, each template uses a role-optimized default:

| Template | Default Provider | Default Model | Why |
|----------|-----------------|---------------|-----|
| planner | anthropic | claude-sonnet-4-20250514 | Best agentic/tool-use capabilities |
| researcher | openai | gpt-4o | Strong research and synthesis |
| coder | anthropic | claude-sonnet-4-20250514 | Best code generation |
| auditor | anthropic | claude-sonnet-4-20250514 | Careful analysis |
| generic | openai | gpt-4o | Balanced capabilities |

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

### Bubblewrap compatibility toggles (optional)

Default sandbox behavior is unchanged (strict network namespace + legacy `/dev` handling).  
For environments where `bwrap --unshare-net` cannot configure loopback or where `/dev/null` writes fail, you can enable compatibility flags:

| Env var | Values | Effect | Default |
|---|---|---|---|
| `AUTONOETIC_BWRAP_SHARE_NET` | `1/true/yes/on` or `0/false/no/off` | Adds `--share-net` (uses host network namespace) | Off |
| `AUTONOETIC_BWRAP_DEV_MODE` | `legacy`, `minimal`, `host-bind` | Controls `/dev` mount strategy (`legacy`: unchanged, `minimal`: `--dev /dev`, `host-bind`: `--dev-bind /dev /dev`) | `legacy` |

Recommended for this environment:

```bash
AUTONOETIC_NODE_ID=demo \
AUTONOETIC_NODE_NAME=demo \
AUTONOETIC_SHARED_SECRET=demo-secret \
AUTONOETIC_BWRAP_SHARE_NET=1 \
AUTONOETIC_BWRAP_DEV_MODE=host-bind \
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway start
```

This is intentionally opt-in so other environments keep the previous bwrap command shape.

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

**Human-readable session views:**

- `agents/.gateway/sessions/<session_id>/timeline.md` — progressive Markdown timeline for the whole session (includes mirrored `workflow.*` gateway rows when delegation uses the durable workflow store).
- `agents/.gateway/sessions/<session_id>/workflow_graph.md` — rewritten whenever a workflow event appends: current `workflow_id`, task list, and recent `events.jsonl` lines (open beside `timeline.md` for a live orchestration snapshot).
- `agents/.gateway/sessions/<session_id>/artifacts/<artifact_id>/` — named projection of built artifact files so you can open generated code directly without resolving SHA handles by hand.
- failed or approval-blocked tool runs now attach an `evidence_ref` in the timeline/causal entry, pointing to the full redacted result payload (useful for test stdout/stderr and approval details).

## Adapter specialist docs

For schema/behavior wrapper generation via `agent-adapter.default`, including
details of `schema_diff.py` and `generate_wrapper.py`, see:

- `docs/agent-adapter-specialist.md`

**Why is `result_preview` truncated in causal_chain.jsonl?**  
Tool results in the causal chain are intentionally limited to 256 characters so log lines stay readable and bounded. The payload still has `result_len` and `result_sha256`. By default, the gateway now captures full redacted evidence and adds an `evidence_ref` for traced events under the agent's `history/evidence/<session_id>/`. If you want to reduce evidence volume, set `AUTONOETIC_EVIDENCE_MODE=off`; failed and approval-blocked tool runs will still preserve an `evidence_ref` so the full error/test payload remains inspectable.

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

When `agent_install_approval_policy` is `always` or `risk_based` and an install is high-risk, `agent.install` returns `approval_required: true` and a `request_id` (short ID format like `apr-db51b7ad`). The install does not proceed until an operator approves.

**List pending approval requests:**

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals list
```

**Approve or reject a request:**

```bash
# Approve - gateway auto-completes install actions when applicable
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals approve apr-db51b7ad --reason "Reviewed; OK to install"

# Reject
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals reject apr-db51b7ad --reason "Out of scope"
```

**Execution and notification flow (recommended):**
1. `agent.install` returns `approval_required: true` with `request_id: "apr-db51b7ad"`
2. Operator approves via CLI
3. **Gateway automatically executes the install** using the stored payload
4. Gateway persists an approval-resolution notification for the waiting session
5. If terminal chat is open on that session, chat resumes automatically and displays the continuation
6. If no consumer is connected, notification remains pending until acknowledged

You should not need to type manual prompts like `continue` or `done` after approval in the normal chat path.

**Delivery semantics (current model):**
- Approval-resolution messages use a structured payload (`type: "approval_resolved"` with `request_id`, `status`, `agent_id`, `install_completed`, `message`).
- The gateway owns background polling/delivery; CLI approval commands only record the decision.
- Chat acknowledges notification consumption only after successful resume/render.
- Pending notifications are durable in the `GatewayStore` SQLite database until consumed.

**Payload storage details:**
- Stored directly in the `GatewayStore` SQLite `approvals` table.
- Persists across gateway restarts natively.
- Cleaned up or marked completed automatically after successful install.
- Enables deterministic execution even if LLM output differs

**Rejected requests** are not retried; the caller sees the rejection and should report to the user.

**LLM truncation note:** Short approval IDs (`apr-XXXXXXXX`) are used to avoid truncation bugs in some LLMs (e.g., Gemini 3 Flash truncates UUIDs by one character).

**Machine-readable list:** Use `--json` for JSON output:

```bash
cargo run -p autonoetic -- --config /tmp/autonoetic-demo/config.yaml gateway approvals list --json
```
For architecture details, see `docs/approval-notification-delivery.md`.

## Troubleshooting: agent.install and memory writes

**"memory write denied by policy" / "scheduled file write denied by MemoryWrite policy"**

- The agent (or the child being installed) has a `MemoryWrite` capability with `scopes` that do not include the path you are writing to.
- **Fix:** In the agent’s SKILL (or in `agent.install` `capabilities` for the child), add a `MemoryWrite` with `scopes` that cover the path, e.g. `["skills/*", "state/*"]`. Paths must be under the agent (or child) directory; do not use absolute or `..` paths. For installed agents, prefer putting files under `skills/*` (e.g. `skills/helper.md`, `skills/script.py`) so they are clearly in scope.

**"Invalid JSON arguments for 'agent.install'" / capabilities validation errors**

- The `agent.install` payload has invalid or missing fields. Common causes: `capabilities` entries without a `type` field, or with wrong field names (e.g. `capability` instead of `type`, or missing `hosts` for `NetConnect`, `scopes` for `MemoryWrite`).
- **Fix:** The tool error includes a `repair_hint`. Use it to correct the payload: each capability must have `type` and the required fields for that type (see specialized_builder SKILL “Capability shapes”). Then retry `agent.install` with the corrected payload. Do not switch to writing files at the planner/coder root as a workaround; keep using specialized_builder and fix the payload.

## Shell Execution Safety Policy (sandbox.exec)

Some specialists can execute shell via `sandbox.exec` (typically through `bash -c` or `sh -c`).

| Class | Examples | Policy |
|---|---|---|
| Safe deterministic shell glue | `bash -c 'pytest -q'`, `bash -c 'ls src'`, `bash -c 'cat report.txt'` | Allowed if agent `CodeExecution` patterns permit it |
| Destructive filesystem operations | `rm`, `rmdir`, `unlink`, `shred`, `wipefs`, `mkfs`, `dd`, `find ... -delete` | Hard deny |
| Privilege escalation | `sudo`, `su`, `doas`, setuid/setgid patterns | Hard deny |
| Environment/process disclosure | `env`, `printenv`, `declare -x`, `/proc/*/environ` reads | Hard deny |

If a command matches an agent's `CodeExecution` pattern but still fails with permission/security errors, assume the command hit one of these hard boundaries and rewrite the approach.

**Networking and `/dev` troubleshooting with bubblewrap**

- `bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted` means the host/kernel blocks loopback setup in isolated net namespaces. Use `AUTONOETIC_BWRAP_SHARE_NET=1` for that environment.
- `curl` reporting `HTTP:200` together with `Failure writing output to destination` means the request succeeded but output write failed (often `/dev/null` or destination path). Use writable paths (for example `/tmp/...`) and, if needed, `AUTONOETIC_BWRAP_DEV_MODE=host-bind` or `minimal`.
