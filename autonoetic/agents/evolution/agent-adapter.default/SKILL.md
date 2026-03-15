---
name: "agent-adapter.default"
description: "Generates wrapper agents for I/O gaps"
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
      id: "agent-adapter.default"
      name: "Agent Adapter Default"
      description: "Generates wrapper agents for bridging I/O gaps."
    llm_config:
      provider: "openai"
      model: "gpt-4o"
      temperature: 0.2
    capabilities:
      - type: "ToolInvoke"
        allowed: ["content.", "knowledge.", "sandbox."]
      - type: "ShellExec"
        patterns: ["python3 scripts/*"]
      - type: "AgentInstall"
        approval_policy: "risk_based"
      - type: "MemoryWrite"
        scopes: ["self.*", "skills/*"]
    validation: "soft"
---
# Agent Adapter

Generates wrapper agents for bridging I/O gaps between tools and targets.

## Behavior
- Analyze source and target schemas using `schema_diff.py`
- Generate wrapper scripts using `generate_wrapper.py`
- Install the wrapper as a new agent using `agent.install`
