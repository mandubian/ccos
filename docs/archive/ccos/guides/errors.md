# Common Error Paths and Demos

This page shows how CCOS surfaces errors early and deterministically, with runnable examples.

## 1) Unknown capability referenced in plan (preflight rejection)

Example: `rtfs_compiler/examples/unknown_capability_demo.rs`

What it does:
- Builds a synthetic plan body with `(call :ccos.does.not.exist ...)`.
- Runs `CCOS::preflight_validate_capabilities(&plan)`.
- Prints the expected error, e.g.: `Unknown capability referenced in plan: ccos.does.not.exist`.

Run:
```sh
cargo run --example unknown_capability_demo --manifest-path rtfs_compiler/Cargo.toml
```

## 2) Intent → Plan has no mapping (no capability/template for goal)

Example: `rtfs_compiler/examples/intent_to_plan_no_capability_demo.rs`

What it does:
- Uses TemplateArbiter with an intent pattern but no matching plan template.
- `natural_language_to_intent` succeeds, but `intent_to_plan` returns:
  `No plan template found for intent: '<intent_name>'`.

Run:
```sh
cargo run --example intent_to_plan_no_capability_demo --manifest-path rtfs_compiler/Cargo.toml
```

## 3) Long-running/stuck orchestration

Mitigation in demos:
- The runtime_service wraps `process_request` in a timeout (~25s) so UI won’t hang.
- TUI loop is cooperative, ensuring keys remain responsive during orchestration.
