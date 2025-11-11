## CCOS Smart Assistant – Generalization Plan (Draft)

### Context
The demo now runs end-to-end, but several pieces are too specific to the GitHub example and ad-hoc heuristics. This plan generalizes the system, introduces a schema-aware “local synthesis” framework based on generic data primitives, enables RTFS-safe execution for synthesized code, and adds a post-discovery rewrite to optimize plans (pushdowns, chain collapse). It keeps with RTFS principles (pure functions; immutable values) and leverages the CCOS prelude where appropriate.

### Goals
- Generalize capability synthesis around reusable, typed primitives: filter, map, reduce, project, sort, groupBy, join.
- Execute synthesized RTFS safely (restricted runtime), not placeholders.
- Make discovery matching/overrides configurable, not code-wired.
- Optimize orchestrators by pushing filters server-side and collapsing trivial local chains.
- Enable LLM-backed RTFS generation with grammar hints + validator loop.
- Replace heuristic RTFS loaders with the canonical parser for both capabilities and plans.

### Work Packages and Milestones
1) LocalSynth: schema-aware primitive framework
- Deliverables:
  - Primitive registry (filter/map/reduce/project/sort/groupBy/join) with type contracts.
  - Binding engine that picks inputs/outputs from schemas, not name heuristics.
  - Canonical output naming that matches plan keys exactly.
- Acceptance:
  - Synthesized capabilities pass type validation; input/output keys match requested schemas.
  - Unit tests for each primitive.

2) Safe RTFS execution for synthesized code
- Deliverables:
  - Restricted RTFS runtime for evaluation of synthesized functions (whitelisted stdlib).
  - Static analyzers to disallow dangerous forms and ensure purity semantics.
  - Error model with clear diagnostics when execution is blocked or unsafe.
- Acceptance:
  - Synthesized steps produce real results in the demo; no placeholder strings.
  - Denylisted forms are rejected at registration time with actionable errors.

3) Discovery matching strategies → config
- Deliverables:
  - Strategy registry (token, substring, embedding, action-verb).
  - TOML config for thresholds/weights/order; curated overrides data-driven.
  - HTTP/WS/mcp URL selection policy in config.
- Acceptance:
  - Demo toggles strategies without code edits; logs show strategy contributions.

4) Orchestrator rewrite (pushdown + chain collapse)
- Deliverables:
  - Rewriter that detects filterable MCP tools (e.g., label/q/query params; GraphQL).
  - Push filters server-side when supported; preserve local fallback.
  - Collapse chains of local primitives into a single synthesized step when semantics allow.
- Acceptance:
  - Reduced roundtrips/payloads; same final outputs; rewrite proofs logged.

5) LLM RTFS synthesis mode
- Deliverables:
  - Compact RTFS grammar hints (forms allowed; stdlib signatures; prelude helpers).
  - Validator loop: parse → analyze → test on sample → auto-repair (1–2 turns) → register.
  - Minimal prompt/playbooks for filter/map and custom transforms.
- Acceptance:
  - At least two synthesized capabilities produced by LLM pass validation and tests.

6) Canonical RTFS loader
- Deliverables:
  - Unified loader for `(capability ...)` and `(plan ...)` (no heuristics).
  - JSON ingestion remains auxiliary; RTFS is the primary format.
- Acceptance:
  - All discovered/generated manifests load via canonical loader.

7) I/O aliaser normalization
- Deliverables:
  - Alias layer mapping plan I/O to canonical internal names (e.g., items/predicate).
  - Reverse mapping at orchestrator boundaries to requested keys.
- Acceptance:
  - Synthesizer templates never guess names; bindings are explicit and reversible.

8) Tracing and auth gating UX
- Deliverables:
  - Structured tracing for strategies, bindings, rewrite decisions.
  - Typed auth-required errors with env-var guidance; policy to skip/continue.
  - Planner loop enforces capability schema compliance and feeds corrective prompts when bindings are invalid.
  - Structured timeline demo (`smart_assistant_viz`) renders collapsible discovery events with MCP/LLM details for easier triage.
- Acceptance:
  - Demo output is concise but debuggable; CI can assert on typed errors.

### Planner Visualization Workstream (smart_assistant_planner_viz.rs)

**Status**
- Capability catalog preloading & menu rendering wired to `smart_assistant_planner_viz`.
- Planner now validates LLM-proposed steps against capability schemas (required/optional inputs) and re-prompts with corrective feedback (max 3 attempts).
- Architecture summary appended to planner output for quick diagnostics.

**Open Tasks**
- Extend schema validation to check basic type compatibility (string vs vector) to prevent label/filters mismatches.
- Incorporate override metadata (aliases, heuristics) into menu display so the LLM sees canonical parameters.
- Add negative tests ensuring invalid plans fail gracefully and log corrective feedback.
- Capture successful plans into plan archive and surface them in catalog for reuse.
- Evaluate fallback flow for empty menus: prompt LLM to request capability synthesis or broaden search tokens.

**Next Steps**
1. Harden capability menu entries with manifest metadata (auth requirements, rate limits).
2. Teach planner to fall back to local primitives (e.g., filter) when remote capability lacks required parameters.
3. Instrument the schema validator with structured telemetry for tracing pipeline decisions.

9) Test suite
- Deliverables:
  - Unit: primitive synthesis & execution; schema binding cases.
  - Integration: demo run (with/without MCP auth), rewrite on/off.
  - Fuzz/static: analyzers over synthesized RTFS to enforce safety.
- Acceptance:
  - CI green; coverage on primitives > 80%.

10) Documentation
- Deliverables:
  - Developer guide: primitive framework, bindings, RTFS hints, rewrite strategies.
  - User guide: configuring discovery strategies; enabling LLM synthesis.
- Acceptance:
  - Docs reviewed; examples runnable.

### Generic Primitives (Design Sketch)
Common constraints for all:
- RTFS **SecureStandardLibrary** only (pure; immutable). Do not load the full `StandardLibrary` or CCOS prelude when executing synthesized code; they carry effectful helpers.
- Input/Output schemas declared as RTFS type expressions; outputs must match the requesting plan’s keys exactly.
- Execution in a restricted RTFS runtime initialized from `SecureStandardLibrary::create_secure_environment()`, plus a thin wrapper for orchestrator-specific helpers.

1) filter
- Inputs:
  - items: [:vector :any] or [:vector [:map ...]] (or map-of)
  - predicate: one of:
    - stringContains on one or more fields (simple mode)
    - boolean lambda expressed in RTFS over item (advanced mode; optional)
  - optional selector: list of fields to inspect in simple mode
- Output: items’ shape, filtered (same element type)
- Template: stable RTFS that lowercases search string and inspected fields; short-circuits on empty search

2) map
- Inputs:
  - items: vector
  - mapper: RTFS lambda from element → element’ (declared type)
- Output: vector of mapped elements

3) reduce
- Inputs:
  - items: vector
  - init: accumulator value
  - reducer: RTFS lambda (acc, item) → acc’
- Output: reduced value

4) project (select)
- Inputs:
  - items: vector of maps
  - fields: vector of keywords (the kept keys)
- Output: vector of maps with only requested keys

5) sort
- Inputs:
  - items: vector
  - key: keyword or lambda item → comparable
  - order: :asc | :desc
- Output: sorted vector

6) groupBy
- Inputs:
  - items: vector
  - key: keyword or lambda item → key
- Output: map key → vector of items

7) join
- Inputs:
  - left: vector of maps
  - right: vector of maps
  - on: [:pair leftKey rightKey] or lambdas
  - type: :inner | :left | :right | :full
- Output: vector of merged maps (schema merge with conflict policy)

### Binding and Naming
- Binding uses plan input/output schemas and (if MCP) remote schemas. No substring heuristics.
- Canonical internal names (items, predicate, mapper, accumulator, fields, key, order, groups).
- Output keys must match the plan’s requested keys exactly; aliaser handles remapping.

### RTFS-Safe Execution
- Build the execution environment by cloning `SecureStandardLibrary::create_secure_environment()`; do **not** register `StandardLibrary` or prelude functions.
- Whitelist: the secure stdlib forms already present (arithmetic, comparison, boolean logic, string ops, collection ops incl. `map`/`filter`/`reduce`/`sort`/`sort-by`, `assoc`/`dissoc`/`merge`, `get`/`get-in`, `every?`/`some?`, `distinct`, `frequencies`, `range`, `partition`, etc.), plus syntax forms (`let`, `if`, `fn`).
- Deny: host calls, prelude helpers (`tool/log`, `call`, kv/assoc!, etc.), microVM providers, IO/network functions, dynamic eval or module import.
- Static analyzers enforce: no mutation, no side-effects, bounded recursion/looping, only secure stdlib symbol usage.

### LLM Synthesis Mode (Optional Path)
- Provide a concise “RTFS grammar primer” and allowed stdlib signatures.
- Require output: (capability ...) with :input-schema, :output-schema, :implementation (fn [input]) and a small test vector.
- Validation loop: parse → analyze → run tests → auto-repair with structured feedback → register.

### Orchestrator Rewrite
- Pushdown filters/fields when MCP tool supports parameters (label/query/GraphQL selection).
- Collapse adjacent local primitives into one synthesized function when semantics allow (e.g., map→filter→project).
- Cache expensive calls; maintain functional purity at the plan level.

### Configuration Surfaces
- discovery.match.strategies = [token, substring, embedding, actionVerb]
- discovery.match.weights.token = 0.4, …; thresholds
- discovery.overrides = file/url; selection policy for remotes
- synthesis.enabled_primitives = [filter, map, …]
- execution.restricted_runtime = true; analyzers.enabled = [purity, denylist, depthLimit]
- orchestrator.rewrite = {pushdown: true, collapseChains: true}

### Migration & Compatibility
- Keep existing demos working; introduce features behind flags with sensible defaults.
- Convert existing discovered RTFS to load via canonical loader.

### Task List (linked to internal TODOs)
- Draft generalization plan doc with tasks and primitives spec (this file)
- Refactor LocalSynth into schema-aware primitive framework
- Add safe RTFS execution for synthesized capabilities
- Externalize discovery match strategies to config (weights/thresholds)
- Implement orchestrator rewrite (filter pushdown, chain collapse)
- Replace heuristic RTFS loader with canonical parser for (capability)/(plan)
- Introduce I/O aliaser normalization layer for plan-capability binding
- Add LLM RTFS synthesis mode with grammar hints and validator loop
- Improve tracing/auth gating UX for MCP capabilities
- Testing: unit primitives, integration demo runs, fuzz static analyzers
- Documentation: user/developer guides for primitives and planner rewrite

### Acceptance
End-to-end demo:
- Without auth: graceful typed failure on MCP step; local steps computed.
- With auth: server-side pushdown where possible; fewer steps; same output.
- Config toggles change discovery behavior and rewrite choices without code edits.


