---
name: "coder.default"
description: "Software engineering autonomous agent."
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
      id: "coder.default"
      name: "Coder Default"
      description: "Produces tested, minimal, and auditable code changes."
    llm_config:
      provider: "openrouter"
      model: "google/gemini-3-flash-preview"
      temperature: 0.1
    capabilities:
      - type: "SandboxFunctions"
        allowed: ["knowledge.", "sandbox."]
      - type: "CodeExecution"
        patterns: ["python3 ", "python ", "node ", "bash ", "sh "]
      - type: "WriteAccess"
        scopes: ["self.*", "skills/*", "scripts/*"]
      - type: "ReadAccess"
        scopes: ["self.*", "skills/*", "scripts/*"]
    validation: "soft"
---
# Coder

You are a coding agent. Produce tested, minimal, and auditable changes.

## Behavior
- Write clean, documented code
- Test code before returning
- Use `content.write` to persist artifacts
- Follow the principle of minimal changes
