# 015 — Capabilities vs Agents (CCOS Core Concepts)

## Purpose
Provide a precise, minimal, operational distinction between a capability and an agent in CCOS, with practical rules, metadata, governance hooks, and examples that align the implementation with the intent graph, orchestration, and marketplace.

## Definitions

- Capability
  - A single-shot callable unit with a stable contract (inputs, outputs, effects, limits).
  - Stateless by default (no persistent working memory), bounded execution.
  - Invoked via `(call :capability/id {...})`.
  - May internally compose other capabilities in a fixed pipeline; remains a capability if it does not plan/search/iterate or delegate with autonomy.

- Agent
  - A goal-directed controller that plans/chooses which capabilities to call, possibly iteratively, and adapts to outcomes.
  - May use Cognitive Engine/Delegation, ask the user (human-in-loop), checkpoint/resume, learn or synthesize new capabilities.
  - Stateful across steps (working memory, context horizon), and can be long-lived.

## Rule of Thumb (Decider)
- Capability if: single deterministic pipeline (even if calling other capabilities), no runtime selection/search/looping, no Cognitive Engine/Delegation control flow.
- Agent if: selects among capabilities dynamically, loops/branches based on results, uses human-in-loop, Cognitive Engine/Delegation, or maintains state/checkpoints.

This keeps the surface minimal while mapping cleanly to orchestration responsibilities.

## Unified Artifact Model (IMPLEMENTED)
Both capabilities and agents use the same artifact form (capability spec) in the CapabilityMarketplace, with discriminating metadata flags:

```rtfs
(capability "domain.entity.action.v1"
  :description "..."
  :parameters {...}
  :metadata {
    :kind :primitive | :composite | :agent
    :planning false | true          ; uses Cognitive Engine/Delegation/market discovery
    :stateful false | true          ; uses working memory / checkpoints
    :interactive false | true       ; uses ccos.user.ask / human gates
  }
  :implementation
    (do ...))
```

- **:primitive**: leaf capability (e.g., HTTP fetch, file read)
- **:composite**: fixed pipeline of sub-capabilities, no autonomy
- **:agent**: planning/selection/iteration allowed (autonomy)

### Implementation Status
✅ **CapabilityMarketplace** registers all artifacts (capabilities and agents)  
✅ **CapabilityQuery** filters by `:kind`, `:planning`, `:stateful`, `:interactive`  
✅ **DelegatingEngine** queries marketplace instead of a separate agent registry  
✅ **AgentMetadata** struct extends CapabilityManifest with agent-specific fields

## Governance and Security Gates
- Capabilities (non-agent)
  - Must declare `:effects`, resource `:limits`, and optional input/output schemas.
  - No Cognitive Engine usage; single-shot bounded execution.
  - Attestation focuses on inputs/outputs/effects; strict resource enforcement.

- Agents
  - Allowed to use Cognitive Engine/Delegation, working memory, checkpoint/resume, human gates.
  - Governance policies cover: marketplace selection, provider attestation, user-data handling, continuations, and long-running control loops.
  - Additional audit requirements: causal chain linking plan → actions → outcomes.

## Orchestration Mapping
- Capability execution
  - Intent → Plan → single execution segment
  - Causal chain logs one CapabilityCall per sub-call
  - No plan mutation at runtime

- Agent execution
  - Intent → Plan template → runtime planning cycles (delegate/select/execute)
  - Checkpoint/resume across cycles (CCOS 017)
  - Human gates when `:interactive true`
  - Causal chain includes Delegation/Selection decisions and retries

## Examples

1) Primitive capability
```rtfs
(capability "ccos.network.http-fetch.v1"
  :description "HTTP GET"
  :parameters {:url :string :timeout_ms :number}
  :metadata {:kind :primitive :planning false :stateful false :interactive false}
  :implementation
    (do (call :ccos.http.get {:url url :timeout_ms timeout_ms})))
```

2) Composite capability (still a capability)
```rtfs
(capability "travel.trip-planner.compose.v1"
  :description "Fixed-pipeline trip planning"
  :parameters {:destination :string :duration :number :budget :currency}
  :metadata {:kind :composite :planning false :stateful false :interactive false}
  :implementation
    (do
      (let flights (call :travel.flights {:destination destination :budget budget}))
      (let hotels  (call :travel.hotels  {:city destination :budget budget :duration duration}))
      (let itin    (call :travel.itinerary {:days duration :flights flights :hotels hotels}))
      {:status :ok :itinerary itin}))
```

3) Agent (planner/controller)
```rtfs
(capability "travel.trip-planner.agent.v1"
  :description "Goal-directed trip planner (selects providers, interacts, retries)"
  :parameters {:destination :string :duration :number :budget :currency}
  :metadata {:kind :agent :planning true :stateful true :interactive true}
  :implementation
    (do
      (let providers (call :market.discover {:capability :travel.flights :constraints {:budget budget}}))
      (let chosen    (call :cognitive engine.select {:options providers :criteria {:price_weight 0.6 :reliability 0.4}}))
      (let clarify   (call :ccos.user.ask "Any stopovers acceptable?"))
      (let flights   (call :provider.flights/search {:provider chosen :destination destination :budget budget :prefs clarify}))
      (let hotels    (call :provider.hotels/search  {:destination destination :duration duration :budget budget}))
      (let itin      (call :planner.compose {:days duration :flights flights :hotels hotels}))
      (call :checkpoint.save {:state {:flights flights :hotels hotels}})
      {:status :ok :itinerary itin}))
```

## Design Guidance
- Start as a capability (primitive/composite). Promote to agent only when you need autonomy (planning/selection/iteration/state/human-in-loop).
- Keep the spec surface small: one artifact shape with metadata flags.
- Apply stricter governance automatically when `:kind :agent` or `:planning true`.
- Expose `:kind` and flags in marketplace/registry UIs for operator clarity.

## Migration Status
The agent unification is **complete** for core functionality:
- ✅ Single registry (CapabilityMarketplace) for all artifacts
- ✅ Unified metadata model with agent-specific flags
- ✅ Cognitive Engine/Delegation queries marketplace with filters
- ✅ Backward compatibility maintained during transition
- ✅ AgentRegistryShim provides marketplace-backed compatibility
- ✅ Deprecated types marked with deprecation warnings
- ✅ Documentation updated with migration notes

**Remaining cleanup tasks** (see 016-agent-unification-migration-plan.md):
- Update governance policies for agent-specific enforcement (pending)
- Remove deprecated types after release cycle (pending)

## Rationale
This preserves the simplest mental model:
- Capability = callable, bounded function (can be composite but not autonomous).
- Agent = planner/controller with autonomy (delegation, state, interaction).

It minimizes concepts (single artifact) while retaining essential distinctions for security, governance, and runtime behavior.
