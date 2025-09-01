wt/rtfs-stability-core — RTFS: umbrella - Remaining E2E feature work and tracking

Goals & references:
- Issue #115: RTFS umbrella - Remaining E2E feature work and tracking
- #127: Stabilize function_expressions feature suite (parser + evaluator parity)
- #128: RTFS: mutation/state primitives spec + atoms + tests
- #125: RTFS 2.0: Add reader deref sugar (@a) and finalize mutation/state semantics
- Group: rtfs-stability-umbrella

Planned work:
- Stabilize failing RTFS e2e feature groups (function_expressions, match_expressions, do_expressions, try_catch_expressions, def_defn_expressions, parallel_expressions, with_resource_expressions, literal_values, vector_operations, map_operations, rtfs2_special_forms, type_system)
- Align parser (shorthand lambdas, method-call sugar) and evaluator (param destructuring, keyword-as-callable, nil-safe keyword access) behavior across AST/IR
- Implement RTFS mutation/state semantics: immutability-by-default, `set!`, `atom`, `deref`, `reset!`, `swap!`
- Add reader deref sugar `@a` for Atoms with proper grammar conflict resolution
- Add comprehensive feature tests and stabilize test suite

Notes:
- Base branch: wt/rtfs-stability-core (branched from main)
- Priority: Fix parser/runtime parity first, then add mutation/state features
- Keep changes focused and well-tested; add e2e feature tests for each implemented feature

Assumptions
- This work will primarily touch `rtfs_compiler/src/rtfs/` (parser, evaluator, IR runtime)
- Local verification will be done by running RTFS e2e feature tests
- Tests are located in `rtfs_compiler/tests/rtfs_files/features/`

Acceptance criteria
- `test_function_expressions_feature` passes locally in both AST and IR modes
- All RTFS e2e feature groups pass consistently (function_expressions, match_expressions, etc.)
- RTFS mutation/state primitives are implemented and tested (`atom`, `deref`, `reset!`, `swap!`)
- Reader deref sugar `@a` works correctly without grammar conflicts
- No regressions in existing RTFS functionality
- Test coverage includes edge cases and error modes

Implementation checklist
- [ ] Triage failing cases in `tests/rtfs_files/features/function_expressions.rtfs`
- [ ] Align parser and evaluator behavior for shorthand lambdas and method-call sugar
- [ ] Fix param destructuring and keyword-as-callable handling in both AST and IR
- [ ] Add spec doc `docs/rtfs-2.0/specs/16-mutation-and-state.md`
- [ ] Implement Value::Atom and stdlib functions (atom/deref/reset!/swap!)
- [ ] Add feature test `tests/rtfs_files/features/mutation_and_state.rtfs`
- [ ] Add reader deref sugar `@a` to pest grammar and parser
- [ ] Resolve grammar conflicts between `@a` (deref) and `@resource-id` (resource refs)
- [ ] Add comprehensive tests for deref sugar and mutation/state primitives
- [ ] Wire new feature tests into test runner for both AST and IR modes
- [ ] Update language features index and cross-link mutation/state docs

Local quick verification
- Build the compiler crate (from repo root):
```bash
cd rtfs_compiler
cargo build
```
- Run RTFS e2e feature tests:
```bash
cd rtfs_compiler
cargo test test_function_expressions_feature -- --nocapture
cargo test --features rtfs2_special_forms -- --nocapture
```
- List available tests:
```bash
cd rtfs_compiler
cargo test -- --list | grep feature
```

Files likely to change
- `rtfs_compiler/src/rtfs/parser.rs` (shorthand lambdas, method-call sugar, deref sugar)
- `rtfs_compiler/src/rtfs/evaluator.rs` (param destructuring, keyword handling)
- `rtfs_compiler/src/rtfs/ir_runtime.rs` (IR-level param binding, assign lowering)
- `rtfs_compiler/src/rtfs/value.rs` (Value::Atom implementation)
- `rtfs_compiler/src/rtfs/stdlib/` (atom, deref, reset!, swap! functions)
- `rtfs_compiler/tests/rtfs_files/features/` (new feature test files)
- `docs/rtfs-2.0/specs/16-mutation-and-state.md` (new spec)
- `docs/rtfs-2.0/specs/01-language-features.md` (update index)

CI and follow-ups
- Ensure all RTFS e2e feature tests pass in CI
- Add CI job that runs feature-specific tests and reports coverage
- Monitor for regressions in existing RTFS functionality

Estimated effort
- Medium change set (3-5 PRs): parser fixes + evaluator alignment + mutation/state primitives + deref sugar + comprehensive tests
- Each PR should be focused on a specific feature group for easier review

Done initial bootstrap; start with function_expressions triage and parser/evaluator alignment.

