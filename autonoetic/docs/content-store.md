# Content Store

This document describes the content storage system with root-session visibility and artifact bundles.

## Overview

The content store provides **content-addressable storage** (SHA-256 based) for agent artifacts. Content is organized by a root-session visibility model where sessions sharing a root can collaborate.

## Architecture

```
.gateway/content/sha256/     ← Immutable content blobs (shared)
└── ab/c123...               ← Content indexed by hash

.gateway/sessions/
├── demo-session/            ← Root session manifest
│   └── manifest.json
└── demo-session/coder-abc123/  ← Child session manifest
    └── manifest.json

.gateway/artifacts/          ← Immutable artifact bundles
├── index.json
└── art_a1b2c3d4/
    └── manifest.json
```

### Key Concepts

| Concept | Description |
|---------|-------------|
| **Content Handle** | SHA-256 hash prefixed with `sha256:` |
| **Short Alias** | 8 hex chars for LLM-friendly lookup |
| **Session Manifest** | Maps names/handles to content with visibility |
| **Root Session ID** | Top-level session for visibility grouping |
| **Artifact** | Immutable file bundle for review/install/execution |

## Visibility Model

### Three Visibility Levels

| Visibility | Scope | Default |
|-----------|-------|---------|
| `private` | Only the writing session | No |
| `session` | All sessions under same root_session_id | **Yes** |
| `global` | Cross-session durable | No |

### Root Session

The root session is the portion before the first `/` in a session ID:

- `"demo-session"` → root is `"demo-session"`
- `"demo-session/coder-abc123"` → root is `"demo-session"`
- `"demo-session/coder-abc123/specialist"` → root is `"demo-session"`

All sessions sharing the same root can read each other's `session`-visible content.

### Visibility Behavior

```
Root: demo-session
├── Planner (demo-session)          writes weather.py (session) → visible to all
├── Coder (demo-session/coder-abc)  writes draft.py (private)   → only coder sees it
└── Evaluator (demo-session/eval-1) can read weather.py          → session visibility
                                   cannot read draft.py          → private
```

## API Reference

### `content.write`

Write content with visibility control.

```json
// Request
{
  "name": "src/main.py",
  "content": "print('hello')",
  "visibility": "session"
}

// Response
{
  "ok": true,
  "handle": "sha256:abc123...",
  "alias": "a1b2c3d4",
  "name": "src/main.py",
  "visibility": "session"
}
```

Default visibility is `session` (collaborative). Use `private` for scratchpads/drafts.

### `content.read`

Read by name, handle, or alias with root-based resolution.

```json
// Request
{
  "name_or_handle": "main.py"
}

// Response
{
  "ok": true,
  "content": "print('hello')"
}
```

Resolution order:
1. If `sha256:...` → direct content lookup
2. If 8 hex chars → alias lookup (session, then root)
3. Otherwise → name lookup (session, then root)

### `artifact.build`

Build an immutable artifact bundle from session content.

```json
// Request
{
  "inputs": ["src/main.py", "src/utils.py"],
  "entrypoints": ["src/main.py"]
}

// Response
{
  "ok": true,
  "artifact_id": "art_a1b2c3d4",
  "digest": "sha256:...",
  "files": [
    {"name": "src/main.py", "handle": "sha256:...", "alias": "a1b2c3d4"},
    {"name": "src/utils.py", "handle": "sha256:...", "alias": "u5e6f7g8"}
  ],
  "entrypoints": ["src/main.py"],
  "created_at": "2026-03-19T..."
}
```

### `artifact.inspect`

Inspect an artifact by ID.

```json
// Request
{
  "artifact_id": "art_a1b2c3d4"
}

// Response
{
  "ok": true,
  "artifact_id": "art_a1b2c3d4",
  "digest": "sha256:...",
  "files": [...],
  "entrypoints": [...],
  "created_at": "...",
  "builder_session_id": "..."
}
```

## Artifact Trust Boundary

**Core rule: no artifact, no review / no install / no execution beyond scratch.**

Artifacts are the only units that may:
- Be reviewed by evaluator/auditor
- Be installed
- Be executed beyond scratch use
- Cross trust boundaries

The workflow for any executable-producing task:

1. Coder writes files via `content.write`
2. Coder builds an artifact: `artifact.build(inputs, entrypoints)`
3. Evaluator/auditor review the artifact via `artifact.inspect`
4. Install/run consumes only the artifact ID

The artifact boundary must cover the full executable behavior surface, including:
- import and source resolution
- direct execution entrypoints
- runtime file-open/read/write access used by the executable

This closed-boundary rule applies equally to Python, shell, Node, generated scripts, config-driven runtimes, and similar executable file sets.

## Manifest Structure

```json
{
  "names": {
    "weather.py": "sha256:abc123..."
  },
  "aliases": {
    "a1b2c3d4": "sha256:abc123..."
  },
  "root_session_id": "demo-session",
  "visibility": {
    "sha256:abc123...": "session"
  }
}
```

## Examples

### Planner Spawning Coder

```json
// Planner spawns coder
agent.spawn({"agent_id": "coder.default", "message": "Write weather.py"})

// Coder writes content (session visibility by default)
content.write({"name": "weather.py", "content": "import json..."})

// Planner can read the coder's output
content.read({"name_or_handle": "weather.py"})

// Coder builds artifact for review
artifact.build({"inputs": ["weather.py"], "entrypoints": ["weather.py"]})

// Evaluator reviews the artifact
artifact.inspect({"artifact_id": "art_a1b2c3d4"})
```

### Private Scratch Work

```json
// Coder writes private draft (not visible to root/siblings)
content.write({"name": "draft.py", "content": "# scratch work", "visibility": "private"})

// Only the coder can read it
content.read({"name_or_handle": "draft.py"})  // works in coder session
// content.read in parent session → error
```

## Testing

```bash
cargo test --lib content_store
cargo test --lib artifact_store
cargo test --test content_storage_integration
```

Key test cases:
- `test_root_session_visibility` — parent reads child's session-visible content
- `test_private_visibility_isolates_from_root` — private content not visible to root
- `test_sibling_session_visibility` — siblings see each other's session content
- `test_artifact_build_and_inspect` — artifact lifecycle
