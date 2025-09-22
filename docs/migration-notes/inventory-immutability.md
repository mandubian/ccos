# Inventory: mutation primitives and Atom uses

This file was generated from a workspace search for the symbols: set!, atom, deref, reset!, swap!, Value::Atom.

Summary of hits (file -> interesting lines / notes):

- rtfs_compiler/src/runtime/stdlib.rs
  - Several references to `set!` and related STD library handlers (lines ~370, ~378, ~386, ~2680+). This is the main place where mutation primitives are implemented.

- rtfs_compiler/src/runtime/ir_runtime.rs
  - References around line 2008 where runtime lowering/IR handles `set!` placeholder lowering.

- rtfs_compiler/src/ccos/delegation.rs
  - One hit referencing delegation logic (line ~188) — likely unrelated to mutation primitives but worth reviewing.

- rtfs_compiler/src/ccos/arbiter/llm_provider.rs
  - Multiple hits (lines ~212, ~831, ~1019, ~1023, ~1208, ~1242) — these are LLM-provider code paths. Not directly mutation primitives but `Value::Atom` usage may be present.

- Examples and docs
  - examples/manual_plan_exec.rs contains `(set! :sum (call :ccos.math.add 2 3))` example usage.
  - examples/llm_rtfs_plan_demo.rs shows `Value::Atom(_) => "#<atom>"` in rendering logic.

- Misc
  - build logs, target/ files and some generated `.d` files also matched `deref` strings; ignore target/ and build artifacts for migration changes.

Notes / Next steps:

- The authoritative implementation of the mutation primitives appears to be in `rtfs_compiler/src/runtime/stdlib.rs`. We'll want to feature-gate or add deprecation stubs there first.
- The interpreter/runtime lowering in `rtfs_compiler/src/runtime/ir_runtime.rs` needs review to replace placeholder lowering for `set!` with an IR-level assign node (per ISSUE_REMAINING_RTFS.md).
- Examples and docs must be updated after changes.
- I excluded `target/` and other build artifacts from the inventory — they are not sources to edit.

If you'd like I can now:

- (A) Propose specific feature-guard edits in `libstd`/`stdlib.rs` to wrap mutation primitives behind a Cargo feature `legacy-atoms`.
- (B) Produce a PR-style patch set and run unit tests locally (on `main` as you requested) iteratively.

Tell me which next step you prefer; I will ask before making changes to `main`.
