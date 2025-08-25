wt/arbiter-delegation-enhancements — progress log

- Milestone 1: Make delegation/arbiter tests green
  - Fixed AST parsing for task context access: `@plan-id` now parsed as `ResourceRef`, `@:context-key` as `Symbol`.
  - Corrected map destructuring `:keys` handling in both AST and IR runtimes (normalized keyword representation).
  - Result: ast_coverage and integration tests passing locally.

Try locally:
```bash
cargo test --test integration_tests -- --nocapture --test-threads 1
cargo test --test ast_coverage -- --nocapture
```