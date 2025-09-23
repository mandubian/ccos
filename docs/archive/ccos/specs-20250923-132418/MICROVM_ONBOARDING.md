# MicroVM Phase 1 — Onboarding

This file contains the onboarding notes for the `wt/microvm-phase1` worktree. It is intentionally separate from `README.md` to avoid overwriting or duplicating any existing README content.

Purpose
- Phase 1 worktree for microVM-related tasks (see issue grouping under `group:microvm`).
- Short-term goals: bootstrap microVM provider integration, add minimal security/perf tests, and provide a reproducible dev environment.

Branch & Worktree
- Branch: `wt/microvm-phase1` (already pushed to `origin`)
- Path: this directory (worktree root)

Quick start
1. From this worktree root:

```bash
# build the project (from repo root)
cd /home/PVoitot/workspaces/mandubian/ccos-wt/microvm-phase1
cargo build

# run the integration tests for compiler/runtime
cd rtfs_compiler
cargo test --test integration_tests -- --nocapture --test-threads 1
```

Local tasks (suggested)
- Implement basic microVM provider scaffold:
  - `src/runtime/microvm/providers/<provider>.rs` (start with a minimal stub that compiles)
  - Add unit tests for provider lifecycle (create/start/execute)
- Security: ensure any microVM operations require explicit capability use
- Performance: add a small benchmark harness or smoke test for VM startup time

Communication
- Tag issues with `group:microvm-phase1` (or `group:microvm`) and assign to reviewers.
- Link PRs to issue #70/#71/#72 (planned microvm issues).

Notes
- This worktree was created from `main` and contains the latest stability patches (RTFS stability work merged). If you need a different base, rebase locally or recreate the worktree from the desired branch.

Contact
- Leave notes or TODOs in this file as you start tasks so others can pick up work easily.
