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
- Error & Result Conventions
    - Return `Result<T, RuntimeError>`; avoid `.unwrap()` / `.expect()` (cursor rules enforce warning).
    - Deterministic, contextual error messages (mention offending capability / rule where safe).
    - Surface governance denials early with explicit reason for audit chain.
- Concurrency & Locks
    - Use `Arc<Mutex<...>>` for CausalChain & IntentGraph; acquire, mutate, release quickly – no `.await` inside locked region.
    - AgentRegistry uses `RwLock` (read‑heavy). Prefer read lock for scoring; write only for registration / feedback update.

---

## 4. git and documentation
- commit: commit as soon as a big-enough feature (not a tiny fix or code update) is implemented, don't wait for the end of worktree or staging area. Use present tense, imperative mood, reference issues/PRs when relevant.
- PR: includea summary of changes, test results, known issues, and next steps.
- docs: update relevant docs in `docs/ccos/specs/` or `docs/rtfs-2.0/specs/` when changing behavior or adding features. For examples or guides, put it in `docs/ccos/guides` and `docs/rtfs-2.0/guides`. Link to these docs in your PR description.

---
## 5. IMPORTANT DOCS TO READ
- CCOS Specs Index: `docs/ccos/specs/` (arbiter, governance, delegation, capability marketplace, causal chain design docs).
- RTFS Language Specs: `docs/rtfs-2.0/specs/` (philosophy, grammar, types, runtime).
- Do not read docs in docs/archive/ except when explicitly asked (outdated).

---
## 6. IMPORTANT CODE TO READ
1. `rtfs_compiler/src/ccos/mod.rs` (system assembly + `process_request` pipeline)
2. Standard Lib with secure functions: grammar `rtfs_compiler/src/rtfs.pest` and secure stdlib `rtfs_compiler/src/runtime/secure_stdlib.rs` and insecure stdlib `rtfs_compiler/src/runtime/stdlib.rs`
3. Governance Kernel: `rtfs_compiler/src/ccos/governance_kernel.rs`
4. Arbiter: `rtfs_compiler/src/ccos/arbiter.rs`
5. Delegating Arbiter: `rtfs_compiler/src/ccos/delegating_arbiter.rs`
6. Orchestrator: `rtfs_compiler/src/ccos/orchestrator.rs`
7. Capability Marketplace: `rtfs_compiler/src/runtime/capability_marketplace.rs`
8. Causal Chain: `rtfs_compiler/src/ccos/causal_chain.rs`
9. Intent Graph: `rtfs_compiler/src/ccos/intent_graph.rs`
10. Agent Registry: `rtfs_compiler/src/ccos/agent_registry.rs`
