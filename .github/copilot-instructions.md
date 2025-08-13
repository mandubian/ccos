# CCOS + RTFS Common Rules (Authoring & Contribution Guide for AI Agents)

Purpose: Single, high‑signal rule sheet to make an AI (or human) productive fast across CCOS (governed cognitive OS) + RTFS (its homoiconic planning language). Keep this open while editing. Link deeper specs where needed.

---
## 0. Read/Scan Order (10–15 min Boot Sequence)
1. `.github/copilot-instructions.md` (this repo’s delta + delegation status)
2. This file (ruleset)
3. `rtfs_compiler/src/ccos/mod.rs` (system assembly + `process_request` pipeline)
4. Specs indices:
   - CCOS specs: `docs/ccos/specs/` (start with any numbered overview + delegation / governance specs)
   - RTFS 2.0 language specs: `docs/rtfs-2.0/specs/` (grammar + semantic rules)
5. `causal_chain.rs`, `governance_kernel.rs`, `delegating_arbiter.rs`, `agent_registry.rs`
6. Capability examples: `runtime/stdlib` registration + any custom capability modules.

---
## 1. Separation of Powers (Never Break This Boundary)
| Component | Privilege | File | Responsibility |
|-----------|-----------|------|----------------|
| Arbiter | Low | `arbiter.rs` | NL → Intent & baseline Plan proposal only |
| DelegatingArbiter | Low (augmented) | `delegating_arbiter.rs` | Heuristic agent delegation + LLM fallback |
| GovernanceKernel | High | `governance_kernel.rs` | Sanitize → Scaffold → Constitutional validate |
| Orchestrator | Medium | `orchestrator.rs` | Deterministic Plan execution via marketplace |
| CapabilityMarketplace | Broker | `runtime/capability_marketplace.rs` | Capability discovery + invocation indirection |
| CausalChain | Immutable Ledger | `causal_chain.rs` | Signed Action append + provenance + metrics + delegation events |
| IntentGraph | Store | `intent_graph.rs` | Intent lifecycle persistence + search |
| AgentRegistry (M4) | Advisory | `agent_registry.rs` | Candidate scoring & metadata for delegation |

Never allow Arbiter/DelegatingArbiter to execute side effects directly; all side effects travel Plan → GovernanceKernel → Orchestrator → CapabilityMarketplace.

---
## 2. RTFS Language Operational Subset (What You Can Safely Generate Now)
- Root form for executable plan body: `(do ...)` only.
- Side effects exclusively via `(call :cap.namespace:vN.op { ... })` – no hidden helpers.
- Wrap auditable operations with `(step "description" <expr>)` (even if currently partially implemented) to future‑proof ledger semantics.
- Avoid introducing unsupported special forms without updating parser + specs.
- Use descriptive intent goal strings; attach constraints via metadata or explicit constraint keys when specs allow.
- Keep plan simple & pure except for `(call ...)` forms. No direct I/O primitives exist in core language.

Reference: RTFS grammar & semantics specs under `docs/rtfs-2.0/specs/` (start with grammar overview + evaluation model documents).

---
## 3. Governance Flow (Golden Path)
1. Arbiter/DelegatingArbiter builds Intent; stores in IntentGraph.
2. Plan proposed (or short‑circuited by delegation).
3. `CCOS::preflight_validate_capabilities` performs naive capability token existence check.
4. GovernanceKernel:
   - `sanitize_intent` – injection & contradiction filters.
   - `scaffold_plan` – ensure `(do ...)` wrap only.
   - `validate_against_constitution` – extend for new rules (see CCOS governance specs directory).
5. Orchestrator executes; each capability invocation logged as `ActionType::CapabilityCall` with signature.
6. CausalChain signs & hashes action → immutable audit.

If you add a governance rule: implement in `governance_kernel.rs`, add targeted test, update spec file in `docs/ccos/specs/`.

---
## 4. Delegation (M4 State)
Implemented:
- Agent scoring → heuristic threshold (hardcoded 0.65) in `attempt_agent_delegation`.
- Approved delegation event: `record_delegation_event(..., "approved", meta)`.
Metadata keys: `delegation.selected_agent`, `delegation.rationale`, `delegation.candidates`.
Pending (when extending): `delegation.proposed`, `delegation.rejected`, `delegation.completed`; governance pre‑approval hook; configurable threshold (env `CCOS_DELEGATION_THRESHOLD`); post‑execution feedback updating success stats.

---
## 5. Capability Lifecycle (Do This Exactly)
1. Implement struct with async `execute(&self, ...) -> ExecutionResult`.
2. Register in `register_default_capabilities` (or dynamic path).
3. Refer only by id in Plan via `(call :your.capability:v1.op { ... })`.
4. Add integration test: run a request or synthetic plan; assert presence of `ActionType::CapabilityCall` with `function_name == capability id` in CausalChain.
5. Add minimal docs/spec note if novel side effect semantics under `docs/ccos/specs/` (capability guidelines file if present).
Never: hardcode direct network / file I/O inside planning path; route through capability.

---
## 6. Action / Ledger Integrity Rules
- Always add `signature` metadata before append (`signing.sign_action`).
- Do NOT reorder hashed fields in `calculate_action_hash` (see `causal_chain.rs`). Breaking changes require explicit migration plan.
- Add new audit data via `action.metadata` namespaces instead of struct fields (backwards compatibility, hash stability).
- Use `record_delegation_event` for delegation audit; do not manually craft unsinged delegation actions.

---
## 7. Error & Result Conventions
- Return `Result<T, RuntimeError>`; avoid `.unwrap()` / `.expect()` (cursor rules enforce warning).
- Deterministic, contextual error messages (mention offending capability / rule where safe).
- Surface governance denials early with explicit reason for audit chain.

---
## 8. Concurrency & Locks
- Use `Arc<Mutex<...>>` for CausalChain & IntentGraph; acquire, mutate, release quickly – no `.await` inside locked region.
- AgentRegistry uses `RwLock` (read‑heavy). Prefer read lock for scoring; write only for registration / feedback update.

---
## 9. Performance Guidelines
- Simple NL→Plan round trip target <1ms for trivial examples (excluding model latency).
- Avoid needless clones in tight loops (ledger hashing / indexing). Pass references where possible.
- Pre‑reserve collections if size known.

---
## 10. Testing & Commands
Core commands (run in `rtfs_compiler/`):
```
cargo run --bin rtfs-repl
cargo test
cargo test --test integration_tests -- --nocapture --test-threads 1
cargo build --release --bin rtfs-compiler
cargo run --bin rtfs-compiler -- --input file.rtfs --execute --show-timing --show-stats
```
Add new tests in `tests/` mirroring existing style; use env flags for delegation tests (`CCOS_USE_DELEGATING_ARBITER=1`). Keep ordering deterministic if inspecting ledger output (`--test-threads 1`).

---
## 11. Rust Style & Rules (Project-Specific)
Reflect `.cursor/rules/rust-rules.mdc`:
- Avoid `unwrap()/expect()` → propagate errors.
- Remove unused `mut` / prefix intentionally unused parameters with `_` (`_capability_id`).
- Minimize `.clone()`; only clone when ownership required; otherwise borrow.
- Use `Vec::with_capacity` when size predictable.
- Keep public surface documented (add `///` comments to new public items).
- Prefer explicit pattern matches; ensure exhaustiveness for enums that may expand.

---
## 12. Spec Link Map (Start Points)
(Do not inline entire specs here; navigate as needed.)
- CCOS Specs Index: `docs/ccos/specs/` (governance, delegation, capability marketplace, causal chain design docs).
- RTFS Language Specs: `docs/rtfs-2.0/specs/` (grammar, types, evaluation semantics, special forms, intent/plan/action object schemas).
- Archived / Historical (for context only): `docs/rtfs-1.0/`.

When adding or changing semantics: update appropriate spec file + reference commit hash in PR description.

---
## 13. Extension Checklist (Before Opening PR)
[ ] New capability registered & tested
[ ] No new Action struct fields (or hash updated + migration rationale documented)
[ ] Governance rule changes accompanied by spec update
[ ] Delegation changes emit proper events
[ ] No direct external side effects outside marketplace
[ ] Tests deterministic & pass locally

---
Need deeper drill‑down (grammar, delegation governance hook, capability example)? Create an issue or ask specifying the subsection number above.
