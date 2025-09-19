
# Conservative staged migration plan: remove atoms via feature-gated, reversible steps

Status: Proposed — conservative/staged
Audience: RTFS compiler/runtime owners, maintainers, release managers
Related: `docs/rtfs-2.0/specs-new/07-immutability-and-state.md`

Goal

Remove mutation primitives from the RTFS core while minimizing risk to CI, review friction, and downstream consumers. Prefer a feature-gated, deprecation-first approach that is fully reversible at each step.

Rationale

The previous aggressive removal plan reduces time-to-completion but increases review and rollback complexity. This staged plan trades elapsed time for safety: each step is reviewable, reversible, and keeps the tree usable.

Plan overview (high level)

1. Discovery & inventory — find every use of `atom`, `set!`, `deref`, `reset!`, `swap!`, `Value::Atom` and reader-macro `@`.
2. Add a `legacy-atoms` cargo feature behind which the current implementations remain. Tests and CI continue to exercise the non-gated code while we iterate.
3. Deprecation pass — mark stdlib functions and docs as deprecated and add runtime warnings (when feature enabled) to steer consumers.
4. Gradual removal — disable feature in CI on a migration branch, fix compile errors, and progressively remove code paths.
5. Final removal & docs cleanup — delete code and update specs and examples; run full CI and performance checks.

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

Phase 3 — Deprecation & docs (0.5 day)

- Mark stdlib functions `#[deprecated(note = "RTFS 2.0 removes atoms — use X instead")]` when feature enabled.
- Add deprecation notes in `docs/rtfs-2.0/specs/` and examples.

Phase 4 — Disable feature in CI on migration branch + fix (iterative)

- On migration branch, run CI with `--no-default-features --features ""` or explicitly with `--no-default-features` to simulate removal.
- Fix compile errors iteratively, preferring small commits that either refactor user code to the immutable model or implement adapter patterns.

Guidance for fixes

- Replace atom-backed counters with pure functions that accept and return state.
- For code expecting shared mutation for identity or handles, replace with host-managed handles (opaque tokens) or use interned IDs.

Phase 5 — Tests & tooling

- Update test helpers that used atoms to accept initial state and return new state.
- For tests that depend on shared mutation for concurrency semantics, replace with mocked Host state or explicit synchronization primitives provided by the test harness.

Phase 6 — Final removal & cleanup

- Remove the `legacy-atoms` feature and delete gated code.
- Remove `Value::Atom` and associated stdlib and evaluator code entirely.
- Update `docs/` and remove the `16-mutation-and-state.md` spec or mark archived.

Phase 7 — CI, perf, PR, release

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

