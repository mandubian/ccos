# Modular Planner Heuristics Inventory

> **Goal**: Minimize hardcoded heuristics and progressively rely more on LLM cognitive capabilities.

This document catalogs all pattern-based heuristics, prefix/suffix matching, and hardcoded logic in the modular planner. Each heuristic should be evaluated for replacement with LLM-based alternatives.

---

## 1. Goal Decomposition Patterns (`decomposition/pattern.rs`)


**Purpose**: Recognize common goal structures without needing an LLM call.

| Pattern | Regex | Description |
|---------|-------|-------------|
| `X but ask me for Y` | `(?i)^(.+?)\s+but\s+(?:ask|prompt)...` | Split into user prompt + action |
| `ask me for X then Y` | `(?i)^(?:ask|prompt)\s+(?:me|user)...` | User input dependency |
| `X then Y` | `(?i)^(.+?)\s+(?:and\s+)?then\s+(.+)$` | Sequential action chain |
| `X and filter/sort by Y` | `(?i)^(.+?)\s+and\s+(filter|sort|...)` | Action with post-processing |

> [!NOTE]
> **Domain-specific code REMOVED ✅:**
> - `OWNER_REPO_REGEX` - Removed from pattern.rs
> - `extract_common_params()` - Removed from pattern.rs
> - `DomainHint::{GitHub, Slack, ...}` - Replaced with `DomainHint::Custom(String)`
>
> **Replacement**: Domain inference now uses `config/domain_hints.toml` via `domain_config.rs`. New domains can be added without code changes.

**LLM Replacement**: Intent-first decomposition already uses LLM. Domain-specific logic moved to config.


---

## 2. Action Verb → CRUD Mapping (`types.rs`, `mcp.rs`)

> [!NOTE]
> **Removed ✅** - Prefix-based action inference removed from `ToolSummary::new()`.
> Now defaults to `Other(name)`, caller uses `.with_action()` if known.
> LLM infers action from tool description during decomposition.

~~**Purpose**: Infer action type from capability/tool name prefixes.~~

```rust
"list_", "get_all"     → List
"get_", "read_"        → Get  
"create_", "add_"      → Create
"update_", "edit_"     → Update
"delete_", "remove_"   → Delete
"search_", "find_"     → Search
```

---

## 3. Capability Scoring Heuristics (`resolution/catalog.rs:462`)

**Purpose**: Score capability match when embeddings unavailable.

**Scoring factors**:
- +0.5 for action word match (list→list, get→get)
- +0.3 for domain/noun word overlap
- +0.2 for parameter name match
- -0.8 penalty for CRUD type mismatch

**Hybrid mode**: 70% embedding score + 30% heuristic boost

**LLM Replacement**: Use LLM to rank capabilities with natural language comparison. Already have `ScoringMethod::Embedding` as primary.

---

## 4. Safe Execution Patterns (`safe_executor.rs`)

> [!NOTE]
> **Removed ✅** - Pattern-based safety guess removed.
> Now requires explicit `effects` metadata in capability manifest.
> If no effects declared: execution blocked (don't guess).

~~**Purpose**: Determine if capability is safe to execute without side effects.~~

```rust
// REMOVED - these were fragile heuristics
["list_", "search_", "get_", ".list", ".search", ".get"]
```

---

## 5. RTFS Sanitization (`orchestrator.rs:147-175`)

**Purpose**: Fix common LLM-generated RTFS syntax errors.

**Transformations**:
| LLM Output | Corrected |
|------------|-----------|
| `str/split` | `str-split` |
| `clojure.string/join` | `str-join` |
| `#"pattern"` | `"pattern"` (regex literal) |
| `#(...)`     | `(...)` (anonymous fn) |

**LLM Replacement**: Better RTFS examples in prompt would reduce these errors. Consider expanding RTFS grammar to accept common Clojure-isms.

---

## 6. Type Mismatch Repair Rules (`repair_rules.rs`)

**Purpose**: Pattern-based fixes for runtime type errors.

| Error Pattern | Repair Action |
|---------------|---------------|
| `expected map, got vector with keyword` | Unwrap `(get step_N :key)` → `step_N` |
| `Type error: expected keyword, got string` | Wrap value in `(str ...)` |
| `expected keyword, got Number` | Wrap value in `(str ...)` |

**LLM Replacement**: Dialog-based repair (already implemented in `llm_repair_runtime_error`). Pattern rules serve as fast-path before LLM dialog.

---

## 7. Parameter Inference Heuristics (`orchestrator.rs:276-290`)

**Purpose**: Normalize capability IDs to canonical names.

```rust
// Common variations mapped to standard forms
"_previous_result" → Preserved as-is
names starting with "_" → Excluded from RTFS params
```

**LLM Replacement**: Include parameter mapping in capability resolution prompt.

---

## 8. Pending Capability Detection (`orchestrator.rs:1060-1086`)

**Purpose**: Detect if plan references unimplemented capabilities.

```rust
// Check for placeholder patterns
id.starts_with("generated/") || id.starts_with("pending/")
suggested_action.contains("Synth-or-enqueue")
```

**LLM Replacement**: Not replaceable - this is structural detection, not semantic inference.

---

## Recommendations

### Keep (Structural, Fast-Path)
1. Pending capability detection
2. RTFS sanitization (until grammar expanded)
3. Pattern-based repairs (fast fallback before LLM dialog)

### Replace with LLM
1. **Goal decomposition patterns** → Already have intent-first LLM decomposition
2. **Action verb mapping** → Include in introspection prompt
3. **Capability scoring** → Already have embedding mode
4. **Safe execution classification** → Have LLM classify or require effect metadata

### Hybrid Approach
1. Use heuristics as **fast-path optimizations**
2. Fall back to **LLM for ambiguous cases**
3. Track heuristic accuracy vs LLM accuracy to guide deprecation

---

## Migration Path

### Phase 1: Audit
- [x] Document all heuristics (this doc)

### Phase 2: Domain Configuration (COMPLETE ✅)
- [x] Create `config/domain_hints.toml` for domain definitions
- [x] Create `domain_config.rs` loader module  
- [x] Simplify `DomainHint` to `Generic` + `Custom(String)`
- [x] Remove `OWNER_REPO_REGEX` and `extract_common_params`
- [x] Update all usages and tests

### Phase 3: Instrument
- [ ] Add telemetry to track heuristic usage
- [ ] Log when heuristic conflicts with LLM decision

### Phase 4: Gradual Replacement
- [ ] Replace lowest-confidence heuristics first
- [ ] A/B test LLM-only vs hybrid approaches
- [ ] Remove heuristics that LLM handles better

### Phase 5: Simplification
- [ ] Move surviving heuristics to explicit config
- [ ] Document remaining heuristics as "performance optimizations"
