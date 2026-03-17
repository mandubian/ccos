# Content Store

This document describes the hierarchical content storage system that enables parent-child agent content visibility.

## Overview

The content store provides **content-addressable storage** (SHA-256 based) for agent artifacts. Content written by child agents is automatically visible to their parent agents through a hierarchical delegation chain.

## Architecture

```
.gateway/content/sha256/     ← Immutable content blobs (shared)
└── ab/c123...               ← Content indexed by hash

.gateway/sessions/
├── demo-session-1/          ← Parent (planner) manifest
│   └── manifest.json
└── demo-session-1/coder-abc123/  ← Child (coder) manifest
    └── manifest.json
```

### Key Concepts

| Concept | Description |
|---------|-------------|
| **Content Handle** | SHA-256 hash prefixed with `sha256:` |
| **Short Alias** | 8 hex chars for LLM-friendly lookup |
| **Session Manifest** | Maps names/handles to content |
| **Parent Session** | Session that can see child's content |
| **Delegation Path** | Unique path for child: `{parent}/{agent-id}-{uuid}` |

## Hierarchical Visibility

### Model

Content written by a child is visible to:
1. **The child itself** - direct name lookup
2. **The parent** - hierarchical name lookup (`{child_session}/{name}`)
3. **All ancestors** - via parent chain traversal

```
Planner (demo-session-1)
  ↓ spawns
Coder (demo-session-1/coder-abc123)
  ↓ writes weather.py
```

The planner can read `weather.py` via:
- Hierarchical name: `coder-abc123/weather.py`
- Short alias: `745af7e6` (global)
- Full handle: `sha256:abc123...` (global)

### Sibling Isolation

Sibling agents cannot read each other's content directly:

```
Planner
├── Coder-A (writes file-a.py)  ← Planner can see
└── Coder-B (writes file-b.py)  ← Planner can see

Coder-A cannot see file-b.py ✗
Coder-B cannot see file-a.py ✗
```

## API Reference

### ContentStore Methods

| Method | Description |
|--------|-------------|
| `set_parent_session(child, parent)` | Establish parent-child relationship |
| `register_name_in_hierarchy(session, name, handle)` | Register in both child and parent manifests |
| `read_by_name_or_handle_hierarchical(session, identifier)` | Read with parent chain traversal |

### Manifest Structure

```json
{
  "names": {
    "weather.py": "sha256:abc123...",
    "coder-abc123/weather.py": "sha256:abc123..."
  },
  "aliases": {
    "745af7e6": "sha256:abc123..."
  },
  "persisted": ["sha256:abc123..."],
  "parent_session_id": "demo-session-1"
}
```

## Tool Integration

### content.write

When a child agent calls `content.write`:

1. Content is stored in the shared content store
2. Name is registered in child's manifest
3. Name is also registered in parent's manifest with hierarchical key

```json
// Child calls
content.write({"name": "weather.py", "content": "..."})

// Result
{
  "ok": true,
  "handle": "sha256:abc123...",
  "alias": "745af7e6",
  "name": "weather.py"
}
```

### content.read

When any agent calls `content.read`:

1. If identifier is a handle (`sha256:...`) → read directly
2. If identifier is an alias (8 hex chars) → walk parent chain
3. If identifier is a name → use hierarchical resolution

```json
// Parent calls
content.read({"name_or_handle": "coder-abc123/weather.py"})

// Also works with alias
content.read({"name_or_handle": "745af7e6"})
```

### agent.spawn

When spawning a child agent:

1. A unique delegation path is generated: `{session}/{agent-id}-{uuid8}`
2. Parent-child relationship is established in content store
3. Child receives the delegation path as its session_id

## Examples

### Planner Spawning Coder

```python
# Planner calls agent.spawn
result = agent.spawn({
    "agent_id": "coder.default",
    "message": "Write a weather script"
})

# Coder writes content
content.write({"name": "weather.py", "content": "import json..."})

# Planner can now read the coder's file
content.read({"name_or_handle": "745af7e6"})  # via alias
content.read({"name_or_handle": "coder-abc123/weather.py"})  # via hierarchical name
```

### Multi-File Project

```python
# Coder writes multiple files
content.write({"name": "src/main.py", "content": "..."})
content.write({"name": "src/utils.py", "content": "..."})
content.write({"name": "SKILL.md", "content": "..."})

# Planner reads them all
content.read({"name_or_handle": "coder-abc123/src/main.py"})
content.read({"name_or_handle": "coder-abc123/src/utils.py"})
```

## Testing

Run content store tests:

```bash
cargo test --lib content_store
```

Key test cases:
- `test_hierarchical_content_visibility` - parent reads child's content
- `test_hierarchical_content_isolation` - siblings cannot read each other
