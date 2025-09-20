Migration checklist: Pure runtime (atoms removed by default) + future Host-backed effects

Phase A — Mutation primitives removed by default (DONE)

- [x] Evaluator: `set!` returns canonical error unless `legacy-atoms` feature is enabled.
- [x] Stdlib: `atom`, `deref`, `reset!`, `swap!` cfg‑gated; non‑legacy stubs return canonical error; legacy builds warn and preserve behavior.
- [x] Secure stdlib: `assoc!`, `reset!` cfg‑gated with same behavior as above.
- [x] Parser keeps `@x` sugar; evaluator lowers to `(deref x)` → canonical error in non‑legacy builds.
- [x] Harness: atom‑heavy feature files (`parallel_expressions`, `test_fault_tolerance`) treated as expected‑fail when `legacy-atoms` is disabled.
- [x] Canonical error: "Atom primitives have been removed in this build. Enable the `legacy-atoms` feature to restore them or migrate code to the new immutable APIs."

Verification

- [x] cargo test (default features): green.
- [x] Feature harness in non‑legacy path: atom‑dependent cases are expected‑fail; suite green.

Phase B — Inventory and migration plan (IN PROGRESS)

- [x] Inventory remaining atom usages in tests; document in `docs/migration-notes/remaining-mutation-inventory.md`.
- [ ] Decide migration for complex features: pure rewrite vs. Host‑backed vs. deprecate.
- [ ] Open follow‑up issues: host APIs design, IR lowering for removal of `set!`, migrate parallel examples.

Phase C — Host‑backed effects (FUTURE)

- [ ] Define RequiresHost contract and resume path.
- [ ] Implement minimal Host capabilities (mock): `counter.inc`, `event.append`, `kv.get`, `kv.cas-put` (idempotency).
- [ ] Migrate tests to host capabilities and remove harness expected‑fail gating.

Notes

- `Value::Atom` variant is retained for compatibility; it isn’t constructed in non‑legacy builds.
- IR converter errors on Deref until a host‑backed deref is designed.

