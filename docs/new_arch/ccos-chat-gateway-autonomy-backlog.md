# CCOS Chat Gateway Autonomy Backlog (Draft)

**Purpose**: track remaining work to support an autonomous `ccos-agent` that can manage generic goals (including the Moltbook demo flow) while preserving the chat security contract.

## P0 — Run Autonomy Core (Must‑Have)

1. **Run object + lifecycle state machine**
   - Define `RunState` with: Done, Paused(Approval), Paused(ExternalEvent), Failed, Cancelled.
   - Store runs per session with immutable budget context + completion predicate.
   - Persist run status transitions to Causal Chain.
   - Acceptance: runs survive restarts and always end in a terminal state.

2. **Run orchestration endpoints**
   - Add Gateway endpoints for: create run, get run status, cancel run.
   - Ensure token/session validation; include run_id, step_id, and correlation IDs in audit events.
   - Acceptance: external system can submit a goal without a chat message.

3. **Checkpoint/Resume segments**
   - Implement bounded execution segments with checkpointing between segments.
   - Wire resume triggers (cron, external event, manual resume).
   - Acceptance: a long‑running goal progresses in bounded segments.

4. **Budget enforcement in agent loop**
   - Enforce budgets for steps, wall‑clock, tokens, and retries within the agent loop.
   - On exceed: hard‑stop or approval‑required (per policy pack).
   - Acceptance: budgets never allow infinite runs.

## P0 — Skill Loading & Execution Safety (Must‑Have)

5. **Skill load contract validation (fail‑fast)**
   - Reject load results with missing `skill_id`, no operations, or no registered capabilities.
   - Do not surface “unnamed skill” states to users; return actionable error instead.
   - Acceptance: invalid skill definitions never enter runtime state.

6. **Capability schema exposure + tool registry sync**
   - Expose operation `input_schema` to the LLM/tooling layer at load time.
   - Ensure gateway registry and agent context stay in sync after load/unload.
   - Acceptance: LLM sees accurate tool schemas and available operations.

7. **Runtime input validation + missing‑field prompting**
   - Validate params against schema before `ccos.skill.execute`.
   - Auto‑fill from safe agent context (allowlist), then prompt user for remaining required fields.
   - Acceptance: no invalid request reaches an executor; user gets clear missing‑field prompts.

8. **Sanitized logging & safe tool outputs**
   - Redact secrets/tokens and avoid logging full skill definitions.
   - Log execution summaries with redacted inputs/outputs.
   - Acceptance: logs contain no sensitive data and are still diagnosable.

## P1 — Jailing + Scheduler (Critical for Safe Autonomy)

9. **Agent process jailing**
   - Replace `LogOnlySpawner` with a jailed `ProcessSpawner` for production.
   - Ensure agent has no direct shell/network; only Gateway allowed.
   - Acceptance: direct egress by agent is impossible without Gateway.

10. **Scheduler / cron triggers**
   - Add a scheduler for periodic goal execution and follow‑ups.
   - Attach schedule to run metadata and persist it.
   - Acceptance: autonomous goals can run without incoming chat.

## P2 — Goal Planning & Memory (Generic Goals)

11. **Goal queue and completion predicates**
   - Support goal queue per run (subgoals + completion predicates).
   - Ensure explicit completion predicate checks before Done.
   - Acceptance: “generic goal” can complete without manual chat prompts.

12. **Governed memory for goals**
   - Add governed working memory for goal progress and context.
   - Ensure data classification and policy enforcement for stored items.
   - Acceptance: agent can resume goal with safe state.

## P3 — Connector + External System Enablement

13. **Stable external connector contract**
   - Provide adapter SDK/contract for external systems (auth, activation, normalization, outbound).
   - Ensure raw chat data stays quarantined by default.
   - Acceptance: any external system can integrate without breaking safety rules.

14. **Outbound delivery guarantees**
   - Implement retry policy and idempotency keys for outbound messages.
   - Ensure audit events record delivery attempts.
   - Acceptance: outbound delivery is reliable and auditable.

## Demo: Moltbook Autonomy Validation

15. **Autonomous Moltbook run**
   - Goal: register → human verification prompt → verify → heartbeat → post to feed.
   - Tie each stage to run lifecycle and budgets.
   - Acceptance: demo completes without manual chat interaction, only approvals where required.

---

## Dependencies

- Checkpoint/Resume: [docs/ccos/specs/017-checkpoint-resume.md](../ccos/specs/017-checkpoint-resume.md)
- Governance Context: [docs/ccos/specs/005-security-and-context.md](../ccos/specs/005-security-and-context.md)
- Two‑Tier Governance: [docs/ccos/specs/035-two-tier-governance.md](../ccos/specs/035-two-tier-governance.md)
- Chat Connector Adapter: [docs/ccos/specs/043-chat-connector-adapter.md](../ccos/specs/043-chat-connector-adapter.md)
- Chat Mode Security Contract: [docs/ccos/specs/037-chat-mode-security-contract.md](../ccos/specs/037-chat-mode-security-contract.md)
- Resource Budget Enforcement: [docs/new_arch/spec-resource-budget-enforcement.md](spec-resource-budget-enforcement.md)
