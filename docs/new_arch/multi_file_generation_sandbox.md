# Sandboxed Multi-File Code Generation

## Objective
The current `ccos.code.refined_execute` and `CodingAgent` pipeline is optimized for single-file Python script generation. We want to enable the AI to generate larger, multi-file projects that run effectively inside the Bubblewrap sandbox.

## Current Limitations
1. `CodingAgent` parses a single `code: String` out of the JSON response returned by the LLM.
2. `ccos.code.refined_execute` stores this single string and passes it directly to `ccos.execute.python`.
3. The sandbox environment only mounts this single blob of code.

## High-Level Architecture
We can achieve this by expanding the JSON protocol between the `CodingAgent` and the LLM, and utilizing the existing `input_files` functionality of `BubblewrapSandbox`. We will rely heavily on storing generated files on the host filesystem so they can be securely mounted into the sandbox, rather than holding massive codebase strings in memory or cramming them into SQLite.

### 1. Update `CodingResponse` (`ccos/src/sandbox/coding_agent.rs`)
Modify the expected JSON format from the LLM.

**Old Format:**
```json
{
  "code": "...",
  "language": "python",
  "dependencies": []
}
```

**New Format:**
```json
{
  "files": [
    {
      "path": "main.py",
      "content": "..."
    },
    {
      "path": "utils.py",
      "content": "..."
    }
  ],
  "entrypoint": "main.py",
  "language": "python",
  "dependencies": []
}
```

Add backwards compatibility so that if the LLM still returns `"code"`, we wrap it into a fake `[{"path": "main.py", "content": code}]`.

### 2. Expand the Refined Execute Phase (`ccos/src/chat/mod.rs`)
Inside the `ccos.code.refined_execute` workflow:
1. When a successful LLM `CodingResponse` is received, iterate over all `files`.
2. Save these files permanently to the host filesystem, mapped by `run_id` (e.g. `~/.ccos/runs/<run_id>/code/`). 
   - *Note: This cleanly ties into our Causal Chain persistence plan, which will now reference these file paths instead of storing megabytes of raw text.*
3. Construct the `input_files` dictionary mapping the expected path inside the sandbox (`/workspace/input/main.py`) to their persistent location on the host (`~/.ccos/runs/...`).
4. In the `code` argument passed to `ccos.execute.python`, we can simply patch it to accept an `entrypoint` directly instead of a raw `code` string.

### 3. Update the Execute Sandbox (`ccos/src/sandbox/bubblewrap.rs` / `execute_python.rs`)
The sandbox `BubblewrapSandbox` **already** supports mounting a `Vec<InputFile>`.
```rust
pub struct InputFile {
    pub name: String,         // E.g. "utils.py"
    pub host_path: PathBuf,   // E.g. "/var/lib/ccos/runs/.../utils.py"
}
```
These are mounted as read-only binds into `/workspace/input/`. Since the Python executable is run with `PYTHONPATH=/workspace/input`, cross-file imports (e.g., `import utils` inside `main.py`) will work seamlessly out of the box.

The main change required here is updating `ccos.execute.python` to accept an `entrypoint` string rather than raw `code`, and modifying how it executes the bwrap child process if `entrypoint` is provided (e.g., running `python /workspace/input/main.py` instead of passing `-c "<code>"`).

## Flow Summary
1. User requests a complex application.
2. `CodingAgent` prompts LLM for a multi-file JSON.
3. LLM returns `main.py` and `helper.py`.
4. `refined_execute` writes them strictly to `~/.ccos/runs/<run_id>/code/main.py` and `helper.py`.
5. `refined_execute` records a Causal Chain event *pointing* to these files (avoiding SQLite truncation/bloat).
6. `refined_execute` calls `ccos.execute.python` with `input_files` mapping to the host paths and `entrypoint: "main.py"`.
7. Sandbox securely bind-mounts the host files to `/workspace/input/main.py` and `/workspace/input/helper.py`.
8. Sandbox runs `python /workspace/input/main.py`.
