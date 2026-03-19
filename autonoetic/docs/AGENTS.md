# Autonoetic Agents

> Reference for the agent system: roles, routing, SKILL.md format, capabilities, and the agent lifecycle.

## Table of Contents

- [Agent Basics](#agent-basics)
- [Roles and Routing](#roles-and-routing)
- [SKILL.md Format](#skillmd-format)
- [Capabilities System](#capabilities-system)
- [Content and Memory Tools](#content-and-memory-tools)
- [Agent Lifecycle](#agent-lifecycle)
- [Script vs Reasoning Agents](#script-vs-reasoning-agents)
- [Building New Agents](#building-new-agents)

---

## Agent Basics

An agent is a **SKILL.md manifest** + **instructions** that runs inside a sandbox. Key principles:

- **Pure reasoner**: Proposes actions, never executes directly
- **Capability-declared**: All permissions declared in manifest YAML
- **Content-addressed**: Uses content.write/read for file operations
- **Role-based**: Each agent fills a specific role (planner, coder, researcher, etc.)

### Agent vs Role vs Template

| Concept | Description | Example |
|---------|-------------|---------|
| **Role** | A contract defining what an agent does | "coder" |
| **Agent Template** | A SKILL.md that implements a role | `coder.default/SKILL.md` |
| **Agent Instance** | A deployed agent directory | `agents/coder.default/` |
| **Learned Specialization** | A promoted agent that proved useful | `weather.script.default` |

---

## Roles and Routing

### Routing Rules

When a message arrives at the gateway:

1. **Explicit target**: Route to specified `target_agent_id`
2. **Session affinity**: Route to session-bound lead agent
3. **Default lead**: Route to `default_lead_agent_id` (typically `planner.default`)
4. **Fail**: If no default lead configured, reject

### Primary Roles

| Role | Agent ID | Purpose |
|------|----------|---------|
| **Lead** | `planner.default` | Decomposes goals, routes to specialists |
| **Researcher** | `researcher.default` | Gathers evidence, cites sources |
| **Architect** | `architect.default` | Defines structure, interfaces, trade-offs |
| **Coder** | `coder.default` | Produces runnable artifacts |
| **Debugger** | `debugger.default` | Isolates root causes, proposes fixes |
| **Evaluator** | `evaluator.default` | Validates behavior with tests/metrics |
| **Auditor** | `auditor.default` | Checks security, governance, reproducibility |

### Evolution Roles

| Role | Agent ID | Purpose |
|------|----------|---------|
| **Builder** | `specialized_builder.default` | Installs new durable agents |
| **Adapter** | `agent-adapter.default` | Generates wrapper agents for I/O gaps |
| **Curator** | `memory-curator.default` | Distills durable learnings |
| **Steward** | `evolution-steward.default` | Decides skill promotion |

### Delegation Ladder (for Planner)

1. **Reuse first**: `agent.discover` → spawn existing match
2. **Adapt**: `agent-adapter.default` → wrapper for I/O gaps
3. **Install**: `specialized_builder.default` → new durable agent
4. **Delegate**: `agent.spawn` → one-shot specialist execution

### Delegation Contract

When calling `agent.spawn`, include structured metadata:

```json
{
  "agent_id": "coder.default",
  "message": {...},
  "metadata": {
    "delegated_role": "coder",
    "delegation_reason": "Implement weather API integration",
    "expected_outputs": ["main.py", "SKILL.md"],
    "parent_goal": "Create a weather agent"
  }
}
```

---

## SKILL.md Format

### Frontmatter Structure

```yaml
---
name: "agent.id"
description: "One-line description"
metadata:
  autonoetic:
    version: "1.0"
    runtime:
      engine: "autonoetic"
      gateway_version: "0.1.0"
      sdk_version: "0.1.0"
      type: "stateful"
      sandbox: "bubblewrap"
      runtime_lock: "runtime.lock"
    agent:
      id: "agent.id"
      name: "Human Name"
      description: "Detailed description"
    llm_config:
      provider: "openai"       # openai, anthropic, gemini, openrouter, etc.
      model: "gpt-4o"
      temperature: 0.1
    capabilities:
      - type: "ToolInvoke"
        allowed: ["content.", "knowledge.", "agent."]
      - type: "MemoryWrite"
        scopes: ["self.*", "skills/*"]
      - type: "AgentSpawn"
        max_children: 10
    execution_mode: "reasoning"   # "reasoning" (default) or "script"
    script_entry: "main.py"       # Required for script mode
    gateway_url: "http://..."     # Optional: remote gateway URL
    gateway_token: "secret"       # Optional: remote gateway auth
    io:
      accepts:
        type: object
        required: [task]
        properties:
          task:
            type: string
      returns:
        type: object
    validation: "soft"            # "soft" for LLM, "strict" for scripts
    middleware:
      pre_process: "python3 scripts/normalize_input.py"
    disclosure:
      defaults:
        "state/*": "confidential"
---
# Agent Instructions

Markdown body with natural language instructions.
```

### Key Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Fully qualified agent ID |
| `description` | Yes | One-line description |
| `metadata.autonoetic.agent.id` | Yes | Agent ID (must match directory name) |
| `metadata.autonoetic.llm_config` | For reasoning | LLM provider/model |
| `metadata.autonoetic.capabilities` | No | Permission declarations |
| `metadata.autonoetic.execution_mode` | No | `"reasoning"` (default) or `"script"` |
| `metadata.autonoetic.script_entry` | For script mode | Entry script path |
| `metadata.autonoetic.io` | No | JSON Schema for input/output |
| `metadata.autonoetic.validation` | No | `"soft"` (LLM) or `"strict"` (script) |

### Markdown Body

The body contains natural language instructions for the agent. Key sections typically include:
- Role description
- Tool usage instructions
- Rules and constraints
- Output format guidance

---

## Capabilities System

### Capability Categories

Capabilities fall into three categories:

| Category | Purpose | Examples |
|----------|---------|----------|
| **Tool Access** | Which tools/commands can be invoked | `SandboxFunctions`, `CodeExecution` |
| **Storage Access** | Which paths/scopes can be read/written | `ReadAccess`, `WriteAccess` |
| **Privilege Escalation** | Operations that escape sandbox/agent boundaries | `NetworkAccess`, `AgentSpawn` |

### Available Capabilities

| Capability | Fields | Description |
|------------|--------|-------------|
| `SandboxFunctions` | `allowed: [string]` | MCP tool access by prefix (e.g., `web.*`, `sandbox.*`) |
| `ReadAccess` | `scopes: [string]` | Read access to content, memory, knowledge (includes search) |
| `WriteAccess` | `scopes: [string]` | Write access to content, memory, knowledge (includes share) |
| `NetworkAccess` | `hosts: [string]` | HTTP/network access to specific hosts |
| `CodeExecution` | `patterns: [string]` | Execute scripts/commands in sandbox |
| `AgentSpawn` | `max_children: number` | Create child agent sessions |
| `AgentMessage` | `patterns: [string]` | Send messages to other agents |
| `BackgroundReevaluation` | `min_interval_secs: number, allow_reasoning: boolean` | Periodic wake-ups for background processing |

### Capability Semantics

**Storage capabilities control all data operations:**

| Capability | Gates These Tools |
|------------|------------------|
| `ReadAccess` | `content.read`, `artifact.inspect`, `memory.read`, `knowledge.recall`, `knowledge.search` |
| `WriteAccess` | `content.write`, `artifact.build`, `memory.write`, `knowledge.store`, `knowledge.share` |

**Privilege capabilities gate boundary-crossing operations:**

| Capability | Controls |
|------------|----------|
| `NetworkAccess` | HTTP requests via `web.fetch`, `web.search` |
| `CodeExecution` | Script execution via `sandbox.exec` |
| `AgentSpawn` | Creating new agent sessions |

### Scoping

Capabilities use pattern-based scoping:
- `*` = wildcard (all access)
- `self.*` = own agent's state
- `skills/*` = installed skills directory
- `scripts/*` = scripts directory
- `api.*` = API-related state

### Adding New Capabilities

Capabilities are defined in `autonoetic-types/src/capability.rs` as a Rust enum. To add a new capability:

1. Add a variant to the `Capability` enum
2. Implement the policy check in `policy.rs`
3. Gate the relevant tool(s) in `is_available()` 
4. Update this documentation

Example: Adding a hypothetical `ImageGenerate` capability:
```rust
// In capability.rs
pub enum Capability {
    // ... existing variants ...
    ImageGenerate {
        max_size: u32,
        allowed_formats: Vec<String>,
    },
}
```

---

## Content and Memory Tools

### Content Tools (Working Memory)

For files and data within a session:

| Tool | Signature | Description |
|------|-----------|-------------|
| `content.write` | `(name: string, content: string, visibility?: string) → handle` | Write content with visibility (private/session/global). Default: session |
| `content.read` | `(name_or_handle: string) → content` | Read by name or handle with root-based resolution |

### Artifact Tools (Trust Boundary)

For reviewable/installable file bundles:

| Tool | Signature | Description |
|------|-----------|-------------|
| `artifact.build` | `(inputs: [string], entrypoints?: [string]) → artifact` | Build immutable artifact from session content |
| `artifact.inspect` | `(artifact_id: string) → artifact` | Inspect artifact files, entrypoints, digest |

### Knowledge Tools (Durable Memory)

For facts with provenance across sessions:

| Tool | Signature | Description |
|------|-----------|-------------|
| `knowledge.store` | `(id: string, content: string, scope: string) → void` | Store fact |
| `knowledge.recall` | `(id: string) → fact` | Retrieve fact |
| `knowledge.search` | `(scope: string, query: string) → [facts]` | Search facts |
| `knowledge.share` | `(id: string, agents: [string]) → void` | Share with agents |

### Agent Tools

| Tool | Signature | Description |
|------|-----------|-------------|
| `agent.spawn` | `(agent_id: string, message: any, ...) → result` | Spawn child agent |
| `agent.install` | `(agent_id: string, files: [...], ...) → result` | Install new agent |
| `agent.exists` | `(agent_id: string) → bool` | Check if agent exists |
| `agent.discover` | `(intent: string, ...) → [candidates]` | Find reusable agents |

---

## Agent Lifecycle

### Wake → Context → Reason → Hibernate

```
1. WAKE: Gateway receives event.ingest or agent.spawn
2. CONTEXT ASSEMBLY:
   - Load SKILL.md instructions
   - Inject foundation instructions
   - Load session context (if re-entering session)
   - Load conversation history (if forked session)
3. REASONING LOOP:
   - Build completion request (messages + tools)
   - Call LLM (or skip for script mode)
   - Dispatch tool calls
   - Check stop reason:
     * end_turn → break
     * tool_use → execute, add result, continue
     * max_tokens → break
   - Apply disclosure filter to response
4. HIBERNATE:
   - Log session end in causal chain
   - Persist conversation history via content.write
   - Update session context
   - Return response through ingress channel
```

### Script Agent Fast Path

For `execution_mode: "script"` agents:

```
1. WAKE: Gateway receives event.ingest or agent.spawn
2. SCRIPT EXECUTION:
   - Resolve script path from manifest
   - Build sandbox command
   - Execute directly (no LLM)
   - Capture stdout as reply
3. HIBERNATE:
   - Log script.completed/failed
   - Return response
```

---

## Script vs Reasoning Agents

### Decision Guide

| Task Type | Mode | Why |
|-----------|------|-----|
| API calls (weather, stocks) | `script` | Deterministic, fast (~100ms) |
| Data transforms (JSON→CSV) | `script` | No ambiguity |
| Simple lookups | `script` | Direct execution |
| Status checks | `script` | Fixed format |
| Code review | `reasoning` | Needs judgment |
| Research + synthesis | `reasoning` | Requires interpretation |
| Ambiguous requirements | `reasoning` | Needs clarification |

### Script Agent Requirements

```yaml
execution_mode: "script"
script_entry: "scripts/main.py"  # Must exist and be executable
# No llm_config needed for script agents
```

**Script interface:**
- Reads JSON from stdin
- Writes JSON to stdout
- Receives input via `SCRIPT_INPUT` env var
- Has access to `AGENT_DIR` env var

### Reasoning Agent Requirements

```yaml
execution_mode: "reasoning"  # or omit (default)
llm_config:
  provider: "openai"
  model: "gpt-4o"
  temperature: 0.1
```

---

## Building New Agents

### Quick Start: Script Agent

1. Create directory: `agents/my.agent/`
2. Create `SKILL.md`:
   ```yaml
   ---
   name: "my.agent"
   description: "My script agent"
   metadata:
     autonoetic:
       version: "1.0"
       runtime:
         engine: "autonoetic"
         gateway_version: "0.1.0"
         sdk_version: "0.1.0"
         type: "stateful"
         sandbox: "bubblewrap"
         runtime_lock: "runtime.lock"
       agent:
         id: "my.agent"
         name: "My Agent"
         description: "My script agent"
       execution_mode: "script"
       script_entry: "main.py"
       capabilities:
         - type: "MemoryWrite"
           scopes: ["self.*"]
   ---
   # My Agent
   This agent does X when given Y input.
   ```
3. Create `main.py`:
   ```python
   #!/usr/bin/env python3
   import json, sys
   from autonoetic_sdk import Client

   sdk = Client()

   def main():
       input_data = json.load(sys.stdin)
       # Do work
       result = {"status": "ok", "data": ...}
       print(json.dumps(result))

   if __name__ == "__main__":
       main()
   ```

### Quick Start: Reasoning Agent

1. Create directory: `agents/my.reasoning.agent/`
2. Create `SKILL.md`:
   ```yaml
   ---
   name: "my.reasoning.agent"
   description: "My reasoning agent"
   metadata:
     autonoetic:
       version: "1.0"
       runtime: {engine: "autonoetic", ...}
       agent:
         id: "my.reasoning.agent"
         name: "My Reasoning Agent"
       llm_config:
         provider: "openai"
         model: "gpt-4o"
       capabilities:
         - type: "ToolInvoke"
           allowed: ["content.", "knowledge."]
         - type: "MemoryWrite"
           scopes: ["self.*", "skills/*"]
       validation: "soft"
   ---
   # Instructions
   You are a [role]. When given [input], you should:
   1. [Step 1 using content.write for files]
   2. [Step 2]
   ...
   ```
3. Create `runtime.lock`:
   ```toml
   lock_version = "1.0"
   [[dependencies]]
   name = "autonoetic_sdk"
   version = "*"
   ```

### Installing Agents

**Via CLI:**
```bash
autonoetic agent install ./path/to/agent/
```

**Via Planner → specialized_builder:**
```
Planner: "Create a weather agent"
  → Spawns specialized_builder
  → specialized_builder calls agent.install
  → Agent is registered and discoverable
```

**Via SDK (in code):**
```python
from autonoetic_sdk import Client
sdk = Client()
sdk.files.write("my_agent/SKILL.md", skill_content)
# Agent install via agent.install tool
```

### Agent Validation

Before installation, the system validates:
1. `agent_id` matches directory name
2. Required fields present (name, description, agent.id)
3. For script mode: `script_entry` exists and is non-empty
4. Capabilities are valid enum objects with correct fields
5. For risk-based approval: classification of install risk

### Discovery

After installation, agents are discoverable:
```python
results = sdk.tools.invoke("agent.discover", {
    "intent": "fetch weather data",
    "required_capabilities": ["NetConnect"]
})
# Returns ranked list with scores
```
