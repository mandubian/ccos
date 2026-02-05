# CCOS Chat Gateway Autonomy Backlog (Draft)

**Purpose**: track remaining work to support an autonomous `ccos-agent` that can manage generic goals (including the Moltbook demo flow) while preserving the chat security contract.

## P0 — Governed Instruction Resources + Egress Boundary (Must‑Have)

**Why**: `skill.md` is only one example of an instruction artifact. The generic problem is: users provide goals plus *custom instructions/resources* (URLs, pasted prompts, docs), and the agent must reason freely **without ever doing direct HTTP**. All I/O must remain inside CCOS governance (proxy/allowlist, budgets, secret handling, causal chain).

1. **Eliminate direct HTTP surfaces reachable by agents**
   - Replace any “convenience fetch” paths that use direct `reqwest` (e.g. MCP `ccos_fetch_url`) with governed capability calls.
   - Ensure `ccos.skill.load` (and any future loaders) do not fetch URLs directly; they must go through the governed network boundary.
   - Status: ⏳ Not done (direct `reqwest` still present in multiple paths).
   - Acceptance: there is exactly one approved egress path for agent-initiated network requests; everything else is blocked or routed through it.

2. **Harden `ccos.network.http-fetch` as the single governed egress path**
   - Route through GK-controlled proxy / allowlist enforcement (domain + port + method).
   - Add network byte metering + budget integration (per-call + per-run counters).
   - Record egress audit events to causal chain (request metadata + redacted headers, response status + byte counts).
   - Status: ⏳ Not done (currently does direct `reqwest` with minimal policy).
   - Acceptance: `http-fetch` enforces allowlists and budgets, and emits causal-chain records for every call.

3. **Generic instruction resource ingestion + retrieval**
   - Add a generic capability set (names TBD) for instruction resources:
     - `ccos.resource.ingest` — ingest `{text | file:// | http(s)://}` into a governed store (provenance, content-type, hash, classification/quarantine label).
     - `ccos.resource.get` — retrieve content by handle with policy-aware truncation/redaction.
     - `ccos.resource.list` — list resources associated with a session/run.
   - Resource ingestion from `http(s)://` must use the governed egress path (`ccos.network.http-fetch`).
   - Persist resource events to causal chain and support rebuild on gateway startup (similar to runs).
   - Status: ⏳ Not implemented.
   - Acceptance: the agent can reliably “read the instructions” from a URL/file/text via CCOS, and resume runs with stable resource handles.

4. **Untrusted-instructions framing + policy guardrails**
   - Update agent prompting/contracts to treat all instruction resources as untrusted data, never as authority over CCOS policy.
   - Ensure “instructions” can’t request direct egress, arbitrary tool installation, or secret exfiltration without approvals and policy pack checks.
   - Status: ⏳ Planned.
   - Acceptance: instruction injection does not bypass approvals/allowlists/secrets governance.

## P0 — Run Autonomy Core (Must‑Have)

1. **Run object + lifecycle state machine**
   - Define `RunState` with: Done, Paused(Approval), Paused(ExternalEvent), Failed, Cancelled.
   - Store runs per session with immutable budget context + completion predicate.
   - Persist run status transitions to Causal Chain.
   - Status: ✅ Implemented - `RunStore::rebuild_from_chain()` replays run events on gateway startup.
   - Acceptance: ✅ Runs survive restarts and always end in a terminal state.

2. **Run orchestration endpoints**
   - Add Gateway endpoints for: create run, get run status, cancel run, transition, list runs, list run actions.
   - Ensure token/session validation; include run_id + step_id correlation in causal chain.
   - Status: ✅ Implemented:
     - `POST /chat/run`
     - `GET /chat/run/:run_id`
     - `GET /chat/run?session_id=...`
     - `GET /chat/run/:run_id/actions?limit=...`
     - `POST /chat/run/:run_id/cancel`
     - `POST /chat/run/:run_id/transition`
   - Acceptance: external system can submit a goal without a chat message.

3. **Checkpoint/Resume segments**
   - Implement bounded execution segments with checkpointing between segments.
   - Wire resume triggers (cron, external event, manual resume).
   - Status: ⏳ Not implemented yet (runs can pause/resume, but without durable checkpoints).
   - **Next (P2)**: Add `RunState::PausedCheckpoint`, `CheckpointStore` for segment state, `POST /chat/run/:run_id/checkpoint` endpoint.
   - Acceptance: a long‑running goal progresses in bounded segments.

4. **Budget enforcement in agent loop**
   - Enforce budgets for steps and wall‑clock within the agent loop.
   - Enforce run state + per-run budget gate in Gateway `/chat/execute` (block non-chat capabilities while paused/terminal; pause to approval on budget exceed).
   - On exceed: hard‑stop or approval‑required (per policy pack).
   - Status: ✅ Steps/time budgets (agent) + run gate (gateway).
   - Remaining: ⏳ token/cost/network metering; retries budget.
   - Acceptance: budgets never allow infinite runs.

## P1 — Structured Skills (Optional) + Execution Safety

**Note**: Skills remain useful as a *structured* format that can optionally register per-operation tools, but they are not the generic “instruction resource” abstraction. Keep skill parsing/registration as an optimization layer on top of generic resource ingestion.

5. **Skill load contract validation (fail‑fast)**
   - Reject load results with missing `skill_id`, no operations, or no registered capabilities.
   - Do not surface “unnamed skill” states to users; return actionable error instead.
   - Status: ✅ Implemented (fail-fast parsing/registration); plus URL guardrails for `ccos.skill.load`.
   - Acceptance: invalid skill definitions never enter runtime state.

6. **Capability schema exposure + tool registry sync**
   - Expose operation `input_schema` to the LLM/tooling layer at load time.
   - Ensure gateway registry and agent context stay in sync after load/unload.
   - Status: 🔶 Partially implemented (schemas flow via capability registry; still need richer “tool registry delta” semantics and unload story).
   - Acceptance: LLM sees accurate tool schemas and available operations.

7. **Runtime input validation + missing‑field prompting**
   - Validate params against schema before `ccos.skill.execute`.
   - Auto‑fill from safe agent context (allowlist), then prompt user for remaining required fields.
   - Status: 🔶 Partially implemented (schema validation exists; prompting behavior needs tightening; agent now skips `ccos.skill.execute` calls missing `operation`).
   - Acceptance: no invalid request reaches an executor; user gets clear missing‑field prompts.

8. **Sanitized logging & safe tool outputs**
   - Redact secrets/tokens and avoid logging full skill definitions.
   - Log execution summaries with redacted inputs/outputs.
   - Status: ✅ Implemented baseline redaction + safer error messaging; continue hardening as needed.
   - Acceptance: logs contain no sensitive data and are still diagnosable.

## P1 — Jailing + Scheduler (Critical for Safe Autonomy)

9. **Agent process jailing**
   - Replace `LogOnlySpawner` with a jailed `ProcessSpawner` for production.
   - Ensure agent has no direct shell/network; only Gateway allowed.
   - Status: ⏳ Planned.
   - Acceptance: direct egress by agent is impossible without Gateway.

10. **Scheduler / cron triggers**
   - Add a scheduler for periodic goal execution and follow‑ups.
   - Attach schedule to run metadata and persist it.
   - Status: ⏳ Planned.
   - Acceptance: autonomous goals can run without incoming chat.

## P2 — Goal Planning & Memory (Generic Goals)

11. **Goal queue and completion predicates**
   - Support goal queue per run (subgoals + completion predicates).
   - Ensure explicit completion predicate checks before Done.
   - Status: 🔶 Partial completion predicate support:
     - Agent respects `manual|always|never` and avoids auto-done for unknown predicates.
     - Gateway enforces `never` and supports `capability_succeeded:<capability_id>` for Done transitions.
   - **Next (P1)**: Generic predicate DSL (`all_of`, `any_of`, `state_exists`, `capability_succeeded`) with evaluator in `predicates.rs`.
   - Acceptance: “generic goal” can complete without manual chat prompts.

12. **Governed memory for goals**
   - Add governed working memory for goal progress and context.
   - Ensure data classification and policy enforcement for stored items.
   - Status: 🔶 Working memory exists; goal-specific memory model + governance policies still evolving.
   - Acceptance: agent can resume goal with safe state.

## P3 — Connector + External System Enablement

13. **Stable external connector contract**
   - Provide adapter SDK/contract for external systems (auth, activation, normalization, outbound).
   - Ensure raw chat data stays quarantined by default.
   - Status: ⏳ Planned.
   - Acceptance: any external system can integrate without breaking safety rules.

14. **Outbound delivery guarantees**
   - Implement retry policy and idempotency keys for outbound messages.
   - Ensure audit events record delivery attempts.
   - Status: ⏳ Planned.
   - Acceptance: outbound delivery is reliable and auditable.

## Demo: Moltbook Autonomy Validation

15. **Autonomous Moltbook run**
   - Goal: register → human verification prompt → verify → heartbeat → post to feed.
   - Tie each stage to run lifecycle and budgets.
   - Status: 🔶 Partial (demo works interactively; full “no-manual-chat” orchestration and robust resume/checkpoint remains).
   - Acceptance: demo completes without manual chat interaction, only approvals where required.

---

## Dependencies

- Checkpoint/Resume: [docs/ccos/specs/017-checkpoint-resume.md](../ccos/specs/017-checkpoint-resume.md)
- Governance Context: [docs/ccos/specs/005-security-and-context.md](../ccos/specs/005-security-and-context.md)
- Two‑Tier Governance: [docs/ccos/specs/035-two-tier-governance.md](../ccos/specs/035-two-tier-governance.md)
- Chat Connector Adapter: [docs/ccos/specs/043-chat-connector-adapter.md](../ccos/specs/043-chat-connector-adapter.md)
- Chat Mode Security Contract: [docs/ccos/specs/037-chat-mode-security-contract.md](../ccos/specs/037-chat-mode-security-contract.md)
- Resource Budget Enforcement: [docs/new_arch/spec-resource-budget-enforcement.md](spec-resource-budget-enforcement.md)
