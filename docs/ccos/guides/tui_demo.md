# TUI Demo (ratatui + crossterm)

An interactive demo that exercises the runtime_service in a non-blocking TUI.

Run (from repo root):

```sh
cargo run --example ccos_tui_demo --manifest-path rtfs_compiler/Cargo.toml
```

Keys:
- s: Start processing current goal
- c: Request cancel (best-effort; runtime_service placeholder as of now)
- q: Quit

Implementation notes:
- The TUI uses a current-thread Tokio runtime with LocalSet to keep non-Send components on the same thread.
- The UI loop polls input and drains the event channel cooperatively; a small sleep (~16ms) yields to prevent busy looping.
- Runtime events are forwarded via a broadcast IntentEventSink without sacrificing CausalChain auditability.
- Default goal is an offline-safe math example to complete quickly.
