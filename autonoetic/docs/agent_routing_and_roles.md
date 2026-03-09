# Autonoetic: Front-Door Routing and Evolving Role Orchestration

> Note: the canonical docs location is now `docs/agent_routing_and_roles.md`.

This document defines how Autonoetic should route ambiguous user goals when the user does not explicitly target a specific agent, and how specialist roles should be modeled in a self-evolving multi-agent system.

The core idea is simple:

- the Gateway stays thin
- ambiguous ingress lands on a default lead agent
- the lead agent decides whether to answer directly, call tools directly, or delegate to specialists
- specialists are chosen through role contracts and learned fitness, not through hardcoded gateway rules

This keeps routing auditable, policy-governed, and compatible with Autonoetic's long-term learning and self-modification goals.

## 1. Core Routing Rule

When an inbound event arrives through `event.ingest`, terminal chat, or any future channel adapter:

1. If the caller explicitly sets `target_agent`, route to that agent.
2. Else if the `session_id` is already bound to a lead agent, route to that same lead agent.
3. Else route to the configured default front-door lead agent, typically `planner.default`.
4. If no default lead agent exists, fail explicitly rather than guessing.

The Gateway does not try to infer whether the request "sounds like research" or "sounds like coding". That decision belongs to the lead agent.

This is the key separation of concerns:

- Gateway: transport, policy, admission control, causal logging, execution
- Lead agent: intent interpretation, decomposition, role selection, synthesis

## 2. Session Affinity

Implicit routing should be stable across a conversation.

If a user starts a session with an ambiguous request and it lands on `planner.default`, follow-up messages in the same `session_id` should continue to route to that lead agent unless one of these is true:

- the user explicitly targets another agent
- the session is intentionally transferred
- the lead agent is retired and replaced with a new lead agent for that session

This prevents follow-up messages from accidentally bouncing between specialists based on keyword matching.

Minimal session-level routing state should include:

```json
{
  "session_id": "sess_01",
  "lead_agent_id": "planner.default",
  "active_goal": "Build a paper-trading bot from public APIs",
  "delegation_count": 3,
  "pending_tasks": [
    "research.public_api_scan",
    "evaluate.backtest_metrics"
  ]
}
```

Autonoetic already has gateway-owned session context; the lead-agent binding should live there.

## 3. Roles Are Contracts, Not Fixed Personalities

Autonoetic should distinguish four different things that are often conflated in simpler systems:

- `role`: the contract for a kind of work, such as `researcher` or `auditor`
- `agent template`: a reusable manifest or scaffold implementing that role
- `agent instance`: one concrete spawned or installed agent with its own memory, history, metrics, and lineage
- `learned specialization`: a role variant that emerged from prior runs, such as `coder.rust`, `researcher.exchange_api`, or `auditor.generated_skill`

This distinction matters because Autonoetic is self-evolving. The planner should route to a role first, then resolve that role to the best available agent instance or template for the current task.

So the user does not target "the coder" directly unless they explicitly ask to. The user targets the system. The lead agent chooses the role contract, then resolves the best implementation of that role.

## 4. Initial Role Catalog

The role set can take inspiration from OpenFang's manager-and-hands shape without copying it mechanically. In Autonoetic, each role should be defined around memory, artifacts, evaluation, and evolution.

### Primary roles

- `planner.default`
  - The front door for ambiguous goals.
  - Decides execution mode: direct answer, deterministic scheduled action, or multi-agent orchestration.
  - Breaks goals into sub-goals, chooses roles, spawns specialists, and synthesizes the final reply.

- `researcher`
  - Gathers external and internal evidence.
  - Produces citations, artifact handles, uncertainty notes, and candidate durable facts for memory.
  - Preferred when the task depends on public docs, prior runs, APIs, issue trackers, or comparable examples.

- `architect`
  - Defines structure before implementation.
  - Produces plans, interfaces, data flow, boundary decisions, and explicit trade-offs.
  - Preferred when the problem is underspecified, cross-cutting, or likely to trigger expensive rewrites if coded too early.

- `coder`
  - Produces or edits code, scripts, skill sidecars, manifests, tests, and migration steps.
  - Preferred when the requested output is a runnable artifact or concrete repo change.

- `debugger`
  - Reproduces failures, isolates root cause, and proposes minimal repair paths.
  - Preferred when the user reports a bug, regression, crash, trace, or unexpected behavior.

- `evaluator`
  - Validates that something actually works.
  - Produces test results, benchmarks, backtests, simulations, metrics, and pass/fail summaries.
  - This role is the Autonoetic reinterpretation of a pure test-engineer role: it is not only about tests, but about evidence of real-world effectiveness.

- `auditor`
  - Reviews outputs for security, policy, reproducibility, and self-poisoning risk.
  - Preferred when the system is about to promote new code, grant more autonomy, access sensitive capabilities, or install reusable artifacts.

### Evolution-native roles

- `memory-curator`
  - Distills stable learnings from a run into Tier 2 memory with provenance, confidence, and visibility.
  - Prevents useful facts from remaining trapped inside one transient run.

- `evolution-steward`
  - Decides what should become durable: a new skill, a revised prompt, a new specialist variant, or an installed long-lived worker.
  - This is where the current `specialized_builder` example naturally fits.

Not every deployment needs every role as a long-lived installed agent. Some roles can exist as templates only and be instantiated on demand.

## 5. Where `specialized_builder` Fits

`specialized_builder` should not be treated as the front-door router.

Its job is not:

- to receive every ambiguous user goal
- to decide between `researcher`, `coder`, `auditor`, and other roles
- to behave like a semantic gateway

Its job is:

- to create or adapt durable specialist agents when the planner decides that a new specialist is needed
- to install a new long-lived worker for a recurring job
- to turn repeated successful patterns into reusable agents or skills
- to rewrite an existing specialist when the platform has enough evidence that the new version is better

In other words:

- `planner.default` answers "which role should handle this work?"
- `specialized_builder` answers "how do I create or evolve the specialist that will handle this kind of work in the future?"

That makes `specialized_builder` part of the evolution layer, not the ingress layer.

## 6. Role Registry

The lead agent needs a compact role registry in its prompt or a retrievable manifest so it knows what roles exist, when to use them, and what outputs they are expected to produce.

One possible thin schema:

```yaml
roles:
  - role_id: "researcher"
    kind: "specialist"
    description: "Collect evidence from docs, web, artifacts, and memory."
    use_when:
      - "Need external or historical evidence before deciding"
      - "Need citations or source-backed claims"
    avoid_when:
      - "The task is pure implementation with already-known requirements"
    expected_outputs:
      - "summary.md"
      - "sources.json"
      - "artifact handles for large collected material"
    success_signals:
      - "Claims are source-backed"
      - "Conflicts and uncertainty are explicit"

  - role_id: "coder"
    kind: "specialist"
    description: "Create or modify code and executable artifacts."
    use_when:
      - "Need a script, patch, skill sidecar, or manifest change"
      - "Need concrete runnable output"
    avoid_when:
      - "The task is still missing key design decisions"
    expected_outputs:
      - "repo diff or generated artifact"
      - "tests or verification notes"
    success_signals:
      - "Artifact runs"
      - "Changes are minimal and auditable"

  - role_id: "auditor"
    kind: "specialist"
    description: "Check security, governance, and reproducibility risk."
    use_when:
      - "New code or skill is about to be promoted"
      - "Sensitive capability use or self-modification is involved"
    avoid_when:
      - "The task is a low-risk informational answer"
    expected_outputs:
      - "findings.md"
      - "risk level and remediation list"
    success_signals:
      - "Every major risk is explicit"
      - "Promotion decision is justified"
```

This registry is guidance for the lead agent, not a hidden gateway rules engine.

## 7. Planner Decision Loop

When the user goal is not explicit about the target role, the lead agent should follow a stable loop:

1. Interpret the goal and extract the requested outcome.
2. Choose execution mode:
   - direct answer
   - direct deterministic tool or scheduled worker
   - multi-step orchestration
3. If orchestration is needed, decompose the work into sub-goals with explicit expected outputs.
4. For each sub-goal, choose the best role based on the role registry.
5. Resolve that role to an existing agent instance or a known template.
6. If no good implementation exists, delegate to `evolution-steward` or `specialized_builder`.
7. Spawn or message the chosen specialist.
8. Collect outputs, run evaluator and auditor passes when needed, then synthesize the user-facing result.
9. Distill durable learnings into memory and candidate reusable skills only when justified by evidence.

The lead agent should ask the user for clarification only when ambiguity changes the business decision, the approval boundary, or the success criteria. Otherwise it should make a best-effort decomposition and proceed.

## 8. Learned Routing Instead of Static Routing

This is where Autonoetic should move beyond a static manager-and-hands model.

Role selection should improve from prior runs. The planner should consider:

- recent success rate of role variants on similar tasks
- failure patterns and known weak spots
- cost and latency profile
- approval burden
- tool and capability fit
- artifact compatibility with the current workflow
- trust level for self-modifying work

For example, the planner may learn that:

- `researcher.api_docs` performs better than generic `researcher` for SDK work
- `coder.rust` has a higher success rate than `coder.general` in this repository
- `auditor.generated_skill` must always review code emitted by self-evolving builders

This means the routing unit is not just "role name". It is:

- task requirement
- role contract
- best current implementation of that role

That is much closer to Autonoetic's identity as a self-improving system.

## 9. Delegation Contracts

Delegation should be explicit and structured so the Causal Chain records not only that a child agent ran, but why it was chosen and what it was expected to deliver.

The actual gateway method may still target a concrete `agent_id`. The role resolution happens before the call.

Example planner-to-gateway spawn call:

```json
{
  "jsonrpc": "2.0",
  "method": "agent.spawn",
  "params": {
    "agent_id": "researcher.default",
    "session_id": "sess_01",
    "message": "Research public docs for the Kraken API and summarize rate limits, auth model, and paper-trading-safe endpoints.",
    "metadata": {
      "delegated_role": "researcher",
      "delegation_reason": "Need external evidence before coding",
      "expected_outputs": [
        "summary.md",
        "sources.json"
      ],
      "reply_to_agent_id": "planner.default",
      "parent_goal": "Build a paper-trading bot from public APIs"
    }
  }
}
```

Example specialist-to-planner result message:

```json
{
  "jsonrpc": "2.0",
  "method": "AgentMessage",
  "params": {
    "target_agent": "planner.default",
    "session_id": "sess_01",
    "payload": {
      "delegated_role": "researcher",
      "status": "completed",
      "summary": "Kraken supports public market data endpoints suitable for paper trading; authenticated order placement endpoints should remain disabled in the first phase.",
      "artifact_refs": [
        "artifact://sources/kraken-api-docs"
      ],
      "memory_candidates": [
        "kraken.paper_trading.safe_endpoints"
      ]
    }
  }
}
```

The exact payload can evolve, but the principle should hold:

- every delegation names the intended role
- every delegation records why that role was chosen
- every result carries explicit outputs, not only free-form prose

## 10. Practical Selection Heuristics for the Lead Agent

These are planner instructions, not gateway rules:

- If the goal depends on facts not yet established, choose `researcher` first.
- If the goal requires major structural decisions, choose `architect` before `coder`.
- If the goal is a failing behavior, choose `debugger` before `coder`.
- If code or a reusable artifact must be produced, choose `coder`.
- If the artifact must be proven, choose `evaluator`.
- If the artifact is about to gain durable authority, reusable status, or self-modifying reach, choose `auditor`.
- If the run discovered stable reusable knowledge, choose `memory-curator`.
- If the run suggests a new persistent specialist or agent upgrade, choose `evolution-steward` or `specialized_builder`.

These heuristics belong in the lead agent's operating instructions and role registry. They should not be reimplemented as a large semantic switchboard in the Gateway.

## 11. Example Flows

### Example A: Ambiguous user goal

User:

```text
Build me a script that uses this exchange API safely.
```

Likely lead-agent plan:

1. Route to `planner.default`.
2. Spawn `researcher` to inspect public docs and safe boundaries.
3. Spawn `architect` if the API or workflow is structurally unclear.
4. Spawn `coder` to implement the script or skill.
5. Spawn `evaluator` to run simulation or tests.
6. Spawn `auditor` before promotion if reusable code or persistent capability is being installed.
7. Optionally hand off to `evolution-steward` if the result should become a reusable skill or durable worker.

### Example B: Bug report

User:

```text
This worker keeps crashing after two runs.
```

Likely lead-agent plan:

1. Route to `planner.default`.
2. Spawn `debugger` to reproduce and isolate root cause.
3. Spawn `coder` to implement the minimal fix.
4. Spawn `evaluator` to confirm the regression is gone.
5. Send findings back through the lead agent for synthesis.

### Example C: Request for a new durable specialist

User:

```text
Create an agent that checks SEC filings every morning and writes me a summary.
```

Likely lead-agent plan:

1. Route to `planner.default`.
2. Spawn `researcher` to understand filing sources and constraints.
3. Spawn `architect` to define the worker shape and output contract.
4. Spawn `coder` to build the runnable logic.
5. Spawn `evaluator` to run a first-cycle validation.
6. Spawn `auditor` if the worker is about to be installed durably.
7. Spawn `specialized_builder` or `evolution-steward` to install the final long-lived worker agent.

In this flow, the user still never directly targets `specialized_builder`. The lead agent chooses it only once the task has become "create a durable specialist".

## 12. Minimal Implementation Path

To turn this design into implementation work, the thin next steps are:

1. Add a configured default lead agent such as `planner.default`.
2. Persist `lead_agent_id` in gateway session context.
3. Provide the lead agent with a compact role registry.
4. Extend delegation metadata so causal logs capture `delegated_role`, `delegation_reason`, and `expected_outputs`.
5. Add example role agents for `researcher`, `architect`, `coder`, `debugger`, `evaluator`, and `auditor`.
6. Treat `specialized_builder` as the first concrete `evolution-steward` proof-of-concept.
7. Add end-to-end tests for:
   - ambiguous ingress to default lead agent
   - multi-role delegation chains
   - session affinity across follow-up messages
   - promotion of a reusable worker through evaluator and auditor gates

## 13. Bottom Line

When the user is not explicit, the goal should target the default lead agent, not a specialist.

The lead agent chooses the specialist role.

The chosen role resolves to the best current specialist implementation.

`specialized_builder` is not the router. It is the mechanism the system uses when it decides it should create or evolve a specialist for future work.
