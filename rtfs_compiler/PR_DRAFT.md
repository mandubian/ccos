Title: tests: migrate small atom-using cases to pure forms; remove harness expected-fail annotations

Summary

This PR applies a conservative migration of small, self-contained tests that previously relied on atom/mutation primitives. It also simplifies the e2e feature test harness by removing now-stale per-feature expected-fail annotations for migrated cases. The goal is to make the test suite pass when the `legacy-atoms` Cargo feature is disabled and to reduce long-term reliance on mutable primitives.

Files changed (high level)

- rtfs_compiler/src/runtime/stdlib.rs
  - Conservative feature-gating changes and clearer runtime error when atom primitives are disabled.
- rtfs_compiler/tests/shared/rtfs_files/features/do_expressions.rtfs
  - Replaced atom/dotimes summation pattern with a pure reduce over a range.
- rtfs_compiler/tests/shared/rtfs_files/features/mutation_and_state.rtfs
  - Rewrote four mutation examples into pure/immutable equivalents.
- rtfs_compiler/tests/shared/e2e_features.rs
  - Removed stale expected_fail entries for `do_expressions` and `mutation_and_state` that were migrated.
- rtfs_compiler/Cargo.toml
  - Minor test/build-related tweaks.

Test results

- Ran the full feature test matrix with `legacy-atoms` disabled:
  - Command: RUST_BACKTRACE=1 cargo test --no-default-features --features "pest regex" -- --nocapture
  - Result: All feature tests passed locally (106 passed; 0 failed; 2 ignored).

Rationale

- Small examples (set!, atom increment, sequential swap!, deref sugar) were straightforward to convert to pure forms. Removing these reduces the test-maintenance burden and enables turning off the `legacy-atoms` feature sooner.
- Larger/more complex tests that rely on mutation (memoization caches, parallel shared mutation) remain and are intentionally left for follow-up, as they require design decisions.

Notes for reviewers

- Review the replaced `.rtfs` examples for behavioral equivalence. They were intentionally changed to avoid mutation while preserving expected outputs.
- The harness change removes per-case expected-fail markers that are no longer needed. The harness still treats atom-removal runtime errors as expected failures for other un-migrated cases when built without `legacy-atoms`.
- I ran the local test suite with the same flags used by CI; please verify CI settings (branch protection or required checks) before merging.

Next steps (recommended)

1. Open a pull request for review and CI validation (this branch contains the changes on `main` locally).
2. Create follow-up issues for complex mutation cases (memoization cache, parallel mutation, etc.) and assign owner(s).
3. When CI is green and branch protection allows, push and merge. If `main` is protected, prefer a PR from a feature branch.

Signed-off-by: automated-agent
