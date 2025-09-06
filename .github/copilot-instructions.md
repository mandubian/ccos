# CCOS + RTFS Common Rules (Authoring & Contribution Guide for AI Agents)

Purpose: Single, high‑signal rule sheet to make an AI (or human) productive fast across CCOS (governed cognitive OS) + RTFS (its homoiconic planning language). Keep this open while editing. Link deeper specs where needed.

---
## 0. foundation rules (ABSOLUTELY MANDATORY)

- YOU SHALL NOT try to echo or display in chat any variable from shell environment, secret or not. This is a security risk.

---
## 1. Give your decision in natural language before implementing anything (ABSOLUTELY MANDATORY)
Before making any changes, always provide a brief natural language explanation of your reasoning and the intended outcome. This helps ensure clarity and alignment with project goals. For example:
- "I am adding a new capability to handle user authentication because it is a common requirement for many applications."
- "I am refactoring the governance kernel to improve code readability and maintainability."

---
## 2. Testing & Commands
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
## 3. Rust Style & Rules (Project-Specific)
- Avoid `unwrap()/expect()` → propagate errors.
- Remove unused `mut` / prefix intentionally unused parameters with `_` (`_capability_id`).
- Minimize `.clone()`; only clone when ownership required; otherwise borrow.
- Use `Vec::with_capacity` when size predictable.
- Keep public surface documented (add `///` comments to new public items).
- Prefer explicit pattern matches; ensure exhaustiveness for enums that may expand.
- prefer module with `mod.rs` and several files will less than 1000 lines of code over flat structure; use `pub(crate)` for internal visibility.
- Avoid needless clones in tight loops (ledger hashing / indexing). Pass references where possible.
- Pre‑reserve collections if size known.
- Use `log` crate for logging; avoid `println!()`.


---
## 4. git and documentation
- commit: commit as soon as a feature is implemented, don't wait for the end of worktree. Use present tense, imperative mood, reference issues/PRs when relevant.
- PR: includea summary of changes, test results, known issues, and next steps.
- docs: update relevant docs in `docs/ccos/specs/` or `docs/rtfs-2.0/specs/` when changing behavior or adding features. Link to these docs in your PR description.

---
Need deeper drill‑down (grammar, delegation governance hook, capability example)? Create an issue or ask specifying the subsection number above.

---

## 5. IMPORTANT DOCS TO READ (10–15 min Boot Sequence)
- CCOS Specs Index: `docs/ccos/specs/` (governance, delegation, capability marketplace, causal chain design docs).
- RTFS Language Specs: `docs/rtfs-2.0/specs/` (grammar, types, evaluation semantics, special forms, intent/plan/action object schemas).
2. `rtfs_compiler/src/ccos/mod.rs` (system assembly + `process_request` pipeline)
3. Standard Lib with secure functions: `rtfs_compiler/src/runtime/secure_stdlib.rs` and insecure ones `rtfs_compiler/src/runtime/stdlib.rs`


When adding or changing semantics: update appropriate spec file + reference commit hash in PR description.

---
## 6. Separation of Powers (Never Break This Boundary)
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
## 7. RTFS Language Operational Subset (What You Can Safely Generate Now)
- Root form for executable plan body: `(do ...)` only.
- Side effects exclusively via `(call :cap.namespace:vN.op { ... })` – no hidden helpers.
- Wrap auditable operations with `(step "description" <expr>)` (even if currently partially implemented) to future‑proof ledger semantics.
- Avoid introducing unsupported special forms without updating parser + specs.
- Use descriptive intent goal strings; attach constraints via metadata or explicit constraint keys when specs allow.
- Keep plan simple & pure except for `(call ...)` forms. No direct I/O primitives exist in core language.

Reference: RTFS grammar & semantics specs under `docs/rtfs-2.0/specs/` (start with grammar overview + evaluation model documents).

---
## 8. Error & Result Conventions
- Return `Result<T, RuntimeError>`; avoid `.unwrap()` / `.expect()` (cursor rules enforce warning).
- Deterministic, contextual error messages (mention offending capability / rule where safe).
- Surface governance denials early with explicit reason for audit chain.

---
## 9. Concurrency & Locks
- Use `Arc<Mutex<...>>` for CausalChain & IntentGraph; acquire, mutate, release quickly – no `.await` inside locked region.
- AgentRegistry uses `RwLock` (read‑heavy). Prefer read lock for scoring; write only for registration / feedback update.
