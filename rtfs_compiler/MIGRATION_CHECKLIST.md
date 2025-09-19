Migration checklist for removing `legacy-atoms` dependency

Completed (this PR)

- [x] Replace simple `set!` examples with let-shadowing or pure functions.
- [x] Replace atom-based increment examples with pure increment functions or computed results.
- [x] Replace sequential swap! sequences with pure reductions over collections.
- [x] Update e2e harness to avoid marking migrated cases as expected failures.

Pending (follow-ups)

- [ ] Replace memoization tests that use atoms with a pure `memoize` helper or a test shim that simulates caching without global mutation.
- [ ] Rework parallel tests that rely on shared mutable atoms — options include:
  - Use a concurrency-safe, feature-retained primitive if needed during migration.
  - Rewrite tests to avoid shared mutable state and assert on eventual state deterministically.
- [ ] Audit the repo for any remaining uses of `atom`, `swap!`, `reset!`, `set!`, and `@`/`deref` outside tests and decide whether to gate or migrate.
- [ ] Update documentation explaining the migration rationale and the replacement patterns for common mutation idioms.

Risks & notes

- Behavioral parity must be verified for migrated tests; I ensured outputs are unchanged for the cases migrated here.
- For parallel tests, deterministic assertions are important; consider replacing timing-based concurrency checks with controlled concurrency helpers.


## Local verification

- Date: 2025-09-19
- Commit: 4e62267
- Action: Ran full feature test matrix from `rtfs_compiler/` with `--no-default-features --features "pest regex"` (i.e. `legacy-atoms` disabled). All features passed locally (106 passed, 0 failed).

Next: create migration branch and open PR (awaiting user signal).

