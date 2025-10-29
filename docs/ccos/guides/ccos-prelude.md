# CCOS Prelude: Effectful Helpers for RTFS Programs

This guide shows how CCOS loads a host-owned prelude that provides ergonomic wrappers for common effects. The prelude delegates to CCOS capabilities and respects governance policies.

## What you get
The prelude registers functions like:
- `log`, `tool/log`, `tool.log`, `println`, `step`
- `tool/time-ms`, `tool.time-ms`, `current-time-millis`, `thread/sleep`
- `get-env`, `file-exists?`, `tool/open-file`
- `http-fetch`, `tool/http-fetch`
- `kv/assoc!`, `kv/dissoc!`, `kv/conj!` (implemented via host get/put)

All of these call CCOS capabilities under the hood:
- IO: `ccos.io.log`, `ccos.io.println`, `ccos.io.file-exists`, `ccos.io.open-file`
- System: `ccos.system.current-timestamp-ms`, `ccos.system.sleep-ms`, `ccos.system.get-env`
- Network: `ccos.network.http-fetch`
- State: `ccos.state.kv.get`, `ccos.state.kv.put`

## How it’s wired
- RTFS creates a pure environment via its stdlib.
- CCOS then calls `ccos::prelude::load_prelude(&mut evaluator.env)` after creating the evaluator. This augments the environment with the effectful helpers.

## Usage example
In RTFS code running under CCOS, you can simply call:

```
(log "hello from CCOS")
(println "visible to user")
(current-time-millis)
(thread/sleep 50)
(file-exists? "/etc/hosts")
(http-fetch {:url "https://example.com"})
```

For KV helpers, the pattern is atomic get-transform-put via host:
```
(kv/assoc! "session:123" :last_seen (current-time-millis))
(kv/dissoc! "session:123" :temp)
(kv/conj! "queue:work" {:job "index", :id 42})
```

## Security and governance
- All helpers route through CCOS capabilities; policies and resolution apply.
- If a capability is missing or denied, you’ll receive a runtime error identifying the attempted capability.

## Portability
- Outside CCOS, these helpers aren’t available unless your host provides a compatible prelude. RTFS itself remains pure and host-agnostic.

## Troubleshooting
- “Function not found”: Ensure the CCOS Orchestrator is loading the prelude for your evaluator.
- “Capability denied/missing”: Check marketplace/registry configuration and the current runtime context.
