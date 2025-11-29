# [FUTURE] JIT Polyglot Capability Generation via MicroVMs

## Context
Currently, CCOS missing capability resolution relies on:
1. Discovery (Marketplace, MCP, etc.)
2. Pure RTFS generation (Limited to CCOS/RTFS primitives)

This proposal introduces a third, powerful fallback: **Just-in-Time (JIT) Polyglot Generation**.

## Concept
When a capability is missing and cannot be solved by existing tools or pure RTFS logic, the system can:
1. **Generate** a standalone script in a general-purpose language (e.g., Python, TypeScript, Rust) using an LLM.
2. **Execute** this script in a secure, ephemeral sandbox (MicroVM/Docker).
3. **Bridge** the inputs and outputs back to the CCOS runtime.

This allows the agent to "write its own tools" on the fly for complex tasks like data analysis, image processing, or interacting with libraries not natively available in RTFS.

## Architecture

### 1. Strategy Integration
The `UserInteractionStrategy` (and eventually an automated strategy) gets a new option:
- "Generate external implementation (Python/MicroVM)"

### 2. Execution Flow
1. **Intent Analysis**: The system captures the `capability_id`, input arguments, and context.
2. **LLM Prompting**: A specialized prompt asks the LLM to write a self-contained Python script.
   - Input: JSON via stdin or file.
   - Output: JSON via stdout.
   - Constraints: No network (default), specific libraries allowed.
3. **Sandboxing (The "Executor")**:
   - Spawns a lightweight VM (e.g., Firecracker) or Container.
   - Injects the generated script and input data.
   - Runs the script with timeouts and resource limits.
4. **Result Handling**:
   - Parses the JSON output from the script.
   - Validates against the expected schema.
   - Returns the result to the CCOS plan execution.

## Proposed Components

- **`PolyglotGenerationStrategy`**: A new strategy in `missing_capability_strategies.rs`.
- **`SandboxExecutor`**: A trait for abstracting the execution environment (Docker, Firecracker, WASM).
- **`ScriptSynthesizer`**: Helper to prompt the LLM for specific languages.

## Security Considerations
- **Isolation**: Code must run in a strictly isolated environment (no access to host FS or internal network).
- **Review**: In interactive mode, the user should be able to review/edit the generated Python code before execution.
- **Resource Limits**: CPU/RAM caps to prevent DoS.

## Example User Flow
```text
🤖 MISSING CAPABILITY: analyze_csv_trends
   I couldn't find this capability automatically.
   How would you like to resolve this?
   1. Generate Pure RTFS implementation (mock)
   2. Generate Python implementation (Sandbox)  <-- NEW
   3. Skip resolution (fail)
   > 2

   ... Generating Python script ...
   ... Provisioning Sandbox ...
   ... Executing ...
   ✅ Result: { "trend": "upward", "confidence": 0.95 }
```

