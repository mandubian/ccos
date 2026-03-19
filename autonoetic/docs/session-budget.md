# Session budget controls

For the **end-to-end enforcement flow** and **when the OpenRouter catalog** supplies context vs price estimates, see [budget-management.md](budget-management.md).

Role-agnostic limits on **how much work** one **session id** may consume across all agents that share that session (lead + nested `agent.spawn` runs using the same session).

This complements:

- **`AgentSpawn.max_children`** (per agent manifest) — how many child runs an agent may start per session.
- **`LoopGuard`** (fixed inner loop) — consecutive LLM steps without successful tools inside a single agent run.

## Configuration

Set optional limits under `session_budget` in the gateway YAML (see `autonoetic-gateway::config::load_config`). Omit a field or leave limits unset for **unlimited** in that dimension.

```yaml
# Example — tune for your environment
session_budget:
  profile: production
  max_llm_rounds: 200          # each provider completion() call, including retries
  max_tool_invocations: 800     # each tool call in a batch counts
  max_llm_tokens: 8_000_000     # sum of reported input + output tokens
  max_wall_clock_secs: 14_400   # wall time from first budget touch for this session
  max_session_price_usd: 2.5    # optional: estimated USD cap (see below)
  extensions: []                # reserved: future named gateway modules
```

## Semantics

| Limit | When enforced |
|--------|----------------|
| `max_llm_rounds` | Before each LLM completion; incremented after each real provider call (skipped when middleware uses `skip_llm`). |
| `max_llm_tokens` | After each completion, using provider-reported usage (often `0` if the API omits usage). |
| `max_tool_invocations` | Before executing a tool batch (`ToolUse`); reserves `len(tool_calls)`. |
| `max_wall_clock_secs` | Checked at the start of each LLM pre-check; clock starts on first use of that session in the registry. |
| `max_session_price_usd` | After each real LLM completion, adds an **estimated** USD cost from OpenRouter’s public [Models API](https://openrouter.ai/docs/guides/overview/models) (`pricing.prompt` / `pricing.completion` × token counts). Requires `llm_config.provider: openrouter` and a model id that exists in the catalog; if the estimate is missing, spend is not accumulated toward this cap. |

Counters live in an in-memory `SessionBudgetRegistry` shared by the gateway process (`GatewayExecutionService`). They are **not** persisted across gateway restarts.

### OpenRouter catalog (context + price)

The gateway loads model metadata from `GET https://openrouter.ai/api/v1/models` (no API key) into an in-memory cache:

- **`AUTONOETIC_OPENROUTER_CATALOG`** — set to `0`, `false`, `no`, or `off` to disable fetching (no context-from-catalog, no price estimates).
- **`AUTONOETIC_OPENROUTER_MODELS_URL`** — override the list URL if needed.

For **% of context** in logs/CLI when `context_window_tokens` / `AUTONOETIC_LLM_CONTEXT_WINDOW` are unset: if `provider` is `openrouter`, the gateway may fill the window from the catalog’s `context_length` for the configured model id.

## Evolution / plug-in story

- **Config-first:** add new optional fields to `SessionBudgetConfig` in `autonoetic-types` when you need new numeric limits; wire them in `runtime/session_budget.rs` and document here.
- **`extensions`:** reserved list of names for future optional modules (e.g. org-specific rate limiters) without breaking existing YAML.
- **Custom code:** implement additional checks inside the gateway by extending `SessionBudgetRegistry` or calling it from new hook points; keep policy **session-scoped**, not role-aware.

## Related code

- `autonoetic_types::config::SessionBudgetConfig`
- `autonoetic_gateway::runtime::session_budget::SessionBudgetRegistry`
- `AgentExecutor::with_session_budget` + lifecycle hooks in `runtime/lifecycle.rs`

## LLM token usage in logs and CLI

Each real LLM completion records **input/output tokens** (from the provider) into session evidence / timeline, returns them in JSON-RPC as `llm_usage` (array of `{ model, input_tokens, output_tokens, context_window_tokens?, input_context_pct?, estimated_cost_usd? }`), and prints a summary on **stderr** for `autonoetic agent run` / interactive mode.

To show **approximate % of context window** used by the prompt (`input_tokens` / window):

1. Set `context_window_tokens` under `llm_config` in the agent `SKILL.md`, or  
2. Set env `AUTONOETIC_LLM_CONTEXT_WINDOW` (used when the manifest omits the field), or  
3. For **OpenRouter** agents, the catalog may supply `context_length` for the configured model id.

If none apply, totals and per-round token counts still appear; the percentage line is omitted.

When catalog pricing is available, **`estimated_cost_usd`** is included (rough estimate from public list prices).
