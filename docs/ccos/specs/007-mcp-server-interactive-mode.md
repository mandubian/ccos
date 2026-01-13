# CCOS Specification 007: MCP Server & Interactive Mode

**Status:** Implemented
**Version:** 1.0
**Date:** 2026-01-12
**Related:** [000: Architecture](./000-ccos-architecture.md), [002: Plans and Orchestration](./002-plans-and-orchestration.md)

## Introduction: The "Body" without the "Brain"

While the core CCOS specifications describe an **Autonomous Mode**—where CCOS maintains the Intent Graph, acts as the Cognitive Engine, and drives execution via the Orchestrator—implementation reality often requires a "Human-in-the-Loop" or "External-Agent-in-the-Loop" approach.

This is **Interactive Mode**, primarily exposed via the **Model Context Protocol (MCP)** server.

In Interactive Mode, CCOS acts as the **Body** (Capabilities, Memory, Tools) and **Hippocampus** (Context, Audit Log), while the **Brain** (Intent, Planning, Decision Making) is external—hosted by an IDE agent (e.g., Cursor, Windsurf) or a Chat Interface (e.g., Claude Desktop).

## Core Concepts

### 1. The Mode Duality

| Feature | Autonomous Mode (Target) | Interactive Mode (Current MCP) |
| :--- | :--- | :--- |
| **Driver** | CCOS Internal Cognitive Engine | External Agent (MCP Client) |
| **Intent Source** | Intent Graph (Persistent) | Chat Context / User Prompt |
| **Execution Unit** | RTFS Plan (Compiled IR) | `Session` (Linear Steps) |
| **State Management**| Orchestrator (Reentrant) | Session Manager (Step-by-Step) |
| **Role of CCOS** | Full Agent | Powerful Tool Use Backend |

### 2. The `Session` Model

In Interactive Mode, the complex `IntentGraph` is replaced by a simplified linear `Session` structure. A Session tracks a single thread of execution driven by the external agent.

**Structure**:
```rust
pub struct Session {
    pub id: String,
    pub goal: String,          // The "Prompt" or high-level objective
    pub steps: Vec<ExecutionStep>,
    pub context: HashMap<String, Value>, // Shared scratchpad
}

pub struct ExecutionStep {
    pub tool_name: String,
    pub inputs: Value,
    pub result: Value,         // The return value from the tool
    pub success: bool,
    pub rtfs_code: String,     // The pure representation of this step
}
```

### 3. Execution Flow

1.  **Start**: The External Agent initializes a session (or implicit session created on first tool call).
2.  **Act**: The External Agent decides to call a tool (e.g., `fs.read_file`).
3.  **Execute**:
    *   CCOS receives the tool call request.
    *   CCOS maps the tool to an internal **Capability**.
    *   CCOS executes the capability (via the same underlying `SafeExecutor` mechanisms as Autonomous Mode).
    *   CCOS records the action in the **Causal Chain** and adds a step to the `Session`.
4.  **Return**: The result is sent back to the External Agent.
5.  **Loop**: The External Agent ingests the result and decides the next step.

### 4. Architectural Alignment

Despite the simpler flow, Interactive Mode adheres to CCOS core principles:

1.  **Capabilities**: All actions are routed through the same Capability Marketplace. The MCP Server is just a protocol adapter exposing these capabilities.
2.  **Purity (Local)**: Each step is effectively a discrete "Unit of Work". The `Session` logic generates a valid RTFS snippet for each step (`(call "tool" inputs)`), enabling the session to be replayed or exported as a valid RTFS Plan later.
3.  **Causal Chain**: Every tool execution is logged to the Causal Chain, ensuring auditability regardless of who is driving (Internal Cognitive Engine or External Agent).

### 5. Security & Governance

Interactive Mode operates under a distinct security profile compared to Autonomous Mode, characterized by "Static Safety" rather than "Semantic Governance".

#### 5.1 The "Implicit Trust" Model
In MCP Mode, CCOS assumes the **External Agent (Brain)** is the primary authority on user intent.
*   **No Governance Kernel**: The `GovernanceKernel` (the "Pre-Frontal Cortex") is generally bypassed for blocking actions, though the **Constitution Discovery** tools allow the External Agent to proactively check rules.
*   **Voluntary Compliance**: Agents can call `ccos_get_constitution` to read the system's rules (e.g., "no-global-thermonuclear-war") and `ccos_get_guidelines` for operating instructions, enabling them to self-regulate.

#### 5.2 Enforcement Mechanisms
While semantic governance is dormant, strict **Static Safety** mechanisms are enforced by the `CapabilityMarketplace` for every call:
1.  **Isolation Policy**:
    *   **Allow/Deny Lists**: Capabilities are explicitly permitted or blocked by ID patterns.
    *   **Time Constraints**: Execution is restricted to allowed time windows.
    *   **Namespace Policies**: Access is controlled based on capability namespaces.
2.  **Resource Monitoring**:
    *   Hard limits on CPU and Memory usage per execution.
    *   Rate limiting (if configured).
3.  **Schema Validation**:
    *   Strict type checking of input and output JSON against the capability's RTFS manifest.
4.  **Audit Trail**:
    *   Every action is immutably logged to the **Causal Chain**, providing "detective" security even if "preventive" governance is relaxed.

## 6. Session Consolidation (Trace-to-Agent)

To bridge the gap between "one-off interactive tool use" and "reusable autonomous agents", Interactive Mode supports **Consolidation**.

### 6.1 The Workflow
1.  **Interactive Exploration**: The user works with the External Agent (e.g., "Build and deploy this app") using various tools.
2.  **Validation**: The user confirms the session was successful.
3.  **Consolidation Request**: The user asks to "Save this workflow as a new Agent".
4.  **Synthesis**:
    *   CCOS calls `planner.synthesize_agent_from_trace` (see [033-capability-importers-and-synthesis.md](./033-capability-importers-and-synthesis.md)).
    *   The linear session is converted into a governed **Agent Capability** (`:kind :agent`).
5.  **Autonomous Reuse**: The new Agent is now available for future autonomous execution, subject to the Governance Kernel.

---

## 7. Tangible Learning
> This section details the "Hippocampus" role of CCOS in Interactive Mode.

While the External Agent manages short-term context, CCOS provides **Tangible Learning**—a persistent, structured memory that survives across sessions and agents.

### 7.1 Explicit Memory Tools
Interactive agents are expected to use these tools to "teach" the system:

1.  **`ccos_log_thought`**: Records reasoning. Essential for debugging failures later.
    *   *Usage*: "I am choosing tool X because..."
2.  **`ccos_record_learning`**: Explicitly crystallizes a lesson.
    *   *Usage*: "The `list_issues` tool fails if the repository name is not fully qualified."
3.  **`ccos_recall_memories`**: Retrieves relevant past learnings.
    *   *Usage*: "Before I start, what do we know about this codebase?"

### 7.2 The Learning Cycle
1.  **Recall**: Agent starts a task → Calls `recall_memories` (tags: "github", "current-project").
2.  **Act**: Agent executes tools → Logs thoughts via `log_thought`.
3.  **Reflect**: Agent succeeds or fails → Calls `record_learning` with the outcome.
4.  **Consolidate**: (Optional) User triggers `ccos_consolidate_session` to turn the whole experience into an executable asset.

---

## 8. Strategic Value

Interactive Mode serves as the **Training Ground** for Autonomous Mode:
*   It allows us to test Capabilities and the Causal Chain in isolation.
*   It accumulates "Gold Standard" execution traces (Sessions) that can be used to fine-tune the Internal Cognitive Engine's planning logic.
*   It provides immediate utility to users (Code Assistant backends) while the complex autonomous subsystems are being matured.

---

## Appendix A: Interactive Tool Reference

| Tool Name | Purpose | Phase |
| :--- | :--- | :--- |
| **`ccos_execute_capability`** | **Primary**. Execute any capability with JSON. Auto-manages sessions. | Execution |
| `ccos_search` | Search for capabilities. | Discovery |
| `ccos_suggest_apis` | Ask LLM for API ideas (non-binding). | Discovery |
| `ccos_plan` | Decompose high-level goal using CCOS Planner. | Planning |
| `ccos_session_start` | Explicitly start a session. | Session |
| `ccos_session_end` | End session, save plan. | Session |
| `ccos_session_plan` | Get current RTFS plan. | Session |
| `ccos_consolidate_session` | Convert session trace to Agent Capability. | Synthesis |
| `ccos_synthesize_capability` | Generate new RTFS capability. | Synthesis |
| `ccos_log_thought` | Record reasoning to AgentMemory. | Learning |
| `ccos_record_learning` | Record explicit lesson. | Learning |
| `ccos_recall_memories` | Recall past learnings. | Learning |
| `ccos_get_constitution` | Read system rules. | Governance |
| `ccos_get_guidelines` | Read agent manual. | Governance |

