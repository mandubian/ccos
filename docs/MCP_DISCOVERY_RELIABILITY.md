# MCP Discovery Reliability Analysis & Improvements

## Overview

This document analyzes the MCP discovery, persistence, and invocation pipeline to ensure reliable operation.

## Discovery Pipeline Flow

```
1. MCP Session Init
   ├─ POST /server/url with initialize request
   ├─ Extract Mcp-Session-Id header (if present)
   └─ Parse InitializeResult (server info, capabilities, protocol version)

2. Tools Discovery
   ├─ POST /server/url with tools/list request
   ├─ Include Mcp-Session-Id header (if from step 1)
   ├─ Parse tools array from response
   └─ Create DiscoveredMCPTool for each tool

3. Tool Selection & Introspection
   ├─ Select tool via LLM arbiter OR simple matching
   ├─ Extract parameters from user hint
   ├─ Call tools/call with test inputs (optional)
   └─ Create MCPIntrospectionResult

4. RTFS Generation
   ├─ MCPIntrospector.create_capability_from_mcp_tool()
   ├─ Generate implementation code
   └─ Save to capabilities/discovered/mcp/<namespace>/<tool_name>.rtfs

5. File Persistence
   ├─ Create directory structure
   ├─ Write RTFS file with metadata
   └─ Verify file written successfully

6. Loading & Registration
   ├─ CapabilityMarketplace.import_capabilities_from_rtfs_dir()
   ├─ MCPDiscoveryProvider.load_rtfs_capabilities()
   ├─ Parse RTFS module definition
   ├─ Convert to CapabilityManifest
   └─ Register in marketplace

7. Invocation
   ├─ CapabilityMarketplace.execute_capability()
   ├─ Look up capability by ID
   ├─ Extract MCP provider config
   ├─ Initialize new MCP session
   ├─ Call tools/call with inputs
   └─ Return result
```

## Potential Failure Points

### 1. **Session Initialization Failures**
**Location**: `ccos/src/synthesis/mcp_session.rs:63-145`

**Issues**:
- Network timeouts (30s limit)
- Authentication failures (401)
- Missing/invalid Mcp-Session-Id header handling

**Reliability Improvements**:
```rust
// CURRENT: Single timeout, no retries
request.timeout(std::time::Duration::from_secs(30))

// RECOMMENDED: Add retry logic with exponential backoff
for attempt in 1..=3 {
    match request.send().await {
        Ok(resp) => return handle_response(resp),
        Err(e) if attempt < 3 => {
            tokio::time::sleep(Duration::from_millis(100 * 2_u64.pow(attempt))).await;
            continue;
        }
        Err(e) => return Err(e),
    }
}
```

### 2. **File System Robustness**
**Location**: `ccos/src/synthesis/mcp_introspector.rs:939-1020`

**Issues**:
- Directory creation might fail silently
- File write might succeed but be incomplete
- No atomic write guarantees
- Path traversal vulnerabilities

**Reliability Improvements**:
```rust
// CURRENT: Basic create_dir_all
std::fs::create_dir_all(&dir_path)?;

// RECOMMENDED: Verify directory creation and use atomic writes
std::fs::create_dir_all(&dir_path)?;
if !dir_path.exists() {
    return Err(RuntimeError::Generic("Directory creation failed".into()));
}

// Use atomic write pattern
let temp_file = format!("{}.tmp", file_path);
std::fs::write(&temp_file, content)?;
std::fs::rename(&temp_file, file_path)?; // Atomic on most filesystems
```

### 3. **RTFS Parsing** ✅ FIXED
**Location**: `ccos/src/capability_marketplace/mcp_discovery.rs:791-925`

**Previous Issues**:
- ~~Simple string-based parsing (not a real parser)~~
- ~~Whitespace sensitivity~~
- ~~Missing error recovery~~
- ~~No schema validation~~

**Current Status**: ✅ **FIXED** - Now using proper RTFS parser
```rust
// CURRENT: Using real RTFS parser from rtfs crate
use rtfs::parser::parse;
let top_levels = parse(&rtfs_content)?;
self.extract_module_from_ast(top_levels)
```

**Implementation**:
- Uses `rtfs::parser::parse()` for proper AST generation
- Extracts module from parsed `TopLevel::Def` or `TopLevel::Expression`
- Type-safe navigation through AST structures
- Proper error messages from the parser

### 4. **Marketplace Registration Race Conditions**
**Location**: `ccos/src/capability_marketplace/marketplace.rs:2212-2300`

**Issues**:
- No duplicate detection
- Write lock held during file I/O
- Silent failures with debug callback
- No rollback on partial failure

**Reliability Improvements**:
```rust
// CURRENT: Lock held during I/O
let mut caps = self.capabilities.write().await;
for entry in entries {
    // ... file I/O happens here ...
    caps.insert(manifest.id.clone(), manifest);
}

// RECOMMENDED: Separate I/O from locking
let mut manifests = Vec::new();
for entry in entries {
    // Parse files without lock
    if let Ok(manifest) = parse_file(&entry) {
        manifests.push(manifest);
    }
}

// Acquire lock only for registration
let mut caps = self.capabilities.write().await;
for manifest in manifests {
    caps.insert(manifest.id.clone(), manifest);
}
```

### 5. **Session Reuse Issues**
**Location**: Throughout MCP provider implementations

**Issues**:
- Sessions created per-call (overhead)
- No session pooling
- Session expiry (404) not always handled
- No graceful degradation to stateless mode

**Reliability Improvements**:
- Implement session pool per server URL
- Add automatic session renewal on 404
- Fall back to stateless requests if sessions fail
- Cache authentication headers properly

## Diagnostic Tool Usage

We've created `/ccos/examples/diagnose_mcp_discovery.rs` to test each stage:

```bash
# Full diagnostic
cargo run --example diagnose_mcp_discovery -- \
  --server-url https://glama.ai/mcp/github

# Skip specific tests
cargo run --example diagnose_mcp_discovery -- \
  --server-url https://glama.ai/mcp/github \
  --skip-parsing \
  --skip-marketplace

# Custom output directory
cargo run --example diagnose_mcp_discovery -- \
  --server-url https://glama.ai/mcp/github \
  --output-dir /tmp/mcp_test
```

Tests performed:
1. **Session Init**: Verifies MCP session can be established
2. **File Ops**: Tests directory creation, file writing, reading
3. **RTFS Parsing**: Validates RTFS file can be parsed correctly
4. **Marketplace**: Confirms capabilities load into marketplace

## Recommended Quick Wins

### Priority 1: Add Retries to Session Initialization
```rust
// In mcp_session.rs initialize_session()
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 100;

for attempt in 0..MAX_RETRIES {
    match self.try_initialize(server_url, client_info).await {
        Ok(session) => return Ok(session),
        Err(e) if attempt < MAX_RETRIES - 1 => {
            let delay = BASE_DELAY_MS * 2_u64.pow(attempt);
            eprintln!("⚠️  Init attempt {} failed, retrying in {}ms: {}", attempt + 1, delay, e);
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        Err(e) => return Err(e),
    }
}
```

### Priority 2: Atomic File Writes
```rust
// In mcp_introspector.rs save_capability_to_rtfs()
let temp_path = final_path.with_extension("rtfs.tmp");
std::fs::write(&temp_path, &content)?;
std::fs::rename(&temp_path, &final_path)?;
```

### Priority 3: Better Error Reporting
```rust
// Add structured error types
#[derive(Debug)]
pub enum MCPDiscoveryError {
    SessionInit { server_url: String, source: Box<dyn Error> },
    ToolsListFailed { server_url: String, source: Box<dyn Error> },
    FileWriteFailed { path: PathBuf, source: std::io::Error },
    ParsingFailed { file: PathBuf, line: usize, source: String },
    MarketplaceRegistration { capability_id: String, source: Box<dyn Error> },
}
```

### Priority 4: Add Validation Layer
```rust
// Before saving RTFS file
pub fn validate_rtfs_capability(content: &str) -> Result<(), ValidationError> {
    // Check required fields
    // Verify RTFS syntax
    // Validate capability ID format
    // Ensure no path traversal in file names
}
```

## Testing Strategy

### Unit Tests
- Each pipeline stage should have unit tests
- Mock MCP servers for testing
- Test error paths explicitly

### Integration Tests
```rust
#[tokio::test]
async fn test_full_discovery_flow() {
    // 1. Start mock MCP server
    // 2. Run discovery
    // 3. Verify file created
    // 4. Load from file
    // 5. Invoke capability
    // 6. Verify result
}
```

### End-to-End Tests
```bash
# Use real MCP servers with known good responses
./scripts/test_mcp_discovery.sh https://glama.ai/mcp/github
```

## Monitoring & Observability

### Add Metrics
- Discovery success/failure rates
- File I/O latency
- Session initialization times
- Marketplace load times

### Structured Logging
```rust
use tracing::{info, warn, error, span, Level};

let span = span!(Level::INFO, "mcp_discovery", server_url = %url);
let _enter = span.enter();

info!("Initializing session");
match session_manager.initialize_session(url, client_info).await {
    Ok(session) => info!(session_id = ?session.session_id, "Session initialized"),
    Err(e) => error!(error = %e, "Session initialization failed"),
}
```

## Summary

The MCP discovery pipeline has several potential reliability issues:

1. ✅ **Session management**: Add retries, better 404 handling
2. ✅ **File operations**: Use atomic writes, verify directory creation
3. ✅ **Parsing**: More robust RTFS parsing with error recovery
4. ✅ **Registration**: Separate I/O from locking, add validation
5. ✅ **Monitoring**: Add structured logging and metrics

**Next Steps**:
1. Run diagnostic tool to identify current failure modes
2. Implement Priority 1-2 quick wins
3. Add comprehensive error types
4. Create integration test suite
5. Add telemetry for production monitoring
