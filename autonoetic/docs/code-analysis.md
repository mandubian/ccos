# Pluggable Code Analysis

## Overview

Autonoetic uses a **pluggable analysis architecture** for code security and capability detection during `agent.install`. This allows different analysis strategies to be swapped at runtime, triggered by the gateway (NOT by the planner).

### Why Pluggable Analysis?

1. **Gateway-triggered**: Analysis runs automatically during install, independent of LLM agents
2. **Swappable providers**: Different strategies (pattern, LLM, hybrid) can be configured
3. **Extensible**: Custom analyzers can be added without changing core code
4. **Future-proof**: LLM-powered code review can be added when needed

---

## Gateway security goal (defense in depth)

The gateway does **not** try to prove that code is harmless (that is impossible in general). Its job is to ensure that **sensitive behavior is not silent**: code must either **declare** the right capabilities, **pass** analysis, hit **policy** gates, or obtain **human approval** before high-risk operations proceed.

| Layer | Mechanism | When it runs |
|-------|-----------|----------------|
| **1. Install — bundle analysis** | `AnalysisProvider` (default [`PatternAnalyzer`](../autonoetic-gateway/src/runtime/analysis/pattern.rs)) over **artifact files + SKILL.md** | Every `agent.install` |
| **2. Install — capability contract** | Inferred vs declared capabilities; optional hard reject (`require_capabilities`) | `agent.install` |
| **3. Install — human approval** | Risk-based policy (e.g. `NetworkAccess`, broad `WriteAccess`, background, scheduled actions) | When install is classified high-risk |
| **4. Sandbox exec — scan + policy** | [`RemoteAccessAnalyzer`](../autonoetic-gateway/src/runtime/remote_access.rs) on resolved command/script text; `CodeExecution` shell policy; optional **approval** for remote-like patterns | Each `sandbox.exec` |
| **5. Tool & sandbox boundary** | Capability-gated tools (`web.*`, `content.*`, …); bubblewrap sandbox | Runtime |

**Why pluggable:** `AnalysisProvider` is the stable seam. Today: fast **pattern** (and optional **composite** / **LLM**). Tomorrow: the same trait can back a provider that invokes **Bandit**, **Semgrep**, or a **tree-sitter** AST walk—without rewriting `agent.install`.

**Honest limits:** Dynamic imports, C extensions, reflection, and deliberately evasive code can bypass static checks. The architecture assumes **layered controls + stronger providers over time**, not a single perfect pass.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    agent.install                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │           AnalysisProviderFactory                     │  │
│  │   (creates provider based on config)                  │  │
│  └─────────────┬──────────────┬──────────────┬──────────┘  │
│                │              │              │               │
│    ┌───────────▼──────┐  ┌───▼────────┐  ┌──▼──────────┐   │
│    │ PatternAnalyzer  │  │LlmAnalyzer │  │ Composite   │   │
│    │ (fast, default)  │  │ (future)   │  │ Analyzer    │   │
│    └──────────────────┘  └────────────┘  └─────────────┘   │
│                                                              │
│  Capabilities detected → compared with declared             │
│  Security threats      → block or require approval          │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### Analysis Flow

1. `agent.install` receives install payload with `files[]` and `capabilities[]`
2. Gateway creates analyzer based on `config.yaml` settings
3. Analyzer examines code and returns:
   - **Capability Analysis**: What capabilities the code requires
   - **Security Analysis**: Any security threats detected
4. If capabilities are missing → reject with clear error
5. If security threats found → block or require approval

---

## Providers

### PatternAnalyzer (Default)

Fast, deterministic analysis using pattern matching.

**Pros:**
- Very fast (~10ms)
- Deterministic results
- No external dependencies

**Cons:**
- May have false positives/negatives
- Limited to known patterns

### PythonAstAnalyzer (`python_ast`)

Runs a bundled **Python 3** script (`minimal_python_scan.py`) using only the **stdlib `ast`** module: walks imports and call sites for network-related modules, `open(..., write mode)`, file deletion helpers, `subprocess.*`, `os.system`, `eval`/`exec`.

**Pros:**
- No pip/Bandit/Semgrep install; only `python3` on `PATH`
- More structure-aware than substring search for Python
- If `python3` is missing or the script errors, **falls back to `PatternAnalyzer`**

**Cons:**
- Only `*.py` files in the install bundle are analyzed (SKILL.md body is not parsed as Python)
- Runs a subprocess per analysis call (capability + security each invoke the script once when both providers are `python_ast`)

**Detection Patterns:**

| Pattern | Capability/Threat |
|---------|-------------------|
| `urllib.request`, `requests.get`, `http://`, `https://` | NetworkAccess |
| `with open(`, `pathlib.Path(`, `fs.readFile` | ReadAccess |
| `os.remove`, `fs.unlink`, `os.makedirs` | WriteAccess |
| `subprocess.run`, `os.system`, `shell=True` | CodeExecution |
| `rm -rf /` | Critical: Destructive |
| `sudo`, `chmod 777` | High: Privilege Escalation |

### LlmAnalyzer (Future)

LLM-powered code review using configured model.

**Pros:**
- More accurate analysis
- Can detect complex patterns
- Understands context better

**Cons:**
- Slower (~3-5 seconds)
- Requires LLM API access
- Higher cost

**Status**: Stub implementation with fallback to pattern analysis.

### CompositeAnalyzer

Combines multiple providers with escalation logic.

**Escalation Policies:**

| Policy | Description |
|--------|-------------|
| `always` | Always run secondary analyzer |
| `on_threat_detected` | Escalate when primary finds threats |
| `on_low_confidence(threshold)` | Escalate when confidence below threshold |
| `for_capabilities(["NetworkAccess"])` | Escalate for specific capability types |

---

## Configuration

### config.yaml

```yaml
# Code analysis configuration for agent.install
code_analysis:
  # Provider for capability detection: "pattern", "python_ast", "llm", "composite", "none"
  capability_provider: "pattern"

  # Provider for security analysis: "pattern", "python_ast", "llm", "composite", "none"
  security_provider: "pattern"

  # Require capabilities to be declared (reject install if missing)
  require_capabilities: true

  # Capability types that always require human approval when detected
  require_approval_for:
    - "NetworkAccess"
    - "CodeExecution"

  # LLM configuration (for llm and composite providers)
  llm_config:
    provider: "openrouter"
    model: "google/gemini-3-flash-preview"
    temperature: 0.1
    timeout_secs: 30
```

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `capability_provider` | string | `"pattern"` | Provider for capability analysis (`python_ast` = stdlib ast scan for `.py`) |
| `security_provider` | string | `"pattern"` | Provider for security analysis |
| `require_capabilities` | bool | `true` | Reject if code requires undeclared capabilities |
| `require_approval_for` | list | `["NetworkAccess", "CodeExecution"]` | Capabilities requiring human approval |
| `llm_config.provider` | string | `"openrouter"` | LLM provider for LLM-based analysis |
| `llm_config.model` | string | `"google/gemini-3-flash-preview"` | Model for code analysis |
| `llm_config.temperature` | float | `0.1` | Temperature (lower = more deterministic) |
| `llm_config.timeout_secs` | int | `30` | Timeout for LLM analysis |

---

## Implementation

### AnalysisProvider Trait

All analysis providers implement this trait:

```rust
pub trait AnalysisProvider: Send + Sync + std::fmt::Debug {
    /// Name of this provider (e.g., "pattern", "llm")
    fn name(&self) -> &str;

    /// Analyze files to detect required capabilities
    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis;

    /// Analyze files for security threats
    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis;

    /// Perform combined analysis (optional override)
    fn analyze_combined(&self, files: &[FileToAnalyze]) -> CombinedAnalysis;

    /// Whether this provider requires async execution
    fn is_async(&self) -> bool { false }

    /// Estimated analysis duration in milliseconds
    fn estimated_duration_ms(&self) -> u64 { 100 }
}
```

### Creating a Custom Analyzer

```rust
use autonoetic_gateway::runtime::analysis::{
    AnalysisProvider, FileToAnalyze, CapabilityAnalysis, SecurityAnalysis,
};

#[derive(Debug)]
pub struct MyCustomAnalyzer;

impl AnalysisProvider for MyCustomAnalyzer {
    fn name(&self) -> &str {
        "my_custom_analyzer"
    }

    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis {
        let mut inferred_types = Vec::new();
        let mut evidence = Vec::new();

        for file in files {
            // Your custom detection logic
            if file.content.contains("urllib") {
                inferred_types.push("NetworkAccess".to_string());
                evidence.push(CapabilityEvidence {
                    file: file.path.clone(),
                    line: Some(1),
                    pattern: "urllib".to_string(),
                    capability_type: "NetworkAccess".to_string(),
                    confidence: 0.95,
                });
            }
        }

        CapabilityAnalysis {
            inferred_types,
            missing: vec![],
            excessive: vec![],
            confidence: 0.9,
            evidence,
            provider: self.name().to_string(),
        }
    }

    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis {
        // Your custom security logic
        SecurityAnalysis {
            passed: true,
            threats: vec![],
            remote_access_detected: false,
            confidence: 1.0,
            provider: self.name().to_string(),
        }
    }
}
```

### Registering a Custom Analyzer

Currently, custom analyzers need to be registered in `analysis/mod.rs`:

```rust
// In runtime/analysis/mod.rs
pub fn create_provider(provider_type: &AnalysisProviderType) -> Box<dyn AnalysisProvider> {
    match provider_type {
        AnalysisProviderType::Pattern => Box::new(PatternAnalyzer::new()),
        AnalysisProviderType::Llm => Box::new(LlmAnalyzer::new()),
        AnalysisProviderType::Custom("my_analyzer") => Box::new(MyCustomAnalyzer),
        // ...
    }
}
```

---

## Results

### CapabilityAnalysis

```json
{
  "inferred_types": ["NetworkAccess", "ReadAccess"],
  "missing": ["NetworkAccess"],
  "excessive": [],
  "confidence": 0.95,
  "evidence": [
    {
      "file": "main.py",
      "line": 5,
      "pattern": "urllib.request.urlopen",
      "capability_type": "NetworkAccess",
      "confidence": 0.95
    }
  ],
  "provider": "pattern"
}
```

### SecurityAnalysis

```json
{
  "passed": false,
  "threats": [
    {
      "threat_type": "Destructive",
      "severity": "Critical",
      "description": "Detected pattern: rm -rf",
      "file": "cleanup.py",
      "line": 10,
      "pattern": "rm -rf",
      "confidence": 0.90
    }
  ],
  "remote_access_detected": true,
  "confidence": 0.90,
  "provider": "pattern"
}
```

---

## Error Handling

When capability mismatch is detected, the install is rejected:

```json
{
  "ok": false,
  "error_type": "validation",
  "message": "Capability mismatch: code requires NetworkAccess but it was not declared in capabilities. Add these capabilities to your install request. (Analyzer: pattern)",
  "recoverable": true,
  "repair_hint": "Add NetworkAccess capability to your agent.install payload"
}
```

---

## Security Model

### Analysis is Gateway-Triggered

- Analysis runs automatically during `agent.install`
- NOT triggered by the planner or other agents
- Gateway controls which provider is used
- LLM analysis uses gateway's configured LLM, not agent's LLM

### Analysis results vs enforcement

- **Capability mismatch** → hard reject (unless `require_capabilities: false`)
- **Security threats** (install-time) → can fail install or drive policy; severity informs composite/LLM escalation
- **Remote access (install-time)** → recorded in `SecurityAnalysis.remote_access_detected`; must align with promotion-gate evidence when used
- **Remote access (`sandbox.exec`)** → if `RemoteAccessAnalyzer` finds patterns and there is no valid `approval_ref`, the tool returns **`approval_required`** (execution blocked until an operator approves and the agent retries with the ref)

### Trust Model

```
Low Trust (Pattern) ──────────────────────────── High Trust (LLM + Human)
      │                                                │
      │   Fast, may have false positives               │
      │   Works offline                                │
      │                                                │
      │                    ───────────────────────────► │
      │                     Slower, more accurate       │
      │                     Requires LLM API            │
      │                     Human review for high risk  │
```

---

## Future Enhancements

### LLM Analyzer Implementation

When enabled, the LLM analyzer will:

1. Send code to configured LLM with analysis prompts
2. Request structured JSON response with capabilities/threats
3. Parse and validate response
4. Return results to agent.install flow

**Prompt template:**
```
Analyze the following code for required capabilities and security threats.

Code:
{code}

Return JSON:
{
  "capabilities": ["NetworkAccess", ...],
  "threats": [{"type": "...", "severity": "...", "description": "..."}],
  "reasoning": "..."
}
```

### AST-based Analysis (Python)

The built-in **`python_ast`** provider uses CPython’s `ast` module (see `autonoetic-gateway/src/runtime/analysis/minimal_python_scan.py`). Deeper or multi-language AST analysis (e.g. tree-sitter) can be added as another `AnalysisProvider` later.

### External analyzers (optional integrations)

The same **`AnalysisProvider`** trait can wrap subprocess-based tools for **stronger** install-time signals (at the cost of latency and dependencies):

| Tool | Role |
|------|------|
| [Bandit](https://github.com/PyCQA/bandit) | Python-focused security findings (subprocess, unsafe patterns, …) |
| [Semgrep](https://semgrep.dev/) | Rule packs for network, file IO, secrets |
| Custom `ast` / tree-sitter | Deterministic import and call-site walks |

Implementation sketch: new `AnalysisProviderType` + provider that maps tool JSON/SARIF output into `CapabilityAnalysis` / `SecurityAnalysis`. Start with **artifact bundle only** (same inputs as today).

---

## See Also

- [Agent Capabilities](./agent-capabilities.md) - Capability types and semantics
- [Agent Install Approval](./agent-install-approval-retry.md) - Approval flow details
- [Remote Access Approval](./remote-access-approval.md) - Network access detection
