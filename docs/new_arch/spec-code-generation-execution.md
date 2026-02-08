# CCOS Code Generation and Execution Architecture

**Status**: Phase 1-3 Complete, Phase 4-6 Pending  
**Related**: [Polyglot Sandboxed Capabilities](./spec-polyglot-sandboxed-capabilities.md), [Skill Interpreter](./spec-skill-interpreter.md)

## 1. Executive Summary

This specification extends the CCOS polyglot sandbox architecture to support **LLM-driven code generation** as a first-class capability. The key innovations are:

1. **Code Generation as a Capability**: LLM generates Python/JS/RTFS code to fulfill user goals
2. **Dependency Resolution**: Sandboxes can install dependencies from approved package registries
3. **Specialized Coding Agents**: Delegate code generation to purpose-built coding LLMs
4. **Iterative Refinement**: Code can be tested, debugged, and refined in a feedback loop

## 2. Problem Statement

### 2.1 Current Limitations

The CCOS agent currently has limited execution capabilities:
- HTTP calls (via `ccos.network.http_request`)
- Chat transforms (via `ccos.chat.*` capabilities)
- RTFS expression evaluation

For complex tasks requiring computation, data transformation, or integration logic, the agent cannot:
- Generate and execute arbitrary code
- Install dependencies (pandas, requests, etc.)
- Iterate on failed attempts

### 2.2 Goal

Enable the agent to:
1. Understand user intent from natural language
2. Generate executable code to fulfill the goal
3. Execute in a secure, isolated sandbox
4. Handle dependencies transparently
5. Iterate on failures with debugging context

## 3. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           User Message                                       │
│  "Analyze this CSV and create a chart showing sales by region"              │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Agent (Deputy)                                        │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  1. Intent Extraction                                                  │  │
│  │     → Goal: Analyze CSV, create chart                                  │  │
│  │     → Required: Data processing, visualization                         │  │
│  │     → Language: Python (pandas, matplotlib)                            │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
│                                    │                                         │
│                                    ▼                                         │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  2. Capability Selection                                               │  │
│  │     → ccos.execute.python (code execution sandbox)                     │  │
│  │     OR                                                                 │  │
│  │     → ccos.delegate.coding_agent (specialized LLM)                     │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
│                                    │                                         │
│                                    ▼                                         │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  3. Code Generation                                                    │  │
│  │     → LLM generates Python code                                        │  │
│  │     → Dependencies detected: pandas, matplotlib                        │  │
│  │     → Input/output schema inferred                                     │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
│                                    │                                         │
│                                    ▼                                         │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  4. Sandbox Execution                                                  │  │
│  │     → Sandbox provisioned with dependencies                            │  │
│  │     → Code executed with timeout/resource limits                       │  │
│  │     → Output captured (stdout, files, errors)                          │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
│                                    │                                         │
│                            ┌───────┴───────┐                                │
│                            ▼               ▼                                │
│                      [SUCCESS]        [FAILURE]                             │
│                            │               │                                │
│                            ▼               ▼                                │
│                     Return result   5. Refinement Loop                      │
│                                        → Error context to LLM              │
│                                        → Generate fixed code               │
│                                        → Re-execute (max 3 attempts)       │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 4. Code Generation Strategies

### 4.1 Direct Generation (Default)

The primary agent's LLM generates code directly:

```
User: "Calculate the factorial of 100"

Agent LLM thinks:
  - Goal: Compute factorial
  - Approach: Python with math library
  - Generate code:
    ```python
    import math
    result = math.factorial(100)
    print(f"100! = {result}")
    ```
```

**Pros**: Low latency, single LLM call  
**Cons**: Quality depends on primary LLM's coding ability

### 4.2 Specialized Coding Agent (Delegation)

For complex tasks, delegate to a specialized coding LLM:

```
┌─────────────────────────────────────────────────────────────────┐
│  Primary Agent (Orchestrator)                                    │
│  • Understands user intent                                       │
│  • Decides what code is needed                                   │
│  • Formats requirements for coding agent                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │  ccos.delegate.coding_agent
                              │  {
                              │    "task": "Create data viz",
                              │    "inputs": ["sales.csv"],
                              │    "outputs": ["chart.png"],
                              │    "language": "python",
                              │    "constraints": {
                              │      "dependencies_allowed": true,
                              │      "max_lines": 100
                              │    }
                              │  }
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Coding Agent (Specialized LLM)                                  │
│  • Fine-tuned for code generation                                │
│  • Examples: Claude Sonnet, GPT-4o, DeepSeek Coder, Codellama   │
│  • Produces: code, requirements.txt, test cases                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │  Returns:
                              │  {
                              │    "code": "...",
                              │    "dependencies": ["pandas", "matplotlib"],
                              │    "tests": "...",
                              │    "explanation": "..."
                              │  }
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Sandbox Execution                                               │
│  • Install dependencies                                          │
│  • Run code                                                      │
│  • Capture outputs                                               │
└─────────────────────────────────────────────────────────────────┘
```

**Pros**: 
- Higher quality code from specialized models
- Separation of concerns (orchestration vs. coding)
- Different models for different languages

**Cons**:
- Higher latency (two LLM calls)
- Additional cost

### 4.3 RTFS Generation (Safe Path)

For pure computation, generate RTFS instead of general-purpose code:

```clojure
;; Generated RTFS for "sum values in list"
(reduce + 0 (get-in inputs [:values]))
```

**Pros**:
- No sandbox needed
- Deterministic, auditable
- Fast execution

**Cons**:
- Limited to RTFS primitives
- Not suitable for I/O, visualization

## 5. Dependency Management

### 5.1 Dependency Sources

| Source | Trust Level | Use Case |
|--------|-------------|----------|
| Pre-baked images | Highest | Common stacks (data science, web) |
| Allowlisted packages | High | Approved on first use |
| Any PyPI/npm package | Low | Requires elevated approval |
| Git repositories | Low | Requires per-use approval |

### 5.2 Pre-baked Sandbox Images

Most requests can be served by pre-built images with common packages:

```yaml
# sandbox-images.yaml
images:
  - name: python-data-science
    base: python:3.12-slim
    packages:
      - pandas>=2.0
      - numpy>=1.24
      - matplotlib>=3.7
      - seaborn>=0.12
      - scipy>=1.10
      - scikit-learn>=1.3
    warm_pool_size: 2
    
  - name: python-web
    base: python:3.12-slim
    packages:
      - requests>=2.28
      - httpx>=0.24
      - beautifulsoup4>=4.12
      - lxml>=4.9
    warm_pool_size: 1
    
  - name: nodejs-tools
    base: node:20-slim
    packages:
      - axios
      - cheerio
      - lodash
    warm_pool_size: 1
```

**Benefits**:
- Fast startup (no install step)
- Security reviewed
- Reproducible environments

### 5.3 Dynamic Dependency Installation

For packages not in pre-baked images:

```
┌─────────────────────────────────────────────────────────────────┐
│  1. Code Generation                                              │
│     → LLM outputs: code + dependencies                           │
│     → dependencies: ["pandas", "openpyxl"]                       │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  2. Dependency Resolution                                        │
│     → Check if all deps in pre-baked image                       │
│     → pandas: ✓ (in python-data-science)                         │
│     → openpyxl: ✗ (not pre-baked)                                │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  3. Package Allowlist Check                                      │
│     → Is openpyxl in approved packages?                          │
│     → If YES: auto-install                                       │
│     → If NO: request user approval                               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  4. Sandbox Provisioning                                         │
│     → Start python-data-science image                            │
│     → pip install openpyxl (within sandbox)                      │
│     → Network isolated after install                             │
└─────────────────────────────────────────────────────────────────┘
```

### 5.4 Package Allowlist

```yaml
# package-allowlist.yaml
python:
  auto_approved:
    - pandas
    - numpy
    - matplotlib
    - requests
    - beautifulsoup4
    - pillow
    - openpyxl
    - xlrd
    - pyyaml
    - jinja2
    
  requires_approval:
    - torch        # Large, GPU
    - tensorflow   # Large, GPU
    - langchain    # External API calls
    - openai       # External API calls
    
  blocked:
    - subprocess32  # Shell escape risk
    - pyautogui     # Desktop control
    - keyboard      # Input capture
    
nodejs:
  auto_approved:
    - axios
    - lodash
    - moment
    - cheerio
```

### 5.5 Caching Strategy

```
┌───────────────────────────────────────────────────┐
│  Layer 1: Pre-baked Images (warmest)              │
│  • Maintained as Docker/OCI images                │
│  • Updated weekly with security patches           │
│  • Kept warm in pool                              │
└───────────────────────────────────────────────────┘
                      │
                      ▼
┌───────────────────────────────────────────────────┐
│  Layer 2: Package Cache (warm)                    │
│  • Pip/npm cache shared across sandboxes          │
│  • Network-isolated after initial download        │
│  • TTL: 7 days                                    │
└───────────────────────────────────────────────────┘
                      │
                      ▼
┌───────────────────────────────────────────────────┐
│  Layer 3: Fresh Install (cold)                    │
│  • For packages not in cache                      │
│  • Requires network during install phase          │
│  • Then network isolated for execution            │
└───────────────────────────────────────────────────┘
```

## 6. Execution Capabilities

### 6.1 Core Capabilities

```clojure
;; Execute Python code in sandbox
(capability "ccos.execute.python"
  :description "Execute Python code in isolated sandbox"
  :input-schema [:map
    [:code :string]
    [:inputs {:optional true} [:map]]
    [:dependencies {:optional true} [:vector :string]]
    [:image {:optional true} :string]             ; default: python-data-science
    [:timeout_ms {:optional true} :int]]          ; default: 30000
  
  :output-schema [:map
    [:stdout :string]
    [:stderr :string]
    [:exit_code :int]
    [:files {:optional true} [:map :string :bytes]]
    [:error {:optional true} :string]]
  
  :runtime {:type :container}
  :effects [:compute]
  :resources {:memory-mb 512 :timeout-ms 30000})

;; Execute JavaScript/Node.js code in sandbox
(capability "ccos.execute.javascript"
  :description "Execute Node.js code in isolated sandbox"
  :input-schema [:map
    [:code :string]
    [:inputs {:optional true} [:map]]
    [:dependencies {:optional true} [:vector :string]]]
  
  :output-schema [:map
    [:stdout :string]
    [:stderr :string]
    [:exit_code :int]]
  
  :runtime {:type :container :image "nodejs-tools"}
  :effects [:compute])

;; Interpret RTFS expression
(capability "ccos.execute.rtfs"
  :description "Evaluate an RTFS expression"
  :input-schema [:map
    [:expression :string]
    [:bindings {:optional true} [:map]]]
  
  :output-schema :any
  
  :runtime {:type :rtfs}
  :effects [])

;; Delegate to specialized coding agent
(capability "ccos.delegate.coding_agent"
  :description "Delegate code generation to specialized LLM"
  :input-schema [:map
    [:task :string]
    [:language {:optional true} :string]
    [:inputs {:optional true} [:vector :string]]
    [:outputs {:optional true} [:vector :string]]
    [:constraints {:optional true} [:map]]]
  
  :output-schema [:map
    [:code :string]
    [:language :string]
    [:dependencies [:vector :string]]
    [:explanation :string]
    [:tests {:optional true} :string]]
  
  :effects [:llm])
```

### 6.2 Chat Gateway Integration

Register these as chat capabilities with data classification:

```rust
// In chat/mod.rs

register_native_chat_capability(
    &*marketplace,
    "ccos.execute.python",
    "Execute Python Code",
    "Run Python code in a sandboxed environment",
    Arc::new(move |inputs: &Value| {
        let inputs = inputs.clone();
        Box::pin(async move {
            // 1. Extract code and dependencies
            // 2. Select/provision sandbox
            // 3. Execute with timeout
            // 4. Capture outputs
            // 5. Return result with data labels
        })
    }),
    "high",                              // Risk tier
    vec!["compute".to_string()],         // Required approvals
    EffectType::State,                   // Has side effects
).await?;
```

## 7. Specialized Coding Agents

### 7.1 Agent Configuration

```toml
# config/agent_config.toml

[coding_agents]
# Default coding agent for general tasks
default = "openrouter:deepseek-coder"

# Language-specific overrides
[coding_agents.python]
model = "openrouter:deepseek-coder-v2"
max_tokens = 4096

[coding_agents.javascript]
model = "openrouter:codellama-70b"
max_tokens = 4096

[coding_agents.rust]
model = "openrouter:claude-3-sonnet"
max_tokens = 8192

# Coding agent profiles
[[coding_agents.profiles]]
name = "deepseek-coder"
provider = "openrouter"
model = "deepseek/deepseek-coder-v2-instruct"
api_key_env = "OPENROUTER_API_KEY"
system_prompt = """
You are a code generation assistant. Generate clean, well-documented code.
Always include:
1. Import statements
2. Type hints (Python) or TypeScript types (JS)
3. Error handling
4. Brief comments explaining logic
"""

[[coding_agents.profiles]]
name = "claude-coder"
provider = "anthropic"
model = "claude-3-sonnet-20240229"
api_key_env = "ANTHROPIC_API_KEY"
system_prompt = """
You are an expert programmer. Write production-quality code.
Focus on correctness, efficiency, and maintainability.
Always validate inputs and handle edge cases.
"""
```

### 7.2 Coding Agent Protocol

```
┌─────────────────────────────────────────────────────────────────┐
│  Primary Agent → Coding Agent Request                            │
│                                                                  │
│  {                                                               │
│    "task": "Create a bar chart from CSV data",                   │
│    "context": {                                                  │
│      "input_files": [                                            │
│        {"name": "sales.csv", "schema": "region,amount,date"}     │
│      ],                                                          │
│      "output_requirements": ["PNG image", "stdout summary"],     │
│      "constraints": {                                            │
│        "language": "python",                                     │
│        "max_dependencies": 5,                                    │
│        "timeout_ms": 30000                                       │
│      }                                                           │
│    },                                                            │
│    "examples": [                                                 │
│      {"input": "...", "expected_output": "..."}                  │
│    ]                                                             │
│  }                                                               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Coding Agent Response                                           │
│                                                                  │
│  {                                                               │
│    "code": "import pandas as pd\nimport matplotlib...",          │
│    "language": "python",                                         │
│    "dependencies": ["pandas", "matplotlib"],                     │
│    "entry_point": "main()",                                      │
│    "input_parsing": "CSV file read from stdin or first arg",     │
│    "output_format": "PNG written to /output/chart.png",          │
│    "explanation": "Uses pandas for data loading, groups by...",  │
│    "tests": "def test_chart_creation(): ...",                    │
│    "estimated_runtime_ms": 2000                                  │
│  }                                                               │
└─────────────────────────────────────────────────────────────────┘
```

### 7.3 Delegation Decision

When to use specialized coding agent vs. direct generation:

| Criteria | Direct Generation | Delegate to Coding Agent |
|----------|-------------------|--------------------------|
| Complexity | Simple (< 20 lines) | Complex (> 20 lines) |
| Dependencies | 0-2 standard | 3+ or unusual |
| Language | Python, JS | Rust, Go, specialized |
| Error likelihood | Low | High (needs iteration) |
| Quality requirement | Good enough | Production quality |

## 8. Iterative Refinement Loop

### 8.1 Error Handling Flow

```
┌─────────────────────────────────────────────────────────────────┐
│  Initial Code Execution                                          │
│  → Run code in sandbox                                           │
│  → Capture stdout, stderr, exit code                             │
└─────────────────────────────────────────────────────────────────┘
                              │
                      ┌───────┴───────┐
                      ▼               ▼
               [exit_code = 0]  [exit_code ≠ 0]
                      │               │
                      ▼               ▼
               Return success    ┌─────────────────────────────────┐
                                 │  Error Analysis                  │
                                 │  → Parse error message           │
                                 │  → Identify error type:          │
                                 │    • Syntax error                │
                                 │    • Import error                │
                                 │    • Runtime exception           │
                                 │    • Timeout                     │
                                 └─────────────────────────────────┘
                                              │
                                              ▼
                                 ┌─────────────────────────────────┐
                                 │  Refinement Request to LLM       │
                                 │                                  │
                                 │  "The code failed with:          │
                                 │   ModuleNotFoundError: pandas    │
                                 │                                  │
                                 │   Please fix the code. You may   │
                                 │   need to add missing imports    │
                                 │   or dependencies."              │
                                 └─────────────────────────────────┘
                                              │
                                              ▼
                                 ┌─────────────────────────────────┐
                                 │  LLM Generates Fixed Code        │
                                 │  → Adds: dependencies = [pandas] │
                                 │  → Retries execution             │
                                 └─────────────────────────────────┘
                                              │
                                              ▼
                                      [Attempt < MAX?]
                                     /              \
                                   YES              NO
                                    │                │
                                    ▼                ▼
                              Retry execution   Return failure
                                                with all attempts
```

### 8.2 Refinement Prompt Template

```
Previous code failed with the following error:

```
{error_message}
```

The code was:
```{language}
{original_code}
```

Please analyze the error and provide a corrected version.

If the error is:
- Import error: Add the missing dependency to the requirements
- Syntax error: Fix the syntax
- Runtime error: Fix the logic
- Timeout: Optimize the algorithm

Respond with the corrected code only, no explanations.
```

### 8.3 Retry Limits

```yaml
# retry-policy.yaml
max_attempts: 3
attempt_backoff_ms: [0, 100, 500]  # Delay before each retry

error_specific:
  import_error:
    max_attempts: 2  # Usually fixable
  syntax_error:
    max_attempts: 2  # Simple fix
  timeout:
    max_attempts: 1  # Likely algorithmic issue
  resource_exhausted:
    max_attempts: 0  # Don't retry, fail immediately
```

## 9. Security Considerations

### 9.1 Code Review for Sensitive Operations

Before executing code, scan for risky patterns:

```python
BLOCKED_PATTERNS = [
    r'subprocess\.',           # Shell execution
    r'os\.system\(',           # Shell execution
    r'exec\(',                 # Dynamic code execution
    r'eval\(',                 # Dynamic evaluation
    r'open\(.*/etc/',          # System file access
    r'socket\.',               # Raw network access
    r'__import__\(',           # Dynamic import
    r'pickle\.loads?',         # Deserialization attacks
]
```

### 9.2 Resource Isolation

| Resource | Limit | Enforcement |
|----------|-------|-------------|
| CPU | 1 core | cgroups |
| Memory | 512 MB | cgroups + OOM killer |
| Disk | 100 MB | quota |
| Network | Blocked during execution | iptables |
| Time | 30 seconds | Sandbox timeout |
| File descriptors | 64 | ulimit |

### 9.3 Data Classification

Generated code inherits data classification from its inputs:

```
Input: PII data → Output must be tagged PII
Input: Public data → Output can be Public or PII
```

## 10. Implementation Phases

### Phase 1: Basic Python Execution (MVP) ✅ IMPLEMENTED

**Status**: Complete (2026-02-08)

**Implementation**:
- ✅ `ccos.execute.python` capability registered in chat gateway
- ✅ Bubblewrap sandbox for Linux process isolation
- ✅ Security scanning for blocked patterns (subprocess, eval, exec, etc.)
- ✅ File mounting support (input files read-only, output directory writable)
- ✅ Resource limits (timeout: 30s default, memory: 512MB default)
- ✅ Stdout/stderr capture
- ✅ Output file collection with base64 encoding
- ✅ Output file extension filtering (png, csv, json, etc.)

**Files Created/Modified**:
- `ccos/src/sandbox/bubblewrap.rs` - Bubblewrap sandbox implementation
- `ccos/src/chat/mod.rs` - Python capability registration
- Pre-installed packages: pandas, numpy, matplotlib, requests

**Usage**:
```json
{
  "code": "import pandas as pd\ndf = pd.read_csv('/workspace/input/data.csv')\nprint(df.describe())",
  "input_files": {
    "data.csv": "/host/path/to/data.csv"
  },
  "timeout_ms": 30000,
  "max_memory_mb": 512
}
```

### Phase 2: Dependency Management ✅ IMPLEMENTED

**Status**: Complete (2026-02-08)

**Implementation**:
- ✅ Package allowlist configuration (auto-approved, requires-approval, blocked lists)
- ✅ Dynamic pip install in sandbox (before code execution)
- ✅ Package cache layer (pip download cache to /tmp/ccos-sandbox-cache)
- ✅ Multiple pre-baked images (python-data-science, python-web)
- ✅ Dependency resolution with approval gates
- ✅ Configurable via agent_config.toml

**Files Created/Modified**:
- `ccos/src/config/types.rs` - SandboxConfig, PackageAllowlistConfig, SandboxImageConfig
- `ccos/src/sandbox/dependency_manager.rs` - Dependency resolution and installation
- `ccos/src/sandbox/bubblewrap.rs` - Updated to support dependency installation
- `ccos/src/chat/mod.rs` - Updated Python capability to parse dependencies
- `config/agent_config.toml` - Added [sandbox] section with examples

**Configuration** (in agent_config.toml):
```toml
[sandbox]
enabled = true
runtime = "bubblewrap"
package_cache_dir = "/tmp/ccos-sandbox-cache"
enable_package_cache = true

[sandbox.package_allowlist]
auto_approved = ["pandas", "numpy", "matplotlib", "requests"]
requires_approval = ["torch", "tensorflow", "langchain"]
blocked = ["subprocess32", "pyautogui"]

[[sandbox.images]]
name = "python-data-science"
base = "python:3.12-slim"
packages = ["pandas>=2.0", "numpy>=1.26", "matplotlib>=3.8"]
```

**Usage**:
```json
{
  "code": "import pandas as pd\nimport openpyxl\ndf = pd.read_excel('/workspace/input/data.xlsx')",
  "input_files": {"data.xlsx": "/path/to/data.xlsx"},
  "dependencies": ["openpyxl"],
  "timeout_ms": 30000
}
```

### Phase 3: Specialized Coding Agents
- [x] Implement `ccos.delegate.coding_agent` capability
- [x] Configuration for multiple coding LLMs
- [x] Coding agent protocol (request/response schema)
- [x] Language-specific agent selection (via profile parameter)

### Phase 4: Iterative Refinement
- [x] Error parsing and classification (`sandbox/refiner.rs`)
- [x] Refinement prompt generation (`sandbox/coding_agent.rs`)
- [x] Retry policy configuration (`max_coding_turns`)
- [x] Attempt history tracking (Full history supported)
- [x] Advanced retry logic (Classification-based retry decisions)
- [x] Demo verification (`python_execution_demo.rs`, `iterative_refinement_demo.rs`)

### Phase 5: RTFS Integration
- [x] Implement `ccos.execute.rtfs` capability
- [x] Load standard library for RTFS execution
- [x] Verify with `rtfs_execution_demo.rs`

### Phase 6: JavaScript/Node.js Support
- [ ] Implement `ccos.execute.javascript` capability
- [ ] Node.js sandbox image
- [ ] npm package allowlist

## 11. Open Questions

### Resolved

2. ✅ **File I/O**: **RESOLVED** - Mount files into sandbox
   - Input files: Mounted read-only at `/workspace/input/{filename}`
   - Output files: Written to `/workspace/output/` and collected after execution

3. ✅ **Output artifacts**: **RESOLVED** - Base64 encode in response
   - Files collected from `/workspace/output/`
   - Base64 encoded and returned in JSON response
   - Extension filtering applied (png, csv, json, etc.)

### Still Open

1. **Stateful sandboxes**: Should code execution be able to persist state between calls? (e.g., Jupyter-like REPL)
   - Decision: Not for Phase 1, evaluate for Phase 3+

4. **Cost attribution**: How to charge for coding agent calls + sandbox compute?
   - Decision: Out of scope for Phase 1, requires accounting infrastructure

5. **Multi-step execution**: Should we support Python notebooks / multi-cell execution?
   - Decision: Not for Phase 1, evaluate based on user needs

6. **GPU access**: How to safely expose GPU for ML workloads?
   - Decision: Not for Phase 1, requires additional security review

## 12. References

- [CCOS Polyglot Sandboxed Capabilities](./spec-polyglot-sandboxed-capabilities.md)
- [CCOS Skill Interpreter](./spec-skill-interpreter.md)
- [Bubblewrap Sandbox](https://github.com/containers/bubblewrap)
- [Firecracker MicroVMs](https://firecracker-microvm.github.io/)
