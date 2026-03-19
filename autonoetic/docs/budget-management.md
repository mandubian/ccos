# Budget management

This document describes **how session budgets are enforced** in Autonoetic and **when the OpenRouter models catalog** is consulted for **context window size** and **estimated price**.

For YAML fields and limit semantics, see [session-budget.md](session-budget.md).

## Scope

- **Session id** — All counters are keyed by the same session id used for `agent.spawn`, chat, and nested runs. One budget pool per session per gateway process.
- **Process-local** — `SessionBudgetRegistry` is in-memory; restarting the gateway resets counters.
- **Not a billing system** — USD figures are **estimates** from OpenRouter’s public model list, not provider invoices.

## Enforcement flow (high level)

1. **Before each LLM attempt** — `check_pre_llm` (wall clock, max LLM rounds).
2. **After each real completion** (middleware did **not** set `skip_llm`) — `record_llm_completion` (token totals, optional estimated USD, increments round count).
3. **Before each tool batch** — `reserve_tool_invocations` (tool call budget).

Code: `autonoetic_gateway::runtime::session_budget::SessionBudgetRegistry` + hooks in `runtime/lifecycle.rs` (`AgentExecutor::execute_with_history`).

## When `OpenRouterCatalog` is used

The catalog wraps OpenRouter’s public [Models API](https://openrouter.ai/docs/guides/overview/models) (`GET …/api/v1/models`). It caches results (TTL ~1 hour) and can be disabled or pointed at a custom URL — see [session-budget.md § OpenRouter catalog](session-budget.md#openrouter-catalog-context--price).

An `OpenRouterCatalog` instance is attached when:

- The **gateway** builds an `AgentExecutor` (`GatewayExecutionService`), sharing the gateway’s `reqwest::Client`.
- The **CLI** runs an agent (`run_agent_with_runtime_with_driver`), using a dedicated client.

If `openrouter_catalog` is `None`, no catalog lookups run (no automatic context window from OpenRouter, no `estimated_cost_usd`).

### Maximum context (`context_length`)

**When:** Once per **`execute_with_history` invocation**, before the main agent loop runs.

**Function:** `resolve_context_window_for_run` → `OpenRouterCatalog::context_length_for_model(model_id)` **only if**:

1. The manifest does **not** set `llm_config.context_window_tokens`, **and**
2. Env **`AUTONOETIC_LLM_CONTEXT_WINDOW`** is unset, **and**
3. `llm_config.provider` is **`openrouter`** (case-insensitive), **and**
4. `AgentExecutor` was given `with_openrouter_catalog(Some(…))`.

Otherwise the catalog is **not** queried for context; manifest/env wins.

**Why:** The resolved value drives **“% of context”** UX (logs, CLI, `llm_usage.context_window_tokens` / `input_context_pct`) for that run. It is **not** sent as a hard API limit to the provider.

**Network:** `context_length_for_model` calls `refresh_if_needed`, which may `GET` the models list if the cache is empty or stale.

### Estimated price (`pricing` → USD)

**When:** **After every real LLM completion** in the loop — specifically **after** the post-process middleware runs, and **only if** `skip_llm` is false.

**Function:** `OpenRouterCatalog::estimate_cost_usd(model_id, input_tokens, output_tokens)` multiplies token counts by cached per-token `pricing.prompt` / `pricing.completion` for that model id.

**Used for:**

- **`LlmExchangeUsage.estimated_cost_usd`** (JSON-RPC / CLI).
- **`max_session_price_usd`** — `record_llm_completion` adds the estimate to the session’s running USD total; if the estimate is `None`, **no USD is added** for that completion (the cap may never trigger).

**Network:** Same cache as context; `estimate_cost_usd` also triggers `refresh_if_needed` (usually no extra HTTP if the cache was just populated).

### Rounds where the LLM is skipped

If pre-process middleware sets `skip_llm: true`:

- No provider call, no `estimate_cost_usd`, no `record_llm_completion` for that iteration.
- Context percentage for that iteration is omitted (`context_window_tokens` forced to `None` for that round in the tracer path).

## Configuration reference (quick)

| Concern | Where to configure |
|--------|---------------------|
| Limits | Gateway YAML `session_budget` — [session-budget.md](session-budget.md) |
| Disable catalog fetch | `AUTONOETIC_OPENROUTER_CATALOG` |
| Override models URL | `AUTONOETIC_OPENROUTER_MODELS_URL` |
| Context without catalog | `llm_config.context_window_tokens` or `AUTONOETIC_LLM_CONTEXT_WINDOW` |

## Related code

| Component | Path |
|-----------|------|
| Budget registry | `autonoetic-gateway/src/runtime/session_budget.rs` |
| OpenRouter cache + pricing | `autonoetic-gateway/src/runtime/openrouter_catalog.rs` |
| Lifecycle: resolution + `record_llm_completion` | `autonoetic-gateway/src/runtime/lifecycle.rs` |
| Config struct | `autonoetic_types::config::SessionBudgetConfig` |
