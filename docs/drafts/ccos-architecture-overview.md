# CCOS Architecture Overview

Status: **Current as of 2025-12-08**

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CLI / External API                              │
└──────────────────────────────────┬──────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CCOS (ccos_core.rs)                                │
│  Main entry point - initializes and coordinates all components              │
│  - process_request(nl_request) → Plan + ExecutionResult                     │
│  - validate_and_execute_plan(plan) → ExecutionResult                        │
└───────┬─────────────┬─────────────┬─────────────┬─────────────┬─────────────┘
        │             │             │             │             │
        ▼             ▼             ▼             ▼             ▼
┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐
│  Arbiter  │  │Governance │  │Orchestr.  │  │RuntimeHost│  │Capability │
│           │  │ Kernel    │  │           │  │           │  │Marketplace│
└───────────┘  └───────────┘  └───────────┘  └───────────┘  └───────────┘
        │             │             │             │             │
        └─────────────┴─────────────┴─────────────┼─────────────┘
                                                  │
                                                  ▼
                          ┌─────────────────────────────────────┐
                          │         RTFS Runtime (rtfs)          │
                          │  Pure expression evaluator + stdlib  │
                          └─────────────────────────────────────┘
```

## Core Modules

### 1. CCOS Core (`ccos/src/ccos_core.rs`)

**Role**: Main system entry point and component orchestration

**Key Responsibilities**:
- Initialize all CCOS components (Arbiter, GovernanceKernel, RuntimeHost, etc.)
- Process natural language requests: NL → Intent → Plan → Execution
- Coordinate between Arbiter (planning) and Governance (validation/execution)
- Plan auto-repair pipeline with LLM support

**Key Methods**:
- `CCOS::new()` - Initialize full system
- `process_request(nl_request, context)` - End-to-end NL → Result
- `validate_and_execute_plan(plan, context)` - Run validated plan

---

### 2. Arbiter (`ccos/src/arbiter/`)

**Role**: Convert natural language into structured intents and executable RTFS plans

**Key Components**:
| Component | Purpose |
|-----------|---------|
| `DelegatingArbiter` | Main arbiter - delegates to agents or generates plans via LLM |
| `LlmArbiter` | Uses LLM for plan generation |
| `TemplateArbiter` | Pattern-based plan templates |
| `HybridArbiter` | Combines template + LLM approaches |
| `LlmProvider` | Abstraction over OpenAI/Anthropic/OpenRouter |
| `PromptManager` | Manages LLM prompt templates |

**Flow**:
```
NL Request → Intent Classification → Capability Discovery → Plan Generation → RTFS
```

---

### 3. Governance Kernel (`ccos/src/governance_kernel.rs`)

**Role**: Root of trust - enforces Constitution rules on all executions

**Key Features**:
- **Constitution**: Set of human-authored rules (Allow/Deny/RequireHumanApproval)
- **Pattern Matching**: Rules match capability IDs with patterns (`mcp.*`, `ccos.io.*`)
- **Execution Modes**: `full`, `read-only`, `dry-run`, `require-approval`
- **Security Levels**: `low`, `medium`, `high`, `critical`
- **Delegation Validation**: Pre-approve agent selection before delegation

**Key Methods**:
- `validate_and_execute(plan, context)` - Primary entry point
- `validate_against_constitution(plan, mode)` - Check rules
- `sanitize_intent(intent, plan)` - Detect malicious patterns
- `detect_execution_mode(plan, intent)` - Determine safety level

**Constitution Rules** (default):
```rust
("mcp.*", Allow)           // MCP tools generally allowed
("ccos.io.*", Allow)       // IO operations allowed
("ccos.*", Allow)          // CCOS native capabilities
("*delete*", RequireHumanApproval)  // Destructive actions need approval
```

---

### 4. Orchestrator (`ccos/src/orchestrator.rs`)

**Role**: Executes validated plans through RTFS runtime

**Key Responsibilities**:
- Bridge between GovernanceKernel and RTFS Runtime
- Track execution state in Causal Chain
- Handle step-by-step execution with logging

---

### 5. RuntimeHost (`ccos/src/host.rs`)

**Role**: Bridge between RTFS runtime and CCOS stateful components

**Key Responsibilities**:
- Implement `HostInterface` for RTFS runtime callbacks
- Execute capabilities via CapabilityMarketplace
- Log all actions to CausalChain
- Handle native capability execution (MCP, CLI, etc.)

**Key Methods**:
- `execute_capability(name, args)` - Called by RTFS when evaluating `(call ...)`
- `notify_step_started/completed/failed()` - Lifecycle hooks
- `build_context_snapshot()` - Create audit trail data

---

### 6. Modular Planner (`ccos/src/planner/modular_planner/`)

**Role**: Decompose goals into executable RTFS plans

**Key Components**:
| Component | Purpose |
|-----------|---------|
| `orchestrator.rs` | Main planner coordinator |
| `decomposition/` | Strategies for breaking down goals |
| `resolution/` | Finding capabilities for intents |
| `adapters/` | Schema bridging between tools |

**Planning Flow**:
```
Goal → Decompose → SubIntents → Resolve Capabilities → Generate RTFS
         │              │              │
         ▼              ▼              ▼
   PatternDecomp   IntentGraph    Catalog/MCP
   GroundedLLM                    Discovery
```

---

### 7. Capability Marketplace (`ccos/src/capability_marketplace/`)

**Role**: Registry and discovery of all capabilities

**Sources**:
- **Native**: CLI commands exposed as RTFS (`native_provider.rs`)
- **MCP**: Model Context Protocol tools (GitHub, etc.)
- **Generated**: LLM-synthesized capabilities
- **Catalog**: Pre-registered RTFS capabilities

---

### 8. Causal Chain (`ccos/src/causal_chain/`)

**Role**: Audit trail and observability

**Tracked Data**:
- All actions (capability calls, step starts/completions)
- Decision points (decomposition choices, resolution)
- Execution results and errors
- Metrics (latency, success rates)

---

## Data Flow

### Request Processing
```
1. CLI: ccos plan create "goal"
2. CCOS::process_request(goal)
3. Arbiter: NL → Intent → Decompose
4. ModularPlanner: Goal → SubIntents → Capabilities → RTFS
5. GovernanceKernel: Validate against Constitution
6. Orchestrator → RuntimeHost: Execute RTFS
7. RuntimeHost: (call "capability" args) → CapabilityMarketplace
8. CausalChain: Log all actions
9. Return: ExecutionResult
```

### Constitution Enforcement
```
Plan submitted → GovernanceKernel
  ├─ Sanitize intent (check for malicious patterns)
  ├─ Detect execution mode (full/read-only/dry-run)
  ├─ Validate against constitution rules
  │   └─ For each capability: match patterns → Allow/Deny/Approve?
  └─ If OK: Pass to Orchestrator for execution
```

---

## Configuration Files

| File | Purpose |
|------|---------|
| `config/agent_config.toml` | Main configuration (LLM, MCP, validation) |
| `config/constitution.rtfs` | *TODO*: Constitution rules in RTFS format |
| `config/storage/` | Plans, pending synthesis, approvals |
| `capabilities/` | RTFS capability definitions |

---

## Key Relationships

```
CCOS
 ├── Arbiter (NL→Plan generation)
 │    └── LlmProvider (OpenAI/Anthropic/OpenRouter)
 ├── GovernanceKernel (Constitution enforcement)
 │    └── Constitution (rules)
 ├── Orchestrator (execution coordination)
 │    └── RuntimeHost (RTFS↔CCOS bridge)
 │         └── CapabilityMarketplace (capability registry)
 ├── ModularPlanner (goal decomposition)
 │    ├── DecompositionStrategies
 │    └── ResolutionStrategies
 └── CausalChain (audit trail)
```

---

## Security Model

1. **Constitution-Based**: All actions validated against constitution rules
2. **Execution Modes**: Configurable safety levels per plan
3. **Approval Gates**: High-risk actions require human approval
4. **Audit Trail**: All decisions logged to CausalChain
5. **Capability Security Levels**: low/medium/high/critical per capability
