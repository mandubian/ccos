wt/arbiter-delegation-enhancements — progress log

- Milestone 1: Make delegation/arbiter tests green ✅
  - Fixed AST parsing for task context access: `@plan-id` now parsed as `ResourceRef`, `@:context-key` as `Symbol`.
  - Corrected map destructuring `:keys` handling in both AST and IR runtimes (normalized keyword representation).
  - Result: ast_coverage and integration tests passing locally.

- Milestone 2: Centralize delegation metadata key constants ✅
  - Created `delegation_keys` module with centralized constants for all metadata keys.
  - Replaced string literals with constants throughout codebase:
    - `intent::INTENT_TYPE`, `intent::COMPLEXITY`
    - `generation::GENERATION_METHOD` with `methods::LLM/TEMPLATE/DELEGATION/etc.`
    - `agent::DELEGATED_AGENT`, `agent::AGENT_TRUST_SCORE`, `agent::AGENT_COST`
  - Added validation functions for key and value validation.
  - Updated all arbiter implementations and examples to use constants.
  - Result: All tests passing, no raw metadata keys in codebase.

Try locally:
```bash
cargo test --test integration_tests -- --nocapture --test-threads 1
cargo test --test ast_coverage -- --nocapture
cargo test delegation_keys --lib
```