
# Migration plan: Pure RTFS runtime, effects delegated to Host (feature-gated, reversible)

Status: In Progress — Phase 2 Complete, Ready for Phase 3
Audience: RTFS compiler/runtime owners, maintainers, release managers
Related: `docs/rtfs-2.0/specs-new/07-immutability-and-state.md`

Goal

Make the RTFS runtime pure/deterministic and delegate all effects and mutations to the Host via explicit capability calls and continuations. Maintain a feature-gated, deprecation-first path for legacy atom semantics to keep migration safe and reversible.

Rationale

The previous aggressive removal plan reduces time-to-completion but increases review and rollback complexity. This staged plan trades elapsed time for safety: each step is reviewable, reversible, and keeps the tree usable.

Plan overview (high level)

1. Discovery & inventory — find every use of `atom`, `set!`, `deref`, `reset!`, `swap!`, `Value::Atom` and reader-macro `@`.
2. Feature-gate legacy atoms — keep legacy behavior behind `legacy-atoms` while default builds forbid mutation with a clear error.
3. Introduce Host effect boundary — standardize ExecutionOutcome::RequiresHost(effect_request) and a typed effect_request schema.
4. Add continuations — make evaluation resumable after Host completes an effect (sync first, async later).
5. Provide Host-backed state primitives — versioned KV + CAS, counters, log/event append, all with ACLs and audit logs.
6. Migrate tests/examples — rewrite trivial cases to pure; replace complex mutation patterns with Host capability calls.
7. Final removal & cleanup — delete legacy atoms and docs when coverage is sufficient and CI is green.

Phases and concrete actions

Phase 0 — Prep: branch + communication (0.25–0.5 day)

- Create branch: `migration/remove-atoms-immutability`.
- Open an issue/PR that contains this plan and the migration checklist; tag owners and ask for reviewers.

Phase 1 — Discovery & inventory (0.25–1 day)

- Run a workspace search for the keywords and produce an inventory (CSV or markdown table) with file, lines, category (core, stdlib, tests, tools), and recommended action (deprecate, gate, refactor, remove).
- Add inventory to `docs/migration-notes/inventory-immutability.md`.

Phase 2 — Feature gate + compatibility shim (0.5–1 day)

- Add a cargo feature `legacy-atoms` (default = on initially on main; migration branch will flip it off in CI).
- Move all atom-related code paths (Value::Atom variant, stdlib functions, evaluator branches) behind `#[cfg(feature = "legacy-atoms")]` where practical.
- Provide compatibility shims when feature is off: these shims should either return a clear compile-time error, or a runtime error with an actionable message.

Acceptance criteria for Phase 2

- `cargo build` with `--features legacy-atoms` and without the feature both compile (the latter may fail only where shims exist but should fail with clear messages). CI can be configured to test both modes.

Phase 3 — Effect boundary and continuations (1–2 days)

- Define a typed `effect_request` envelope: capability_id, input payload, security_context, causal_context (intent_id/step_id), timeout_ms, idempotency_key.
- Ensure evaluator yields `ExecutionOutcome::RequiresHost(effect_request)` at effect sites and can resume with the Host result injected at the call site.
- Start synchronous resume (single-step) and document the async model for later.

Acceptance criteria for Phase 3

- Round‑trip demo: a simple program that requests `ccos.counter.inc` returns RequiresHost, Host mock processes it, evaluator resumes and completes with the incremented value.
- Idempotency key plumbed through for Host retries.

Phase 4 — Deprecation & docs (0.5 day)

- Mark stdlib functions `#[deprecated(note = "RTFS 2.0 removes atoms — use X instead")]` when feature enabled.
- Add deprecation notes in `docs/rtfs-2.0/specs/` and examples.

Phase 5 — Host-backed state and security (2–4 days, incremental)

- Provide minimal Host capabilities: `kv.get`, `kv.cas-put`, `counter.inc`, `event.append`.
- Enforce ACLs/quotas via arbiter; log all effects to an append‑only audit stream with causal metadata.
- Add timeouts, retry policy, and error taxonomy (retryable vs permanent).

Acceptance criteria for Phase 5

- Concurrency-safe increments verified via CAS or per-key serialization.
- Effect logs show intent/step IDs, inputs (redacted when needed), outcomes, and latency.

Phase 6 — Disable feature in CI on migration branch + fix (iterative)

- On migration branch, run CI with `--no-default-features --features ""` or explicitly with `--no-default-features` to simulate removal.
- Fix compile errors iteratively, preferring small commits that either refactor user code to the immutable model or implement adapter patterns.

Guidance for fixes

- Replace atom-backed counters and logs with Host capabilities: `counter.inc`, `event.append`.
- For shared identity/handles, prefer host-managed opaque handles or IDs, not in-runtime mutation.
- For memoization in examples/tests, prefer pure transforms or host-backed caches with explicit capability calls.
- For code expecting shared mutation for identity or handles, replace with host-managed handles (opaque tokens) or use interned IDs.

Phase 7 — Tests & tooling

- Update tests: trivial cases → pure forms; complex cases → Host capability calls with continuation/resume.
- For concurrency semantics, test using Host mocks that model CAS/retry and assert on final store/log state.

Phase 8 — Final removal & cleanup

- Remove the `legacy-atoms` feature and delete gated code.
- Remove `Value::Atom` and associated stdlib and evaluator code entirely.
- Update `docs/` and remove the `16-mutation-and-state.md` spec or mark archived.

Phase 9 — CI, perf, PR, release

- Ensure `cargo test --all` passes in CI.
- Run benchmarks / spot checks on hot paths.
- Prepare the PR (small per-commit changes, clear description, rollback instructions).

Rollback strategy

- Because changes were gated and incremental, the migration branch can be reverted easily. If merged prematurely, use `git revert` on the PR merge commit and reopen the migration branch for fixes.

Developer guidelines & best practices

- Small commits and per-file changes ease review.
- Prefer compile-time errors with helpful messages over runtime surprises.
- Keep a living inventory and annotate each item with status.

Immediate next step (per your choice)

- I can run the workspace search for mutation symbols and create the inventory file, or
- I can create the migration branch and push it to origin, or
- I can do both.

I will not change code beyond updating docs/branch metadata until you confirm which immediate action(s) to take.

