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

- Milestone 3: Introduce DelegationConfig in agent config; plumb through registry → arbiter ✅
  - Extended `DelegationConfig` in `AgentConfig` to include `AgentRegistryConfig`.
  - Added `AgentRegistryConfig`, `RegistryType`, and `AgentDefinition` structs.
  - Implemented `to_arbiter_config()` conversion method for seamless integration.
  - Updated CCOS initialization to automatically wire delegation configuration.
  - Added comprehensive test suite for delegation configuration.
  - Created detailed documentation in `docs/ccos/specs/022-delegation-configuration.md`.
  - Result: Delegation configuration fully integrated from agent config to arbiter.

- Milestone 4: Implement adaptive threshold using rolling success stats with bounds and env/config overrides ✅
  - Created `AdaptiveThresholdConfig` with comprehensive configuration options.
  - Implemented `AdaptiveThresholdCalculator` with decay-weighted performance tracking.
  - Enhanced `SuccessStats` structure with decay-weighted rates and timestamps.
  - Integrated adaptive threshold into `DelegatingArbiter` decision logic.
  - Added environment variable overrides with configurable prefix.
  - Implemented bounds enforcement (min/max threshold values).
  - Added minimum samples requirement before adaptive threshold applies.
  - Created comprehensive test suite for adaptive threshold functionality.
  - Added feedback recording methods for delegation performance tracking.
  - Result: Adaptive delegation threshold fully implemented with deterministic tests.

Try locally:
```bash
cargo test --test integration_tests -- --nocapture --test-threads 1
cargo test --test ast_coverage -- --nocapture
cargo test delegation_keys --lib
cargo test config::types::tests --lib
cargo test adaptive_threshold --lib
```