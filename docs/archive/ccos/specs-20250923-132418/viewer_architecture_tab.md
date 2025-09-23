# CCOS Viewer Architecture Tab Specification (v1)

Status: Draft (v1 design) – Baseline subset implemented (updated with heuristics + parent backfill)
Author: Automated Agent
Date: 2025-09-14
Version: 1 (post-heuristics update)
Target Release: M3+ (Observability Enhancement)

## 1. Purpose
Provide an in-viewer, live, read-only architectural snapshot of the CCOS runtime currently embedded in the viewer server. This feature enables:
- Faster comprehension of active subsystems (Arbiter, DelegatingArbiter, GovernanceKernel, Orchestrator, IntentGraph, CausalChain, CapabilityMarketplace, PlanArchive, RTFS runtime, AgentRegistry).
- Observability: counts (intents, plans, capabilities), delegation state, isolation policy, security posture.
- Educational transparency: how a natural language request flows through CCOS.
- Future foundation for: dynamic updates, capability provenance inspection, security heatmaps, governance audits.

No secrets or raw credentials are ever exposed. API keys and sensitive token values are replaced with qualitative flags (e.g. `"OPENAI_API_KEY": "SET"`). Heuristic meta warnings surface potential anomalies (see Section 14 updates).

## 2. Scope (v1)
Included:
- HTTP endpoint: `GET /architecture` (snapshot JSON)
- Frontend tab: "Architecture" with component graph + inspector panels
- Capability summary + (optional) full capability listing (behind `?include=capabilities`)
- Isolation policy + allowed/denied pattern lists
- Delegation / LLM model summary (no keys)
- Intent + plan + causal chain aggregate metrics (recent samples)
- Download raw snapshot JSON (client side)

Excluded (future):
- Streaming real-time architecture deltas
- Mutating policies or configurations
- Deep provenance attestation graph
- Capability execution metrics / latency histograms
- Live agent feedback dashboard

## 3. Architecture Flow Model (Conceptual)
```
Natural Language → Arbiter/DelegatingArbiter ──proposes plan──▶ GovernanceKernel
         ▲                                            │
         │                                            ▼
     IntentGraph ◀── creates / updates ── Orchestrator ── validated plan executes ──▶ CapabilityMarketplace → Capabilities
         │                                                   │                              │
         │                                                   ├── records actions ───────────┘
         │                                                   ▼
         └────────── referenced / audited ───────────▶ CausalChain
```

## 4. Endpoint Contract: `GET /architecture`
### 4.1 Query Parameters
- `include` (optional, CSV): supports `capabilities`
- `recent_intents` (int, default 5, max 50)
- `cap_limit` (int, optional hard cap on capability listing; server may truncate)

### 4.2 Response Schema (Top-Level)
```
{
  version: "1",
  generated_at: ISO8601,
  instance: { pid?, build?: { git_commit?, profile } },
  environment: { delegation_enabled, llm: { provider, model, available, timeout_seconds?, max_tokens?, temperature? }, flags: { ...subset } },
  components: { ...subsections },
  security: { isolation_policy, default_runtime_context },
  capabilities?: [ CapabilitySummary ],
  delegation: { enabled, model?, provider?, recent_delegated_intents? },
  graph_model: { nodes: [ArchNode], flow_edges: [ArchEdge] },
  meta: { degraded?: boolean, warnings?: [string] } // warnings populated by heuristic detectors
}
```

### 4.3 Component Subsections
- `intent_graph`: `{ total, active, completed, failed, recent: [{id, goal, status}], leaf_count?, root_count? }` (root/leaf derived from `IsSubgoalOf` edges; parent backfill best‑effort performed before counting)
- `causal_chain`: `{ events_total?, recent_events?: [ {type, ts} ], type_counts_last_window?: {Type: count}, unavailable?: true }`
- `capability_marketplace`: `{ total, by_provider_type: {local,http,mcp,a2a,plugin?}, namespaces: {ns: count}, sample_allowed_ratio }`
- `plan_archive`: `{ stored_plans }`
- `rtfs_runtime`: `{ strategy, modules: [name] }`
- `agent_registry`: `{ agents, top?: [ {agent_id, score} ] }`

### 4.4 CapabilitySummary
```
{
  id: string,
  namespace: string,
  provider_type: "local"|"http"|"mcp"|"a2a"|"plugin",
  version?: string,
  allowed_by_policy: boolean
}
```

### 4.5 Isolation Policy Snapshot
```
{
  allowed_patterns: [string],
  denied_patterns: [string],
  namespaces_policy?: { namespace: { allow: bool? } },
  time_constraints_active: boolean
}
```

### 4.6 Graph Model
```
ArchNode: { id, label, group, present?: bool, metrics?: { key: number } }
ArchEdge: { from, to, relation }

New (post v1 incremental update):
- Optional `user_goal` node (group: `input`) injected when at least one recent intent exists; label = truncated first recent intent goal (<=80 chars).
- Delegation path nodes/edges: when delegation enabled and provider known: `delegating_arbiter` (present flag) plus dynamic LLM provider node (`llm_provider_<provider>`; group: `llm`) with edges:
  - `arbiter -> delegating_arbiter` (relation: `delegates`)
  - `delegating_arbiter -> llm_provider_<provider>` (relation: `llm_call`)
  - `user_goal -> arbiter` (relation: `submits_goal`) when goal node present.

Color / layout semantics updated in frontend (hierarchical LR layout) to visually separate input → arbitration → governance → orchestration → data stores.
```

### 4.7 Error / Degradation Handling
If a subsection fails (lock poisoned, etc.), return placeholder:
```
intent_graph: { degraded: true, error: "lock" }
```
and set top-level `meta.degraded = true`.

## 5. Security & Privacy Considerations
- Never expose API keys or tokens; only boolean flags.
- Capability provider internal handler types are not serialized.
- HTTP capability `auth_token` omitted entirely.
- Limit high cardinality outputs via caps & optional inclusion flag.
- All data read-only.

## 6. Frontend UX (v1)
### 6.1 Layout
- Navigation adds: `Architecture` tab (sibling to existing main view)
- Split view: left = vis.js system diagram; right = inspector with sub-tabs:
  1. Overview
  2. Capabilities (filter: namespace, provider type, search)
  3. Security & Governance
  4. Delegation
  5. Causal Chain
  6. Runtime

### 6.2 Interactions
- Selecting node highlights related edges; inspector auto-focuses relevant tab.
- Refresh button + Auto-refresh toggle (15s). Auto refresh off by default.
- Export button downloads current JSON snapshot.
- Capability table lazy-renders (truncate > 300 rows with "Load more").

### 6.3 Visual Encoding
Group → Color theme (consistent, accessible). Status badges (ok / missing / degraded). Edge tooltips show relation string.

## 7. Implementation Plan
This section distinguishes between the original design intent and the pragmatic baseline now in the repository.

Planned (v1 design) vs Implemented (Baseline) legend:
- ✔ implemented
- △ partially implemented
- ☐ not yet implemented

1. Doc (this file) ✔
2. Sanitizing helper methods:
  - Capability marketplace public snapshot helpers ✔
  - Isolation policy snapshot helper ✔
3. (Design) Direct `Arc<CCOS>` in Axum `AppState` ☐  → (Implemented) Channel-based request pattern (`ArchitectureRequestInternal` sent over mpsc to worker owning non-`Send` CCOS) ✔
4. `architecture_handler` issues oneshot request and awaits snapshot ✔
5. Route registration `.route("/architecture", get(architecture_handler))` ✔
6. Frontend Architecture tab:
  - Basic tab + graph canvas + simple inspector ✔
  - Filters (namespace/provider) ☐
  - Capability search ☐
  - Export JSON button ☐
  - Auto-refresh toggle ☐
  - Sub‑tabs (Overview / Capabilities / Security / Delegation / Chain / Runtime) ☐ (current inspector is minimal)
7. Basic tests / smoke △ (manual build only; automated test pending)
8. WebSocket `ArchitectureUpdate` events ☐ (future)

Rationale for deviation at Step 3: Several CCOS subsystems are not `Send` due to contained types; moving them into Axum state would require broad refactors. The channel + worker thread (Tokio current-thread runtime with `LocalSet`) preserves thread-affinity while providing safe, asynchronous request/response semantics.

## 8. Data Retrieval Strategy
- IntentGraph: lock → copy minimal intent structs (id, goal, status, created_at)
- CausalChain: if public API insufficient → mark unavailable.
- PlanArchive: orchestrator accessor (count only if iteration cheap; else maintain counter later).
- Capabilities: read lock on marketplace `capabilities` map; compute per-namespace counts.
- Isolation Policy: clone pattern arrays.
- Delegation: check `get_delegating_arbiter()` presence; extract model config from arbiter config if accessible (fallback to env values already logged).

## 9. Performance Considerations
- Single snapshot call expected to be light (<10 ms for typical counts).
- Optional exclusion of large capability array reduces payload.
- Future optimization: precomputed caches updated on mutation events.

## 10. JSON Example (Abbreviated)
```json
{
  "version": "1",
  "generated_at": "2025-09-14T10:22:33Z",
  "environment": {"delegation_enabled": true, "llm": {"provider": "openrouter", "model": "moonshotai/kimi-k2:free", "available": true}},
  "components": {"intent_graph": {"total": 12, "active": 10, "completed": 1, "failed": 1}},
  "capability_marketplace": {"total": 58, "by_provider_type": {"local": 50, "http": 4}},
  "security": {"isolation_policy": {"allowed_patterns": ["ccos.*"], "denied_patterns": []}},
  "graph_model": {"nodes": [{"id": "arbiter", "label": "Arbiter", "group": "arbiter"}], "flow_edges": []}
}
```

## 11. Future Roadmap
- v1.1: Streaming diffs, capability provenance expansion.
- v1.2: Performance metrics (plan validation time histograms).
- v1.3: Security heatmap & policy simulation tool.
- v1.4: Delegation agent feedback analytics & trust scores.

## 12. Open Questions / TBD
- Expose causal chain events publicly? (needs accessor) – if not, mark gracefully.
- Standardize build metadata (commit hash injection) – out of scope v1.
- Plan archive enumeration may require adding a public API if not present.

## 13. Acceptance Criteria
- Endpoint returns 200 JSON with `version`, `graph_model.nodes` non-empty.
- No secrets present in payload.
- Frontend renders diagram + capability counts.
- Capability filtering works for namespace + provider type.
- Fallback states (no delegation, missing chain) do not break UI.

## 14. Implementation Status & Deltas

| Feature Area | Designed (v1) | Baseline Implemented | Notes / Follow-up |
|--------------|---------------|----------------------|-------------------|
| Backend snapshot mechanism | Direct access via Axum state | Channel to worker thread | Keeps non-Send CCOS confined |
| Endpoint `/architecture` | Yes | Yes | All remaining documented params honored; `recent_chain` removed pending chain metrics support |
| Capability listing toggle | `include=capabilities` | Yes | Truncation limits TBD |
| Isolation policy exposure | Allowed/denied patterns | Yes | Namespaced policies included if available |
| Delegation / LLM summary | Provider, model, enabled flag | Yes | Derived from env + presence checks |
| IntentGraph metrics | Totals + recent list | Yes | Recent intents limit enforced; parent/child hierarchy derived from edges with best‑effort parent backfill |
| CausalChain metrics | Recent events + counts | Placeholder | Marked unavailable when no accessor |
| PlanArchive metrics | Count | Partial | May require accessor to expose count reliably |
| Graph visualization | Nodes + flow edges | Yes | Static layout; future dynamic styling |
| Inspector sub-tabs | 6 planned tabs | Basic tabs implemented (Overview, Capabilities, Security, Delegation, Chain, Runtime) | Panels show raw JSON / summary; future richer UX |
| Capability filters/search | Namespace/provider filters + search | Implemented (client-side filtering + paging) | Performance OK for moderate lists; consider server pagination later |
| Export JSON | Download button | Implemented (blob download) | Filename includes timestamp |
| Auto-refresh | Toggle (15s) | Implemented | Only refreshes when Architecture view active |
| WebSocket streaming | Incremental deltas | Not implemented | Requires server push channel |
| Automated tests | Snapshot schema & no secrets | Partial | Integration test validates invariants (version, node count, recent limit, secret redaction) |

### Known Discrepancies
1. Causal chain metrics not yet implemented (no public accessor). Parameter `recent_chain` deliberately removed to avoid confusion; will be reintroduced alongside chain metrics.
2. Performance metrics not yet surfaced.
3. No explicit test for degraded-mode paths yet (e.g. forced lock failure simulation).

### Heuristic Warning Rules (Implemented)
| Condition | Warning String | Rationale |
|-----------|----------------|-----------|
| `total > 5 && root_count == total` | `All intents appear as roots (parent linkage may be missing)` | Indicates missing or unlinked hierarchy edges |
| `total > 5 && active/total > 0.8` | `High proportion of active intents (>80%)` | Potential runaway or stuck processing surge |
| Isolation policy exactly `allowed_patterns=["*"]` and empty denies | `Isolation policy is fully permissive ('*')` | Highlights broad access risk |

### 15. UI Rendering of Warnings & Degraded State (Update)
The frontend now surfaces snapshot heuristic warnings and degradation status:

| UI Element | Behavior |
|------------|----------|
| Status Badge (`#architecture-status`) | Shows `OK`, `N warnings`, or `Degraded`. Applies CSS classes: `status-ok`, `status-warning`, `status-degraded`. |
| Overview Grid | Adds `Degraded:` boolean and `Warnings:` count fields. |
| Warnings Panel | If any warnings, a block `.arch-warnings` containing a titled list appears below the overview grid. |

Rendering Logic Summary:
1. When snapshot `meta.degraded == true` → badge text `Degraded` (highest precedence).
2. Else if `meta.warnings.length > 0` → badge text like `3 warnings`.
3. Else → badge text `OK`.
4. The warnings list is escaped for HTML safety and displayed as list items.

Future Enhancements:
- Collapse/expand control for long warning lists (>5).
- Severity tiers (info/warn/critical) via structured rule metadata.
- Inline links from warning entries to contextual inspector tabs (e.g., clicking isolation warning focuses Security tab).

Warnings accumulate; presence of any sets `meta.warnings`. Internal lock / update failures set `meta.degraded=true` and append specific failure reasons.

### Parent Backfill Strategy
During snapshot computation and graph generation flows, missing `parent_intent` fields are opportunistically backfilled using the first observed `IsSubgoalOf` edge (`from` child → `to` parent). Logic is best‑effort and does not currently resolve conflicting parents; future enhancement may record multiple lineage possibilities.

### Immediate Next Steps
1. Add integration test hitting `/architecture` asserting: status 200, `version=="1"`, non-empty `graph_model.nodes`, no raw secrets, recent intents length obeys `recent_intents` cap.
2. Introduce causal chain accessor & metrics (then reintroduce a `recent_chain` style parameter).
3. Add performance metrics & warning badge logic (e.g., degraded component highlighting).

## 15. Navigation Decision (Update 2025-09-14)
User research & UX evaluation determined the Architecture view should be promoted from a nested panel/tab inside the RTFS/Intent viewer to a top-level navigation peer. Rationale:
- Clear separation of concerns (intent lifecycle vs. platform observability)
- Prevent visual overcrowding in the intent panel layout
- Enables future expansion (streaming metrics, security heatmaps) without destabilizing core intent workflows
- Permits direct deep-linking via URL hash (#architecture) for docs & onboarding

### Phase 1 Refactor Scope
Minimal structural change; no new functional features:
1. Add top-level nav with two primary entries: Intents (existing main view) and Architecture.
2. Promote existing architecture DOM subtree to its own top-level `<section id="view-architecture">` sibling of the intents section.
3. Add hash-based routing: `#intents` (default) and `#architecture`.
4. Generic tab/view activation logic consolidating previous ad-hoc button handlers.
5. Pause any future auto-refresh intervals when switching away (placeholder for later phases).

### Non-Goals (Phase 1)
- Adding inspector sub-tabs
- Adding filters/search/export/auto-refresh
- Streaming updates

### Acceptance (Phase 1)
- Direct navigation to `#architecture` shows architecture view without flash of intents content.
- Switching views does not re-trigger an already loaded architecture snapshot unless manually refreshed.
- No regression to existing intent workflows.

### Follow-up Phases (Brief)
- Phase 2: Inspector sub-tabs + filters/search/export/auto-refresh.
- Phase 3: Streaming & performance overlays.

---
Change Log (Spec)
- 2025-09-14: Added Section 15 documenting top-level navigation decision and Phase 1 refactor scope.
- 2025-09-14: Removed undocumented/unused `recent_chain` param (pending causal chain metrics) and updated discrepancies/next steps.

### Risk & Mitigation
| Risk | Impact | Mitigation |
|------|--------|------------|
| Non-Send components block future parallel endpoints | Medium | Reuse channel pattern; abstract request types for reuse |
| Snapshot payload growth (many capabilities) | Performance / latency | Add server-side truncation + pagination parameters |
| UI drift vs spec | Confusion | Maintain this delta section; update on each milestone |

---
Change Log (Spec)
- 2025-09-14: Added Section 14 with implementation deltas and updated Implementation Plan to reflect channel-based approach.

---
End of Specification
