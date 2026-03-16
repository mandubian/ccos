# Agent Capabilities Reference

## Overview

This document describes the capability system used by CCOS agents. Capabilities define what tools and resources an agent can access.

## Capability Types

| Capability | Required Fields | Purpose |
|------------|-----------------|---------|
| `SandboxFunctions` | `allowed: [string]` | Access to MCP tools by prefix |
| `ReadAccess` | `scopes: [string]` | Read content, memory, knowledge |
| `WriteAccess` | `scopes: [string]` | Write content, memory, knowledge |
| `CodeExecution` | `patterns: [string]` | Execute scripts in sandbox |
| `NetworkAccess` | `hosts: [string]` | Make HTTP requests |
| `AgentSpawn` | `max_children: integer` | Create child agent sessions |
| `AgentMessage` | `patterns: [string]` | Send messages to other agents |

## Tool Capability Mapping

### Content Tools
| Tool | Requires Capability | Notes |
|------|---------------------|-------|
| `content.read` | `ReadAccess` | Read from content store |
| `content.write` | `WriteAccess` | Write to content store |
| `content.persist` | `WriteAccess` | Make content persistent |
| `content.search` | `ReadAccess` | Search content (included) |

### Agent Tools
| Tool | Requires Capability | Notes |
|------|---------------------|-------|
| `agent.spawn` | `AgentSpawn` | Spawn child agent sessions |
| `agent.install` | `SandboxFunctions: ["agent."]` | Install new agents (evolution roles only) |
| `agent.exists` | `SandboxFunctions: ["agent."]` | Check if agent exists |
| `agent.discover` | `SandboxFunctions: ["agent."]` | Discover available agents |

### Knowledge Tools
| Tool | Requires Capability | Notes |
|------|---------------------|-------|
| `knowledge.recall` | `ReadAccess` | Recall stored knowledge |
| `knowledge.store` | `WriteAccess` | Store new knowledge |
| `knowledge.search` | `ReadAccess` | Search knowledge base |
| `knowledge.share` | `WriteAccess` | Share knowledge |

### Sandbox Tools
| Tool | Requires Capability | Notes |
|------|---------------------|-------|
| `sandbox.exec` | `CodeExecution` | Execute scripts with patterns |

## Important: SandboxFunctions vs Native Tools

**Common misconception**: `SandboxFunctions` with prefix `"content."` grants access to `content.read`, `content.write`, etc.

**Reality**: `SandboxFunctions` is for **MCP (Model Context Protocol) tools only**. Native content tools require `ReadAccess` and `WriteAccess` capabilities.

```
❌ WRONG:
  capabilities:
    - type: "SandboxFunctions"
      allowed: ["content."]  # This does NOT grant content.read access!

✅ CORRECT:
  capabilities:
    - type: "ReadAccess"
      scopes: ["self.*"]
    - type: "WriteAccess"
      scopes: ["self.*"]
```

## Agent Capability Matrix

### Standard Agents

| Agent | ReadAccess | WriteAccess | CodeExecution | NetworkAccess | AgentSpawn | Can Install Agents |
|-------|------------|-------------|---------------|---------------|------------|-------------------|
| **planner.default** | ✅ | ✅ | ❌ | ❌ | ✅ (10) | ❌ Delegates to specialized_builder |
| **specialized_builder.default** | ✅ | ✅ | ❌ | ❌ | ✅ (5) | ✅ **EXCLUSIVE** |
| **coder.default** | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| **researcher.default** | ✅ | ✅ | ❌ | ✅ | ❌ | ❌ |
| **architect.default** | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| **debugger.default** | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| **auditor.default** | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| **agent-adapter.default** | ✅ | ✅ | ✅ | ❌ | ✅ (5) | ❌ Must delegate |

### Capability Details by Agent

#### planner.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge.", "agent."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*"]
  - type: "AgentSpawn"
    max_children: 10
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]
```
- **Role**: Front-door lead agent, interprets goals, delegates to specialists
- **Cannot**: Execute code, install agents directly, make HTTP requests

#### specialized_builder.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge.", "agent."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*", "agents/*"]
  - type: "AgentSpawn"
    max_children: 5
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*", "agents/*"]
```
- **Role**: **EXCLUSIVE** agent installer - only agent that can call `agent.install`
- **Cannot**: Execute code, make HTTP requests

#### coder.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge.", "sandbox."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*", "scripts/*"]
  - type: "CodeExecution"
    patterns: ["python3 scripts/*", "node *", "bash *"]
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*", "scripts/*"]
```
- **Role**: Code production, sandboxed execution
- **Cannot**: Install agents, make HTTP requests directly

#### researcher.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge.", "web.", "mcp_"]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*"]
  - type: "NetworkAccess"
    hosts: ["*"]
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]
```
- **Role**: Evidence gathering, web research
- **Cannot**: Execute code, install agents

#### architect.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*"]
  - type: "CodeExecution"
    patterns: ["python3 scripts/*"]
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]
```
- **Role**: System design, prototyping
- **Cannot**: Install agents, make HTTP requests

#### debugger.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge.", "sandbox."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*"]
  - type: "CodeExecution"
    patterns: ["python3 scripts/*", "node *", "bash *"]
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]
```
- **Role**: Root cause analysis, targeted fixes
- **Cannot**: Install agents, make HTTP requests

#### auditor.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*"]
  - type: "CodeExecution"
    patterns: ["python3 scripts/*"]
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]
```
- **Role**: Correctness review, reproducibility verification
- **Cannot**: Install agents, make HTTP requests

#### agent-adapter.default
```yaml
capabilities:
  - type: "SandboxFunctions"
    allowed: ["knowledge.", "sandbox."]
  - type: "ReadAccess"
    scopes: ["self.*", "skills/*"]
  - type: "CodeExecution"
    patterns: ["python3 scripts/*"]
  - type: "AgentSpawn"
    max_children: 5
  - type: "WriteAccess"
    scopes: ["self.*", "skills/*"]
```
- **Role**: Generate wrapper agents for I/O bridging
- **Note**: Must delegate agent installation to specialized_builder

## Delegation Model

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              PLANNER (Lead)                                 │
│  • Interprets goals                                                         │
│  • Delegates to specialists                                                 │
│  • Synthesizes responses                                                    │
│  • CANNOT install agents or execute code                                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                ┌───────────────────┼───────────────────┐
                ▼                   ▼                   ▼
┌───────────────────────┐ ┌───────────────────────┐ ┌───────────────────────┐
│  SPECIALIZED_BUILDER  │ │      CODER            │ │     RESEARCHER        │
│  • Installs agents    │ │  • Writes code        │ │  • Gathers evidence   │
│  • ONLY agent that    │ │  • Executes in sandbox│ │  • Has network access │
│    can agent.install  │ │  • Tests before return│ │  • Cites sources      │
└───────────────────────┘ └───────────────────────┘ └───────────────────────┘
                                    │
                ┌───────────────────┼───────────────────┐
                ▼                   ▼                   ▼
┌───────────────────────┐ ┌───────────────────────┐ ┌───────────────────────┐
│     ARCHITECT         │ │      DEBUGGER         │ │      AUDITOR          │
│  • Designs systems    │ │  • Root cause analysis│ │  • Review & audit     │
│  • Creates prototypes │ │  • Targeted fixes     │ │  • Verify results     │
│  • Documents decisions│ │  • Reproduces issues  │ │  • Risk assessment    │
└───────────────────────┘ └───────────────────────┘ └───────────────────────┘
```

## Capability Format Examples

### NetworkAccess (requires `hosts` field)
```json
{"type": "NetworkAccess", "hosts": ["api.example.com"]}
{"type": "NetworkAccess", "hosts": ["*"]}  // Allow all hosts
```

### SandboxFunctions (for MCP tools)
```json
{"type": "SandboxFunctions", "allowed": ["web.", "content.read"]}
```

### CodeExecution (script patterns)
```json
{"type": "CodeExecution", "patterns": ["python3 scripts/*", "node *"]}
```

### ReadAccess/WriteAccess (scopes)
```json
{"type": "ReadAccess", "scopes": ["self.*", "shared.*"]}
{"type": "WriteAccess", "scopes": ["self.*", "agents/*"]}
```

### AgentSpawn (max children)
```json
{"type": "AgentSpawn", "max_children": 5}
```

## Common Mistakes

1. **Using `SandboxFunctions: ["content."]` for content access**
   - Wrong: Use `ReadAccess`/`WriteAccess` instead

2. **Adding extra fields to capabilities**
   - Wrong: `{"type": "NetworkAccess", "description": "...", "hosts": [...]}`
   - Correct: `{"type": "NetworkAccess", "hosts": [...]}`

3. **Planner calling agent.install directly**
   - Wrong: Planner calls `agent.install`
   - Correct: Planner delegates to `specialized_builder.default` via `agent.spawn`

4. **Missing promotion_gate for specialized_builder**
   - `agent.install` from evolution roles requires `{"evaluator_pass": true, "auditor_pass": true}`

## See Also

- [Capability Development Guide](file:///home/mandubian/workspaces/mandubian/ccos/autonoetic/docs/capability-development.md)
- [Agent SKILL.md Files](/tmp/autonoetic-demo/agents/*/SKILL.md)
- [CCOS Architecture](/home/mandubian/workspaces/mandubian/ccos/docs/ccos/specs/000-ccos-architecture.md)
