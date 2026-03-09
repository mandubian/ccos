# Iteration and Repair Validation Runbook

This runbook validates that Autonoetic agents iteratively repair malformed tool requests in the same session, instead of failing one-shot or escalating immediately to the user.

## Scope

Validate three layers:

1. Runtime repair loop behavior (unit-level control).
2. Terminal chat ingress repair behavior (real session path).
3. Prompt parity across execution paths (foundation rules are always present).

## Preconditions

- Workspace root: `autonoetic/`
- Rust toolchain installed.
- No conflicting local ports for test runs.
- Environment can run local loopback TCP listeners (`127.0.0.1`).

## Fast Proof Commands

Run the three critical proofs:

```bash
cd autonoetic
cargo test -p autonoetic-gateway test_in_session_repair_loop_recovery_from_structured_error -- --nocapture
cargo test -p autonoetic --test cli_e2e test_terminal_chat_repairs_invalid_agent_install_in_session -- --nocapture
cargo test -p autonoetic-gateway test_execute_loop_includes_foundation_in_system_prompt -- --nocapture
```

Optional cross-path companion assertion:

```bash
cargo test -p autonoetic-gateway test_build_initial_history_injects_session_context_before_user_message -- --nocapture
```

## Full Regression Sweep

```bash
cd autonoetic
cargo test -p autonoetic --test cli_e2e -- --nocapture
cargo test -p autonoetic-gateway tool_call_processor -- --nocapture
cargo test -p autonoetic-gateway lifecycle -- --nocapture
cargo test -p autonoetic-gateway execution -- --nocapture
```

## Practical Real-Life Session Drill

Goal: confirm behavior in a chat-like path where a malformed request is repaired in-session.

1. Run the ingress e2e repair test:

```bash
cd autonoetic
cargo test -p autonoetic --test cli_e2e test_terminal_chat_repairs_invalid_agent_install_in_session -- --nocapture
```

2. Confirm expected sequence from assertions/output:
- first `agent.install` uses malformed payload (empty `agent_id`)
- gateway returns structured validation `tool_result`
- model emits corrected `agent.install`
- install succeeds (`repair_worker` created)

3. Confirm no fallback behavior:
- no immediate `event.ingest failed` at user layer for validation errors
- no fatal abort path triggered for recoverable validation errors

## KPI Checklist

Track these metrics during repeated runs (10+ iterations recommended):

- Repair success rate: `successful_repair_sessions / repair_sessions`
  - target: `>= 95%`
- Mean retries-to-success for repairable validation cases
  - target: `<= 2`
- Fatal abort rate for repairable error classes (`validation|permission|resource|execution`)
  - target: `0%`
- User clarification rate when intent is clear
  - target: near `0%`

## Pass/Fail Criteria

Pass if all are true:

- Runtime repair loop unit proof passes.
- Ingress malformed `agent.install` repair e2e passes.
- Foundation prompt regression test passes.
- No regressions in terminal chat e2e suite.

Fail if any are true:

- Recoverable validation error aborts session.
- No corrected retry appears after validation `tool_result`.
- Prompt parity test fails (foundation rules missing in a path).

## Troubleshooting

- If e2e test fails with parse/deserialization errors:
  - verify generated test `SKILL.md` frontmatter shape and indentation.
- If repair assertion fails but requests show correct behavior:
  - inspect string-escaping assumptions in test assertions; prefer structured JSON checks.
- If port wait timeouts occur:
  - rerun; if persistent, inspect local process conflicts and firewall rules.

## Evidence to Keep

For each validation run, capture:

- command executed
- pass/fail status
- failing assertion snippet (if any)
- relevant causal trace/test output excerpts

Keep evidence in PR notes when behavior or tests are changed.
