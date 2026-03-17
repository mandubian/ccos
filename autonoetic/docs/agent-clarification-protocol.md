# Agent-to-Agent Clarification Protocol

## Overview

When a spawned specialist agent encounters ambiguous or missing information that fundamentally changes the outcome, it uses a structured output format to request clarification from its caller. The caller (typically the planner) answers from its own context or escalates to the user, then respawns the specialist with clarified instructions.

This protocol enables agents to ask each other questions — not just the user — creating a natural delegation feedback loop.

## Problem It Solves

Before this protocol, when a specialist agent was blocked by missing information:

1. **Guessing** — Agent assumes a default (may be wrong, silent failure)
2. **Failing** — Agent returns an error (wasteful, no recovery)
3. **Unstructured text** — Agent asks a question in its response (may not reach caller)

The foundation instructions said "ask the user rather than inventing hidden assumptions" but provided no mechanism. Worse, when a spawned specialist needs clarification, the user doesn't see its output — only the planner's synthesized response.

## The Protocol

### Output Format

When blocked by missing information, an agent outputs:

```json
{
  "status": "clarification_needed",
  "clarification_request": {
    "question": "What port should the server listen on?",
    "context": "Task says 'build a web service' but port not specified in task or design"
  }
}
```

The format has two fields:
- **`question`** (required): The specific question that needs answering
- **`context`** (required): Why this question is blocking, what the ambiguity is

No other fields. The LLM only makes one decision: "do I need clarification or not?" — binary choice.

### When to Request Clarification

- **Missing required parameter** that changes the implementation fundamentally (port number, API endpoint, data format)
- **Ambiguous instruction** with multiple valid interpretations that produce different outcomes
- **Conflicting requirements** between task and design

### When to Proceed Without Clarification

- **Reasonable default exists**: Use it (e.g., port 8080 for dev server, UTF-8 encoding, standard timeouts)
- **Clear best interpretation**: One interpretation is clearly better given the context
- **Minor issue**: The ambiguity does not change the core outcome

## The Clarification Flow

```
1. Planner spawns coder: "Build an HTTP endpoint"
2. Coder turn: finds ambiguity → outputs clarification_request
3. Gateway routes result back to planner (standard delegation return)
4. Planner's next turn: sees coder needs clarification
5. Planner either:
   a. Answers from its knowledge: "Use 8080" → respawns coder with clarified instructions
   b. Asks user: "The coder needs to know what port. What port?"
   c. Combines both: answers what it can, asks user for what it can't
```

No new gateway tools needed. The clarification request flows through the existing `agent.spawn` return path — the parent always sees the child's output.

### Caller Handling

When a planner receives a child agent's clarification request:

1. **Can I answer from my knowledge of the goal?**
   - Answer directly based on your understanding of the overall objective
   - Respawn the child with clarified instructions

2. **Do I need user input to answer?**
   - Ask the user the child's question (relay it clearly)
   - Wait for the user's response
   - Respawn the child with the user's answer

3. **Combine both**
   - Answer what you can from your context
   - Ask the user for what you cannot determine

### Respawn Pattern

When respawning a child after clarification, include:

- The clarified instruction (incorporating the answer)
- A reference to the child's previous work: `"Your previous work is saved as handle:sha256:..."`
- Original task context so the child continues from where it left off

Example:

```
agent.spawn("coder.default", message="Build an HTTP endpoint on port 8080.
Your previous work on the endpoint structure is saved as handle:sha256:abc123.
Continue from there, adding the port binding.")
```

## Agent-Specific Guidelines

Each specialist has role-appropriate clarification triggers:

### Coder

| Request clarification when | Proceed without when |
|---------------------------|---------------------|
| Required parameter missing (port, endpoint, format) | Reasonable default exists (8080 for dev, UTF-8) |
| Ambiguous instruction with multiple implementations | One interpretation clearly best |
| Task conflicts with design | Issue is minor |

### Architect

| Request clarification when | Proceed without when |
|---------------------------|---------------------|
| Overall goal is unclear or ambiguous | Standard defaults apply (REST, JSON) |
| Missing key constraints (performance, budget, platform) | One interpretation dominates |
| Cannot satisfy all requirements simultaneously | Trade-offs are clear |

### Evaluator

| Request clarification when | Proceed without when |
|---------------------------|---------------------|
| No test criteria specified | Standard test practices apply |
| Missing test inputs or scenarios | Obvious criteria exist |
| Unclear pass/fail thresholds | Partial evaluation possible |

### Auditor

| Request clarification when | Proceed without when |
|---------------------------|---------------------|
| Security policy or threat model undefined | Standard security practices apply |
| Approval criteria ambiguous | Obvious scope (review everything) |
| Review scope undefined | Conservative defaults available |

### Debugger

| Request clarification when | Proceed without when |
|---------------------------|---------------------|
| Cannot reproduce the issue | Standard debugging applies |
| Multiple possible root causes | Obvious reproduction path |
| Missing error context | Most likely cause is clear |

### Researcher

| Request clarification when | Proceed without when |
|---------------------------|---------------------|
| Research scope unclear | Standard research practices |
| Source preferences missing | Obvious scope |
| Depth requirements unknown | Reasonable depth |

## Implementation

This protocol is implemented entirely through instruction changes:

| File | Change |
|------|--------|
| `autonoetic-gateway/src/runtime/foundation_instructions.md` | Rule 13: Clarification Protocol |
| `agents/lead/planner.default/SKILL.md` | "Handling Child Agent Clarification Requests" section |
| `agents/specialists/coder.default/SKILL.md` | "Clarification Protocol" section |
| `agents/specialists/architect.default/SKILL.md` | "Clarification Protocol" section |
| `agents/specialists/evaluator.default/SKILL.md` | "Clarification Protocol" section |
| `agents/specialists/auditor.default/SKILL.md` | "Clarification Protocol" section |
| `agents/specialists/debugger.default/SKILL.md` | "Clarification Protocol" section |
| `agents/specialists/researcher.default/SKILL.md` | "Clarification Protocol" section |

No gateway code changes. No new tools. Uses existing delegation result routing.

## Design Principles

1. **Minimal format**: Two fields (`question`, `context`). The LLM makes only one binary decision: ask or proceed.
2. **Agent-to-agent first**: The primary path is specialist → planner, not specialist → user. The planner decides if user input is needed.
3. **Respawn pattern**: No mid-turn suspension needed. The clarification answer comes in the next turn with a new spawn call.
4. **Previous work continuity**: Respawn includes a reference to the child's previous work via content handles.

## See Also

- [Foundation Instructions](../autonoetic-gateway/src/runtime/foundation_instructions.md) — Rule 13
- [Agent Routing and Roles](./agent_routing_and_roles.md) — Delegation contracts
- [Separation of Powers](./separation-of-powers.md) — Agent vs gateway responsibilities
- [Content Store](./content-store.md) — Content handles for previous work references
