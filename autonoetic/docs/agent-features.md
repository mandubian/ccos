# Agent Features Reference

This document describes all agent features available in the Autonoetic gateway runtime.

## Table of Contents

- [Agent Manifest](#agent-manifest)
- [Execution Modes](#execution-modes)
- [Capabilities](#capabilities)
- [I/O Schemas](#io-schemas)
- [Middleware Hooks](#middleware-hooks)
- [Background Scheduling](#background-scheduling)
- [Disclosure Policy](#disclosure-policy)
- [Memory System](#memory-system)

---

## Agent Manifest

Every agent is defined by a `SKILL.md` file containing YAML frontmatter and a Markdown body. The frontmatter defines the agent's identity, capabilities, and behavior.

### Minimal Example

```yaml
---
name: "my-agent"
description: "A simple reasoning agent"
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
      id: "my-agent"
      name: "My Agent"
      description: "A simple reasoning agent"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.0
    capabilities:
      - type: "MemoryRead"
        scopes: ["*"]
---
# My Agent Instructions

You are a helpful assistant.
```

### Manifest Fields

| Field | Required | Description |
|-------|----------|-------------|
| `version` | Yes | Manifest version (use `"1.0"`) |
| `runtime` | Yes | Runtime declaration block |
| `agent` | Yes | Agent identity (id, name, description) |
| `capabilities` | No | List of granted capabilities |
| `llm_config` | No | LLM provider configuration |
| `limits` | No | Resource limits |
| `execution_mode` | No | `reasoning` (default) or `script` |
| `script_entry` | No | Script path for script mode |
| `io` | No | I/O schema contract |
| `middleware` | No | Pre/post processing hooks |
| `background` | No | Background scheduling policy |
| `disclosure` | No | Output disclosure policy |

---

## Execution Modes

Autonoetic supports two execution modes for agents: **reasoning** (default) and **script**.

### Reasoning Mode (default)

Agents use the full LLM lifecycle: the gateway builds a system prompt, sends it to the configured LLM, processes tool calls, and manages conversation history. This is the default mode and requires `llm_config`.

```yaml
execution_mode: reasoning  # optional, this is the default
llm_config:
  provider: "openai"
  model: "gpt-4o"
```

### Script Mode

Script agents execute directly in the sandbox without LLM involvement. The gateway runs the specified script with the ingress payload as input and returns the script's stdout as the reply.

**Key properties:**
- Fast execution (no LLM latency)
- Deterministic (same input → same output)
- Cheap (no token usage)
- Limited to simple data retrieval and transforms

**Requirements:**
- Must set `execution_mode: script`
- Must set `script_entry` pointing to the script
- `llm_config` is not required

```yaml
execution_mode: script
script_entry: scripts/weather.py
```

**Script input:** The script receives the ingress payload via the `SCRIPT_INPUT` environment variable.

**Script output:** stdout is captured and returned as the agent reply.

**Example script:**
```python
import os
import json

input_data = os.environ.get("SCRIPT_INPUT", "")
# Process input and produce output
print(f"Result: {input_data}")
```

**When to use script mode:**
- API data retrieval (weather, stock prices, status checks)
- Simple data transforms
- Deterministic operations without reasoning needed

**When to use reasoning mode:**
- Tasks requiring judgment or interpretation
- Multi-step reasoning
- Ambiguous user intents
- Tasks requiring tool use chains

### Fast Path

When `execution_mode: script`, the gateway bypasses the full LLM lifecycle:
1. Loads the agent manifest
2. Validates the script entry exists
3. Executes the script directly in sandbox
4. Returns stdout as reply
5. Logs causal events (`script.started`, `script.completed`, `script.failed`)

No LLM request is made, no history is managed, and no tool calls are processed.

---

## Capabilities

Capabilities control what agents can do. The gateway enforces these at runtime.

### Available Capabilities

| Capability | Description |
|------------|-------------|
| `MemoryRead` | Read from memory scopes |
| `MemoryWrite` | Write to memory scopes |
| `ShellExec` | Execute shell commands |
| `ToolInvoke` | Invoke MCP tools |
| `AgentSpawn` | Spawn child agents |
| `AgentMessage` | Message other agents |
| `BackgroundReevaluation` | Schedule background tasks |
| `NetConnect` | Make network requests |
| `SandboxExec` | Execute in sandbox |

### Capability Examples

```yaml
capabilities:
  - type: "MemoryRead"
    scopes: ["*"]
  - type: "MemoryWrite"
    scopes: ["self.*", "shared.*"]
  - type: "AgentSpawn"
    max_children: 5
  - type: "ShellExec"
    patterns: ["cargo test *", "python *"]
```

---

## I/O Schemas

Agents can declare input and output schemas for mechanical validation and adapter-based composition.

### Example

```yaml
io:
  accepts:
    type: object
    required:
      - query
    properties:
      query:
        type: string
  returns:
    type: object
    properties:
      findings:
        type: array
      summary:
        type: string
```

**Purpose:**
- Validates incoming payloads against the accepts schema
- Enables adapter agents to generate wrappers for schema mismatch
- Exposed in `agent.discover` for planner routing decisions

---

## Middleware Hooks

Agents can declare pre-processing and post-processing hooks that run in the sandbox.

### Example

```yaml
middleware:
  pre_process: "python3 scripts/normalize.py"
  post_process: "python3 scripts/format.py"
```

**Pre-process hook:**
- Runs on user input before passing to LLM
- Can set `skip_llm: true` in JSON output to bypass LLM entirely
- Useful for input normalization or short-circuit responses

**Post-process hook:**
- Runs on LLM output before returning to user
- Can transform or filter the response

---

## Background Scheduling

Agents can run in the background on a schedule.

### Example

```yaml
capabilities:
  - type: "BackgroundReevaluation"
    min_interval_secs: 30
    allow_reasoning: false
background:
  enabled: true
  interval_secs: 60
  mode: deterministic
  wake_predicates:
    timer: true
    new_messages: true
```

### Background Modes

| Mode | Description |
|------|-------------|
| `deterministic` | Execute pending scheduled actions directly (no LLM) |
| `reasoning` | Full LLM-driven execution for each wake |

### Wake Predicates

| Predicate | Description |
|-----------|-------------|
| `timer` | Wake on interval timer |
| `new_messages` | Wake on new ingress messages |
| `task_completions` | Wake when delegated tasks complete |
| `queued_work` | Wake when work is queued |
| `stale_goals` | Wake when goals become stale |
| `retryable_failures` | Wake for retry after failures |
| `approval_resolved` | Wake when approval is granted |

---

## Disclosure Policy

Controls what agent replies can contain from sensitive sources.

### Example

```yaml
disclosure:
  default_class: "public"
  path_overrides:
    - path: "secrets/*"
      class: "secret"
      action: "withhold"
```

### Disclosure Classes

| Class | Description |
|-------|-------------|
| `public` | Can be returned verbatim |
| `internal` | Can be returned within gateway |
| `confidential` | Summarized only |
| `secret` | Never returned verbatim |

---

## Memory System

Autonoetic provides two memory tiers:

### Tier 1 (Working Memory)

- Local state files in agent directory (`state/`)
- For deterministic checkpoint and operational continuity
- Restart-safe, per-agent only

### Tier 2 (Durable Memory)

- Gateway-managed SQLite storage
- Cross-session and cross-agent recall
- ACL-controlled sharing
- Full provenance tracking

### Memory Tools

| Tool | Description |
|------|-------------|
| `memory.remember(id, scope, content)` | Store a durable fact |
| `memory.recall(id)` | Retrieve a durable fact |
| `memory.search(query)` | Search memory |
| `memory.working.save(key, content)` | Save to working memory |
| `memory.working.load(key)` | Load from working memory |

---

## Example: Script Agent (Weather)

```yaml
---
name: "weather.default"
description: "Fetches weather data for a city"
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
      id: "weather.default"
      name: "Weather Agent"
      description: "Fetches weather data for a city"
    execution_mode: script
    script_entry: scripts/fetch_weather.py
    capabilities: []
---
# Weather Agent
This agent runs as a script and returns weather data.
```

## Example: Reasoning Agent (Researcher)

```yaml
---
name: "researcher.default"
description: "Researches topics and returns findings"
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
      id: "researcher.default"
      name: "Researcher Default"
      description: "Researches topics and returns findings"
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.2
    capabilities:
      - type: "MemoryRead"
        scopes: ["*"]
      - type: "MemoryWrite"
        scopes: ["self.*"]
    io:
      accepts:
        type: object
        required:
          - query
      returns:
        type: object
        properties:
          findings:
            type: array
          summary:
            type: string
---
# Researcher Agent
You research topics and return structured findings.
```
