# Pluggable Code Analysis

## Overview

Autonoetic uses a **pluggable analysis architecture** for code security and capability detection during `agent.install`. This allows different analysis strategies to be swapped at runtime, triggered by the gateway (NOT by the planner).

### Why Pluggable Analysis?

1. **Gateway-triggered**: Analysis runs automatically during install, independent of LLM agents
2. **Swappable providers**: Different strategies (pattern, LLM, hybrid) can be configured
3. **Extensible**: Custom analyzers can be added without changing core code
4. **Future-proof**: LLM-powered code review can be added when needed

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
  # Provider for capability detection: "pattern", "llm", "composite", "none"
  capability_provider: "pattern"

  # Provider for security analysis: "pattern", "llm", "composite", "none"
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
| `capability_provider` | string | `"pattern"` | Provider for capability analysis |
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

### Analysis Results are Advisory

- Capability mismatch → hard reject (unless `require_capabilities: false`)
- Security threats → may trigger human approval based on severity
- Remote access → logged but not automatically blocked

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

### AST-based Analysis

Future versions may add AST parsing for:
- Python (using `tree-sitter-python`)
- JavaScript/TypeScript (using `tree-sitter-javascript`)
- More accurate detection without false positives

---

## See Also

- [Agent Capabilities](./agent-capabilities.md) - Capability types and semantics
- [Agent Install Approval](./agent-install-approval-retry.md) - Approval flow details
- [Remote Access Approval](./remote-access-approval.md) - Network access detection
