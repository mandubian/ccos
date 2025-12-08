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

### Sample Meta-Plan (RTFS)

```clojure
;; A plan that creates and tests a new capability
(let [goal "group issues by label and count"
      ;; Step 1: Decompose the goal
      intents (call "planner.decompose" {:goal goal})
      
      ;; Step 2: Find missing capabilities
      missing (filter (fn [i] (nil? (call "planner.resolve_intent" {:intent i}))) intents)
      
      ;; Step 3: Synthesize missing capabilities
      synthesized (map (fn [m] (call "planner.synthesize_capability" {:spec m})) missing)
      
      ;; Step 4: Generate the plan
      plan (call "planner.generate_rtfs" {:intents intents :capabilities synthesized})
      
      ;; Step 5: Validate before execution
      validation (call "planner.validate_plan" {:plan plan})]
  
  ;; Only execute if validation passes
  (if (:valid validation)
    (call "planner.execute_plan" {:plan plan})
    {:error "Validation failed" :issues (:issues validation)}))
```

### Implementation

#### [NEW] `ccos/src/planner/capabilities_meta.rs`
Meta-planning capabilities that wrap the orchestrator

#### [MODIFY] `ccos/src/capabilities/native_provider.rs`
Register meta-planning capabilities alongside CLI capabilities

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

## Phase 3: Governance & Safety

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

### Implementation

#### [MODIFY] `ccos/src/governance_kernel.rs`
Add self-modification policies with trust levels

#### [NEW] `ccos/src/capabilities/versioning.rs`
Version control for capabilities with rollback

#### [MODIFY] `config/constitution.rtfs`
Add self-programming governance rules

#### [MODIFY] `config/agent_config.toml`
```toml
[self_programming]
enabled = true
trust_level = 0  # Start with most restrictive
max_synthesis_per_session = 10
max_plan_depth = 5
```


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

> [!IMPORTANT]
> **Key Design Decision**: Should AI self-modification require human approval by default, or should we allow fully autonomous mode with rollback?

> [!IMPORTANT]
> **Scope Question**: Start with Phase 1 (Meta-Planning) only, or include Phase 3 (Governance) in the first iteration?
