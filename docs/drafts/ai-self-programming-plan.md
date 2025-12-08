# AI Self-Programming Architecture

## Vision

Enable CCOS agents to **evolve their own capabilities** by treating the planner itself as an RTFS program. The AI can:
- Create, modify, and execute plans
- Synthesize new capabilities when needed
- Validate its own work before committing
- Learn from execution feedback
- All while remaining **introspectable**, **auditable**, and **controllable**

## Core Principles

1. **RTFS-as-Control-Plane**: Plans themselves are RTFS, allowing AI to manipulate plans as data
2. **Capability Reflection**: AI can inspect, extend, and compose capabilities
3. **Governance-Gated Evolution**: Self-modification goes through approval workflows
4. **Causal Traceability**: Every AI decision is logged in the causal chain
5. **Bounded Autonomy**: Configurable limits on what AI can do without human approval

---

## Implementation Status

> **Last Updated**: December 2024

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 1: Meta-Planning | ✅ **Complete** | `planner.decompose`, `planner.resolve_intent`, `planner.synthesize_capability` implemented in [capabilities_v2.rs](../ccos/src/planner/capabilities_v2.rs) |
| Phase 2: Learning Loop | ⬜ Not started | Feedback collection, failure analysis |
| Phase 3: Governance & Safety | ✅ **Complete** | Trust levels, approval gates, bounded exploration, versioning, causal chain recording |
| Phase 4: Introspection | ⬜ Not started | Plan trace, capability graph |
| Phase 5: Evolutionary Agents | ⬜ Not started | Agent memory, coordination |

### Implemented Components

**Phase 1 - Meta-Planning**:
- [capabilities_v2.rs](../ccos/src/planner/capabilities_v2.rs): `planner.build_menu`, `planner.decompose`, `planner.resolve_intent`, `planner.synthesize_capability`
- Sample meta-plan in [capabilities/samples/meta-planner.rtfs](../capabilities/samples/meta-planner.rtfs)

**Phase 3 - Governance & Safety**:
- [SelfProgrammingConfig](../rtfs/src/config/types.rs): Trust levels (0-4), approval thresholds
- [SelfProgrammingSession](../rtfs/src/config/self_programming_session.rs): Bounded exploration limits, approval queue
- [CapabilityVersionStore](../ccos/src/capability_marketplace/version_store.rs): Version snapshots, rollback support
- [GovernanceEventRecorder](../ccos/src/synthesis/governance_events.rs): Causal chain recording for all governance events
- [config/agent_config.toml](../config/agent_config.toml): `[self_programming]` section added

**New Causal Chain ActionTypes**:
- `CapabilityVersionCreated`, `CapabilityRollback` - Version tracking
- `CapabilitySynthesisStarted`, `CapabilitySynthesisCompleted` - Synthesis lifecycle
- `GovernanceApprovalRequested/Granted/Denied` - Approval workflow
- `BoundedExplorationLimitReached` - Limit enforcement

---

## Phase 1: Meta-Planning Capabilities (Foundation)

Expose the planner itself as RTFS-callable capabilities.

### Proposed New Capabilities

| Capability ID | Description | Security |
|--------------|-------------|----------|
| `planner.decompose` | Break a goal into sub-intents | low |
| `planner.resolve_intent` | Find best capability for an intent | low |
| `planner.synthesize_capability` | Create a new capability from spec | high |
| `planner.validate_plan` | Check plan correctness | low |
| `planner.execute_plan` | Run an RTFS plan | high |
| `planner.introspect_result` | Analyze execution output | low |
| `planner.register_capability` | Add a new capability | medium |

### Sample Meta-Plan (RTFS) - Recursive

```clojure
;; Recursive meta-planning: decompose → resolve → synthesize → repeat
(defn resolve-or-decompose [intent max-depth]
  "Recursively resolve an intent, decomposing if needed"
  (if (<= max-depth 0)
    {:error "Max recursion depth reached" :intent intent}
    
    (let [;; Try to resolve directly
          capability (call "planner.resolve_intent" {:intent intent})]
      
      (if capability
        ;; Found! Return resolved intent
        {:resolved true :capability capability :intent intent}
        
        ;; Not found - decompose into sub-intents
        (let [sub-intents (call "planner.decompose" {:goal (:description intent)})
              
              ;; Recursively resolve each sub-intent
              resolved-subs (map (fn [sub] 
                                   (resolve-or-decompose sub (- max-depth 1))) 
                                 sub-intents)
              
              ;; Check if any failed
              failures (filter (fn [r] (not (:resolved r))) resolved-subs)]
          
          (if (empty? failures)
            ;; All resolved - compose the plan
            {:resolved true 
             :composed (call "planner.compose" {:intents resolved-subs})}
            
            ;; Some unresolved - try synthesis
            (let [synthesized (map (fn [f] 
                                     (call "planner.synthesize_capability" 
                                           {:spec (:intent f)}))
                                   failures)]
              ;; Return with synthesis attempts
              {:resolved (every? :success synthesized)
               :synthesized synthesized
               :failures failures})))))))

;; Main entry: recursive planning with governance
(let [goal "group GitHub issues by label and show counts"
      max-depth 3
      
      ;; Create root intent
      root-intent {:description goal :id (uuid)}
      
      ;; Recursive resolution
      result (resolve-or-decompose root-intent max-depth)]
  
  (if (:resolved result)
    ;; Success - generate and validate the plan
    (let [plan (call "planner.generate_rtfs" {:result result})
          validation (call "planner.validate_plan" {:plan plan})]
      
      (if (:valid validation)
        ;; Execute with governance approval
        (call "governance.request_approval" 
              {:action "execute_meta_plan" :plan plan})
        {:error "Validation failed" :issues (:issues validation)}))
    
    ;; Failed - queue for human review
    {:status "pending_synthesis" 
     :failures (:failures result)}))
```

### Recursive Resolution Flow

```
Goal: "group issues by label"
│
├─► Decompose → [list_issues, group_by_label, display]
│
├─► Resolve list_issues → ✅ mcp.github/list_issues
│
├─► Resolve group_by_label → ❌ Not found
│   │
│   └─► Decompose → [extract_labels, count_per_label, format_output]
│       │
│       ├─► Resolve extract_labels → ❌ Not found
│       │   └─► Synthesize → generated/extract_labels ✅
│       │
│       ├─► Resolve count_per_label → ❌ Not found  
│       │   └─► Synthesize → generated/count_per_label ✅
│       │
│       └─► Resolve format_output → ✅ ccos.io.println
│
└─► Compose final RTFS plan from resolved tree
```

### Key Recursive Capabilities

| Capability | Purpose |
|-----------|---------|
| `planner.decompose` | Break intent into sub-intents |
| `planner.resolve_intent` | Find capability for intent |
| `planner.synthesize_capability` | Create missing capability |
| `planner.compose` | Combine resolved sub-intents into plan |
| `planner.validate_plan` | Check plan correctness |
| `governance.request_approval` | Gate execution on human approval |


---

## Phase 2: Learning Loop (Feedback Integration)

Enable the AI to learn from execution results and improve.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Self-Programming Loop                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌─────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐  │
│   │  Goal   │───▶│ Planning │───▶│ Execute  │───▶│ Evaluate │  │
│   └─────────┘    └──────────┘    └──────────┘    └──────────┘  │
│        ▲                                               │        │
│        │           ┌───────────────────────────────────┘        │
│        │           ▼                                            │
│        │    ┌──────────────┐                                    │
│        └────│   Learn      │                                    │
│             └──────────────┘                                    │
│                    │                                            │
│                    ▼                                            │
│             ┌──────────────┐                                    │
│             │  Improve     │ (new capabilities, better plans)   │
│             └──────────────┘                                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### New Capabilities

| Capability ID | Description | Security |
|--------------|-------------|----------|
| `learning.record_feedback` | Store execution outcome | low |
| `learning.analyze_failure` | Diagnose why a plan failed | low |
| `learning.suggest_improvement` | Propose plan/capability changes | low |
| `learning.apply_improvement` | Modify plan or capability | high |

### Implementation

#### [NEW] `ccos/src/learning/feedback_loop.rs`
Track plan execution outcomes and suggest improvements

#### [MODIFY] `ccos/src/synthesis/introspection/schema_refiner.rs`
Use execution feedback to refine capability schemas

---

## Phase 3: Governance & Safety ✅ IMPLEMENTED

Critical controls for autonomous evolution, **enforced by the constitution**.

### Design Decision (User Confirmed)

> **Approval by default** → Constitution-enforced governance gates
> Progressive relaxation as trust is established

### Trust Levels (Constitution-Encoded)

```
Level 0 (Initial):  ALL self-modification requires approval
Level 1 (Cautious): Read-only introspection auto-approved
Level 2 (Trusted):  Pure data transforms auto-approved
Level 3 (Mature):   Capability synthesis auto-approved (with rollback)
Level 4 (Full):     Full autonomy (use with extreme caution)
```

### Safety Mechanisms

1. **Constitution-Based Approval Gates**
   - All self-modification actions check constitution rules
   - Trust level determines what's auto-approved vs. requires human
   - Constitution can be updated (with approval) to relax controls

2. **Bounded Exploration**
   - Max synthesis attempts per session
   - Max plan depth/complexity
   - Resource quotas (LLM calls, execution time)

3. **Rollback Support**
   - Versioned capabilities with rollback
   - Plan history with undo capability
   - Checkpoint/restore for learning state

4. **Audit Trail**
   - Every AI decision logged to causal chain
   - Diff views for capability changes
   - Attribution of who/what triggered changes

### Implementation ✅

> All governance components implemented. See **Implementation Status** section above for file locations.

- ✅ `SelfProgrammingConfig` with trust levels (0-4)
- ✅ `SelfProgrammingSession` with bounded exploration limits
- ✅ `CapabilityVersionStore` with rollback support
- ✅ `GovernanceEventRecorder` for causal chain audit
- ✅ `config/agent_config.toml` `[self_programming]` section


---

## Phase 4: Introspection & Debugging

Let the AI understand and debug itself.

### New Capabilities

| Capability ID | Description |
|--------------|-------------|
| `introspect.capability_graph` | Visualize capability relationships |
| `introspect.plan_trace` | Step-by-step execution trace |
| `introspect.type_analysis` | Analyze data flow types |
| `introspect.causal_chain` | Query the decision history |

### RTFS Extensions

```clojure
;; Self-debugging capability
(capability "self/debug-plan"
  :input-schema [:map [:plan :string] [:issue :string]]
  :output-schema [:map [:diagnosis :string] [:fix [:maybe :string]]]
  :effects [:pure]
  :implementation
  (fn [input]
    (let [trace (call "introspect.plan_trace" {:plan (:plan input)})
          failure-point (find-failure trace)
          diagnosis (call "learning.analyze_failure" 
                         {:trace trace :issue (:issue input)})]
      {:diagnosis (:explanation diagnosis)
       :fix (:suggested_fix diagnosis)})))
```

---

## Phase 5: Evolutionary Agents

Long-term vision for truly autonomous agents.

### Agent Archetypes

1. **Specialist Agent**: Focused on one domain, optimizes for depth
2. **Generalist Agent**: Broad capabilities, synthesizes on demand
3. **Meta-Agent**: Manages other agents, coordinates complex tasks

### Agent Memory

```clojure
;; Persistent agent memory structure
{:agent_id "agent-xyz"
 :capabilities_owned ["generated/..."]
 :execution_history [...last-100-plans...]
 :learned_patterns [{:pattern "..." :confidence 0.85}]
 :preferences {:llm_model "..." :risk_tolerance 0.3}
 :constraints {:max_autonomy_level 3 :require_approval_domains ["finance"]}}
```

### Implementation

#### [NEW] `ccos/src/agents/agent_memory.rs`
Persistent memory for agent learning

#### [NEW] `ccos/src/agents/coordinator.rs`
Multi-agent coordination and task delegation

---

## Verification Plan

### Automated Tests

1. **Meta-plan execution**: Create, execute, and validate a self-modifying plan
2. **Learning loop**: Verify feedback collection and improvement suggestions
3. **Governance gates**: Confirm approval workflows trigger correctly
4. **Rollback**: Test capability versioning and undo

### Manual Verification

1. Run the meta-plan sample and observe AI creating a new capability
2. Intentionally fail a plan and verify the learning loop suggests improvements
3. Test the approval workflow for high-security operations

---

## Implementation Order

| Priority | Phase | Estimated Effort |
|----------|-------|-----------------|
| 1 | Phase 1: Meta-Planning Capabilities | 2-3 days |
| 2 | Phase 3: Governance & Safety | 1-2 days |
| 3 | Phase 2: Learning Loop | 2-3 days |
| 4 | Phase 4: Introspection | 1-2 days |
| 5 | Phase 5: Evolutionary Agents | 3-5 days |

---

## User Review Required

> [!NOTE]
> **RESOLVED**: AI self-modification requires human approval by default (trust_level=0).
> Trust levels can be progressively increased as confidence builds.

> [!NOTE]
> **RESOLVED**: Phase 1 (Meta-Planning) and Phase 3 (Governance) implemented together.
> Next: Phase 2 (Learning Loop) then Phase 4 (Introspection).
