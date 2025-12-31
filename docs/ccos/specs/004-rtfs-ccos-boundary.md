# CCOS Specification 004: RTFS ⇄ CCOS Purity Boundary and Prelude

**Status: Implemented**
**Scope**: Defines the separation between pure RTFS runtime and effectful CCOS host capabilities.

This document specifies the separation of concerns between RTFS (runtime + language) and CCOS (host) and how CCOS provides effectful helpers via a host-owned prelude.

## Goals
- RTFS remains pure and host-agnostic. No impure I/O in `rtfs_compiler::runtime::stdlib`.
- All side-effects are performed by CCOS through capabilities and policies.
- Ergonomic, effectful helpers exist, but live in CCOS as a prelude that is loaded after the pure RTFS environment is created.

## Design
- RTFS exposes only pure functions in its standard library. Any effect is invoked through the generic `call` primitive that delegates to the Host.
- CCOS owns a new module `rtfs_compiler::ccos::prelude` with `load_prelude(env: &mut Environment)` which registers effectful helpers. Each helper dispatches to a CCOS capability via the Evaluator Host.
- The CCOS Orchestrator loads this prelude immediately after constructing an `Evaluator` with the pure RTFS environment.

## Contracts
- Inputs: Mutable RTFS `Environment` associated to an `Evaluator` with a Host implementation.
- Behavior: Register a set of builtins that call CCOS capabilities (e.g., logging, time, http, kv state) and return values back to RTFS.
- Error modes: If a capability is denied, missing, or fails, the builtin returns `RuntimeError` with contextual information (capability id, reason when safe to surface).
- Success: Builtins return pure `Value`s consistent with their effect (e.g., strings, numbers, booleans, maps).

## Capability-backed helpers (non-exhaustive)
- Logging/output: `tool/log`, `tool.log`, `log` → `ccos.io.log`; `println` → `ccos.io.println`; `step` → `ccos.io.println`
- Time/env: `tool/time-ms`, `tool.time-ms`, `current-time-millis` → `ccos.system.current-timestamp-ms`; `get-env` → `ccos.system.get-env`
- File I/O: `file-exists?` → `ccos.io.file-exists`; `read-file` → `ccos.io.read-file`; `write-file` → `ccos.io.write-file`; `delete-file` → `ccos.io.delete-file`; streaming helpers continue to map to `ccos.io.open-file`
- Network: `tool/http-fetch`, `http-fetch` → `ccos.network.http-fetch`
- Threading: `thread/sleep` → `ccos.system.sleep-ms`
- KV helpers: `kv/assoc!`, `kv/dissoc!`, `kv/conj!` implemented as host `get` → pure transform → host `put`

## Wiring
- `rtfs_compiler/src/ccos/mod.rs` exposes `pub mod prelude;`
- `rtfs_compiler/src/ccos/orchestrator.rs` invokes `crate::ccos::prelude::load_prelude(&mut evaluator.env)` right after creating the `Evaluator`, both in fresh execution and resume paths.

## Rationale
- Purity makes RTFS portable, testable, and secure. All I/O goes through CCOS governance and capability marketplace.
- The prelude keeps developer ergonomics without compromising the purity boundary.

## Migration notes
- Impure helpers previously in RTFS stdlib have been moved to CCOS prelude. Any code relying on them must execute under CCOS (or load an equivalent prelude).
- Tests/examples that executed RTFS-only environments for effectful helpers should be updated to include prelude loading in CCOS.

## Future work
- Expand prelude with richer high-level helpers as thin wrappers over capabilities.
- Add optional “strict mode” that forbids effectful helpers unless `call` is used explicitly.
