# Remote Access Approval

This document describes the static analysis system for detecting remote/network access in sandboxed code execution.

## Overview

When `sandbox.exec` is called, the code is **statically analyzed** before execution to detect patterns that require network access. If detected, execution is blocked and requires operator approval.

This is a **deterministic** security check that does not rely on the LLM's self-declaration.

## Why Static Analysis?

The LLM cannot be trusted to self-declare that code needs remote access:

```
User: "Fetch weather data"
  → LLM generates: import requests; requests.get("https://...")
  → LLM claims: "sandbox.exec with no network access"
  → ❌ LLM is wrong/misleading
```

Static analysis inspects the **actual code** to detect remote access patterns deterministically.

## Detection Categories

### 1. Network Library Imports

| Pattern | Example | Reason |
|---------|---------|--------|
| `import requests` | HTTP client | Makes HTTP requests |
| `from urllib import urlopen` | URL handling | Opens URLs |
| `import socket` | Low-level networking | TCP/UDP connections |
| `import httpx` | Async HTTP client | Makes HTTP requests |
| `import aiohttp` | Async HTTP client | Makes HTTP requests |
| `import ftplib` | FTP client | File transfer |
| `import smtplib` | SMTP client | Email sending |
| `import paramiko` | SSH client | Remote shell |
| `import boto3` | AWS SDK | Cloud access |
| `import google.cloud` | GCP SDK | Cloud access |

### 2. Network Function Calls

| Pattern | Example | Reason |
|---------|---------|--------|
| `.connect()` | `sock.connect(addr)` | Socket connection |
| `.send()` | `sock.send(data)` | Network transmission |
| `.recv()` | `sock.recv(1024)` | Network reception |
| `urlopen()` | `urlopen(url)` | URL connection |
| `requests.get()` | `requests.get(url)` | HTTP GET |
| `requests.post()` | `requests.post(url)` | HTTP POST |
| `httpx.get()` | `httpx.get(url)` | HTTP GET |

### 3. URL Literals

| Pattern | Example | Reason |
|---------|---------|--------|
| `https://` | `"https://api.example.com"` | External resource |
| `http://` | `"http://localhost:8080"` | HTTP endpoint |
| `ftp://` | `"ftp://server.com"` | FTP server |

**Excluded**: `example.com`, `localhost` (development patterns)

### 4. IP Address Literals

| Pattern | Example | Reason |
|---------|---------|--------|
| Public IP | `"192.168.1.100"` | External host |

**Excluded**: `127.x.x.x`, `0.0.0.0` (local/loopback)

## Approval Flow

```
┌─────────────────────────────────────────────────────────────┐
│ sandbox.exec called                                         │
│                                                             │
│ 1. Policy check (CodeExecution capability)                  │
│    ↓ allowed                                                │
│ 2. Static analysis (remote_access.rs)                       │
│    ├─ No remote patterns → Execute immediately              │
│    └─ Remote patterns found → BLOCK + require approval      │
└─────────────────────────────────────────────────────────────┘
```

### When Remote Access Detected

The tool returns a structured response instead of executing:

```json
{
  "ok": false,
  "exit_code": null,
  "stdout": "",
  "stderr": "Remote access detected: Detected 2 remote access pattern(s) in categories: import, url_literal. Operator approval required to execute code with network access.",
  "approval_required": true,
  "remote_access_detected": true,
  "detected_patterns": [
    {
      "category": "import",
      "pattern": "import requests",
      "line_number": 1,
      "reason": "HTTP client library"
    },
    {
      "category": "url_literal",
      "pattern": "https://api.open-meteo.com/v1/forecast",
      "line_number": 5,
      "reason": "URL literal indicates external resource access"
    }
  ]
}
```

## How to Approve Remote Access

When an agent encounters remote access approval:

1. **Agent reports the approval requirement** to the user
2. **User reviews the detected patterns** to understand what network access is needed
3. **User decides** whether to approve or deny
4. **If approved**, user can:
   - Grant `NetworkAccess` capability to the agent
   - Or provide an alternative implementation that doesn't require network access

## Pattern Details

### RemoteAccessAnalyzer

Located in: `autonoetic-gateway/src/runtime/remote_access.rs`

```rust
let analysis = RemoteAccessAnalyzer::analyze_code(code);

if analysis.requires_approval {
    // Return approval request
    return ApprovalRequired {
        detected_patterns: analysis.detected_patterns,
        summary: analysis.summary,
    };
}
// Proceed with execution
```

### DetectedPattern Structure

```rust
struct DetectedPattern {
    category: String,      // "import", "function_call", "url_literal", "ip_address"
    pattern: String,       // The matched text
    line_number: Option<usize>,  // Line where found (1-indexed)
    reason: String,        // Why this indicates remote access
}
```

## Testing

Run remote access analyzer tests:

```bash
cargo test --lib remote_access
```

Test coverage:
- No remote access (pure computation)
- HTTP import detection
- urllib import detection
- Socket call detection
- URL literal detection
- IP address detection
- Local IP exclusion
- Combined patterns (import + usage)

## Integration with Agent Capabilities

Agents that legitimately need network access should declare it:

```yaml
capabilities:
  - type: "NetworkAccess"
    hosts: ["api.open-meteo.com", "nominatim.openstreetmap.org"]
  - type: "CodeExecution"
    patterns: ["python3 "]
```

With `NetworkAccess` declared, the static analysis check can be bypassed for approved hosts.
