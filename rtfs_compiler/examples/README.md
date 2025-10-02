CCOS + RTFS examples

This directory contains a curated set of examples that demonstrate key features of CCOS + RTFS. Older, narrow, or duplicated demos have been moved to `archived/` to keep the top-level examples focused and easier to discover.

How to use

- Most examples live under `rtfs_compiler/examples/` and are intended as runnable demos. Depending on how you build the project, common ways to run them from the repository root or from the `rtfs_compiler` crate are:

  - cargo run --example <example_name>   # if examples are configured as Cargo examples
  - cargo run --bin <binary_name>       # if examples are configured as binaries

  Adjust the command based on your workspace / Cargo layout. If unsure, inspect `rtfs_compiler/Cargo.toml` to see how examples are registered.

Kept examples (top-level) and short descriptions

- `ccos_demo.rs` — High-level demo of CCOS core features and workflows; a good starting point for newcomers.
- `ccos_runtime_service_demo.rs` — Shows how to run CCOS as a runtime service / integration scenario.
- `ccos_tui_demo.rs` (+ `README_TUI_DEMO.md`) — Text UI demo showing an interactive TUI client for exploring RTFS/CCOS flows.
- `llm_arbiter_example.rs` — Example demonstrating the arbiter pattern combined with an LLM provider.
- `rtfs_capability_demo.rs` — Demonstrates RTFS capability invocation and capability lifecycle.
- `rtfs_streaming_complete_example.rs` — A complete streaming example illustrating streaming responses and callbacks.
- `rtfs_reentrance_demo.rs` — Demonstrates RTFS reentrance semantics and how reentrant flows behave.
- `intent_graph_demo.rs` — Shows intent graph generation / inspection and how intents map to plans.
- `llm_rtfs_plan_demo.rs` / `plan_generation_demo.rs` — Plan generation examples showing how LLMs and RTFS collaborate on plan creation (keep one or both if useful).
- `live_interactive_assistant.rs` — A practical interactive assistant demo showcasing a live user flow.
- `comprehensive_demo.rs` — A broad demo that ties multiple features together (kept for tour-style demonstrations).
- `github_mcp_demo.rs` and `github_mcp_demo.rtfs` — Demonstrate the MCP (capability marketplace) integration with GitHub (kept as a pair: Rust demo + RTFS script).
- `serve_metrics.rs` — Demonstrates exposing metrics / observability for runtime components.
- `context_types_demo.rs` — Examples of context typing and context usage across CCOS components.
- `hierarchical_context_demo.rs` — Shows hierarchical contexts and inheritance/composition of contexts.
- `unknown_capability_demo.rs` — Small, focused test/demo for behavior when capabilities are missing or unknown.
- `runtime_test_programs.rtfs` — A set of RTFS test programs useful for runtime verification and smoke tests.

Archived examples

- Older, niche, provider-specific, experimental or duplicated examples have been moved to `rtfs_compiler/examples/archived/`. They remain in the repository history and can be restored at any time.

Next steps (recommended)

- Run quick smoke builds for a few kept examples (e.g., `ccos_demo`, `rtfs_capability_demo`, `ccos_tui_demo`) to ensure everything builds in your environment.
- If you want, I can add a short script `scripts/run_example.sh` that normalizes how examples are executed in this repo.
- Consider adding a short note in the repo top-level README pointing to this examples README for discoverability.

If you'd like different examples kept or want some archived examples restored to top-level, tell me which ones and I'll adjust.
