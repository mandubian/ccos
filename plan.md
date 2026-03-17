# Autonoetic Gateway: Content Store & CodeExecution Improvements

## 1. Hierarchical Content Namespace

**Status**: ✅ COMPLETED

**Implementation**:
- `SessionManifest` now has `parent_session_id: Option<String>` field
- `ContentStore` has new methods:
  - `set_parent_session()` - establishes parent-child relationship
  - `register_name_in_hierarchy()` - registers content in both child and parent manifests
  - `read_by_name_or_handle_hierarchical()` - reads with parent chain traversal
  - `resolve_name_hierarchical()` - resolves names walking up parent chain
  - `resolve_alias_hierarchical()` - resolves aliases walking up parent chain
- `ContentWriteTool` uses `register_name_in_hierarchy()` for automatic parent visibility
- `ContentReadTool` uses `read_by_name_or_handle_hierarchical()` for parent chain lookup
- `AgentSpawnTool` generates unique delegation path for each child and sets parent relationship

**Model**:
```
demo-session-1/                    ← Planner reads here
└── demo-session-1/coder-abc123    ← Coder writes here
    ├── weather.py (also visible to parent via "coder-abc123/weather.py")
    └── SKILL.md
```

### Tests Passing (11/11)
- `test_hierarchical_content_visibility` - parent can read child's content
- `test_hierarchical_content_isolation` - siblings isolated, parent sees both

---

## 2. CodeExecution Remote Access - Static Analysis

**Status**: ✅ COMPLETED

**Implementation**:
- New `remote_access.rs` module with `RemoteAccessAnalyzer`
- Detects before execution:
  - Network library imports (`requests`, `urllib`, `socket`, `httpx`, etc.)
  - Network function calls (`.connect()`, `.send()`, `urlopen()`, etc.)
  - URL literals (`http://`, `https://`, `ftp://`)
  - IP address literals (excluding localhost/loopback)
- `SandboxExecTool` now runs static analysis before execution
- If remote access detected, returns structured approval request instead of executing

**Response when remote access detected**:
```json
{
  "ok": false,
  "approval_required": true,
  "remote_access_detected": true,
  "detected_patterns": [...],
  "stderr": "Remote access detected: ... Operator approval required..."
}
```

### Tests Passing (9/9)
- Import detection (requests, urllib, httpx)
- Function call detection (socket.connect, requests.get)
- URL literal detection
- IP address detection
- Local IP exclusion (127.x.x.x)
