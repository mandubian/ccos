# Tool & Skill Repository Design for Autonoetic

> How Autonoetic should model its tool/skill repository — informed by Hermes-Agent and CCOS, but not copying either.

---

## The Problem

Autonoetic currently has:
- Tools hardcoded in `tools.rs` via scattered match statements
- Capabilities declared in SKILL.md frontmatter (static)
- No skill system separate from agents
- No progressive disclosure — all agent context loaded at once

This violates Autonoetic's own philosophy: **"The gateway provides primitives. Agents compose behaviors."**

The gateway is supposed to be a dumb secure pipe. But right now, adding a tool means modifying gateway code in multiple places. That's the opposite of a registry.

---

## What Hermes Does Right

### 1. Tool Registry (229 lines of Python)

Hermes' `tools/registry.py` is elegant:

```python
# Each tool self-registers at import time:
registry.register(
    name="web_search",
    toolset="web",
    schema={...},           # OpenAI-format JSON Schema
    handler=web_search_fn,  # Python callable
    check_fn=lambda: bool(os.getenv("SERPAPI_KEY")),
    requires_env=["SERPAPI_KEY"],
    is_async=False,
    description="Search the web..."
)
```

**Why it's good:**
- Tools are self-contained: each file declares its own metadata
- No central file to edit when adding a tool
- Schema + handler + capability check in one place
- `registry.get_definitions(tool_names)` returns OpenAI-format schemas
- `registry.dispatch(name, args)` executes with error wrapping
- Availability checking is automatic (check_fn)

**What Autonoetic should take:** The pattern, not the implementation. In Rust, this becomes a declarative macro or builder pattern.

### 2. Skill Progressive Disclosure (3 tiers)

Hermes separates skill access into tiers:

| Tier | Function | Returns | Token Cost |
|------|----------|---------|------------|
| 0 | `skills_categories()` | Category names + descriptions | ~50 tokens |
| 1 | `skills_list()` | name + description only | ~20 tokens/skill |
| 2 | `skill_view(name)` | Full SKILL.md content | ~500-2000 tokens |
| 3 | `skill_view(name, file_path)` | Specific linked file | varies |

**Why it's good:**
- Agent discovers what's available cheaply (tier 1)
- Only loads full content when needed (tier 2+)
- Linked files (references, templates, scripts) loaded on demand
- Prevents context window bloat

**What Autonoetic should take:** The tier structure. But Autonoetic's gateway mediation makes this even more natural — the gateway can enforce disclosure tiers.

### 3. Toolset Composition (542 lines)

Hermes' `toolsets.py` defines composable groups:

```python
TOOLSETS = {
    "web": {"tools": ["web_search", "web_extract"], "includes": []},
    "terminal": {"tools": ["terminal", "process"], "includes": []},
    "file": {"tools": ["read_file", "write_file", "patch", "search_files"], "includes": []},
    "debugging": {
        "tools": ["terminal", "process"],
        "includes": ["web", "file"]  # Composed from other toolsets
    },
}
```

**Why it's good:**
- `includes` enables composition without duplication
- Cycle detection in resolution
- Platform-specific toolsets (`hermes-cli`, `hermes-telegram`) share a core
- Runtime creation of custom toolsets

**What Autonoetic should take:** The composition pattern as a **convention**, not a gateway primitive. Agents declare toolsets in SKILL.md.

### 4. Skills Hub (multiple sources)

Hermes' `skills_hub.py` supports:
- GitHub repos (via Contents API)
- skills.sh marketplace
- ClawHub marketplace
- Well-known endpoints (`/.well-known/skills/index.json`)
- Trust levels: `builtin`, `trusted`, `community`
- Security scanning before installation

**What Autonoetic should take:** The multi-source pattern, but as gateway primitives (`skill.install`, `skill.search`) rather than a monolithic hub.

---

## What CCOS Does Wrong (for Autonoetic)

CCOS's CapabilityMarketplace (13 files, ~3500 lines):
- 9 provider types (MCP, OpenAPI, A2A, Local, Http, File, Script, HTTP2, Custom)
- Complex manifest struct with versioning, domain indexing, trust tiers
- Separate discovery agents
- LLM-driven synthesis pipeline
- Governance gates for risk classification

**Why it's too complex for Autonoetic:**
- Autonoetic's gateway is supposed to be simple
- The synthesis pipeline is an LLM task, not a gateway task
- 9 provider types is overkill — 3-4 is enough
- Governance gates are domain logic that should live in agents

**What CCOS proves:** Multi-source discovery, session consolidation, and missing capability resolution are valuable patterns. But they can be implemented as ~5 gateway primitives instead of a 3500-line marketplace.

---

## Proposed Autonoetic Design

### Key Insight: Skills ≠ Agents ≠ Tools

| Entity | What It Is | Where It Lives | Lifecycle |
|--------|-----------|---------------|-----------|
| **Tool** | Executable gateway primitive | Gateway code (Rust) | Registered at startup |
| **Agent** | SKILL.md + runtime + state | `{data}/agents/{id}/` | Full session (spawn/execute/hibernate) |
| **Skill** | Injectable context fragment | `{data}/skills/store/{hash}/` (shared, content-addressable) | At wake time (injected into prompt) |

**Critical distinction:**
- **Tools** are executable. The gateway runs them. They're Rust code.
- **Skills** are contextual. The gateway injects them. They're Markdown.
- **Agents** are full runtimes. The gateway manages them. They're directories.

### Agent Composition: Delegation, Not Inheritance

Agents compose via `agent.spawn` — the existing delegation primitive. There is no agent-level `composes:` or inheritance in manifests. If an agent needs architect patterns, it spawns `architect.default` as a separate session. The planner already handles decomposition this way.

The agent-adapter wraps another agent to reshape its I/O interface — that's the extent of "agent composition." No new primitives needed.

### 1. Tool Registry (Rust)

Instead of scattered match statements in `tools.rs`, use a declarative registry:

```rust
// In tool_registry.rs (new file, ~150 lines)
pub struct ToolEntry {
    pub name: String,              // "content.write"
    pub schema: Value,             // OpenAI-format JSON Schema
    pub handler: ToolHandler,      // fn(args: Value, ctx: &Context) -> Result<Value>
    pub capability: Capability,    // Required capability to invoke
    pub check_fn: Option<CheckFn>, // Availability check (e.g., env vars present)
    pub description: String,
}

pub struct ToolRegistry {
    tools: HashMap<String, ToolEntry>,
}

impl ToolRegistry {
    pub fn register(&mut self, entry: ToolEntry) { ... }
    pub fn get_definitions(&self, names: &[&str]) -> Vec<Value> { ... }
    pub fn dispatch(&self, name: &str, args: Value, ctx: &Context) -> Result<Value> { ... }
    pub fn available_tools(&self, capabilities: &[Capability]) -> Vec<&ToolEntry> { ... }
}

// Tool files self-register via macro:
register_tool! {
    name: "content.write",
    schema: { ... },
    capability: Capability::WriteAccess,
    handler: content_write_handler,
}
```

**Benefits:**
- Adding a tool = adding a file + one macro call
- Schema is co-located with handler
- Capability checking is automatic
- `available_tools()` filters by agent's declared capabilities
- Gateway code is cleaner — no giant match statement

### 2. Skill Repository (Content-Addressable, Shared)

Skills are stored in a **shared content-addressable store** and referenced by agents via SKILL.md frontmatter. This avoids duplication while keeping capsules portable.

**Directory structure:**
```
{gateway_data}/skills/
├── store/                              # Content-addressable (deduplicated)
│   ├── a3f2b8c1.../                    # SHA-256 of SKILL.md content
│   │   ├── SKILL.md
│   │   ├── references/                 # Optional linked files
│   │   └── scripts/                    # Optional implementation
│   └── c91d4e7f.../
│       └── SKILL.md
├── registry.json                       # name → hash mapping
└── catalog.json                        # Searchable index (name, desc, tags, source)
```

**Agent declares skills in SKILL.md frontmatter:**
```yaml
---
name: coder.default
description: "Produces runnable code artifacts"
metadata:
  autonoetic:
    agent:
      id: coder.default
    capabilities:
      - type: ToolInvoke
        allowed: ["content.", "knowledge.", "sandbox."]
    skills:                        # Skills to load at wake
      - python-patterns
      - rust-idioms
      - fastapi-builder
---
```

No separate `skills_manifest.yaml` — everything is in the SKILL.md the gateway already reads.

**SKILL.md format (for skills themselves):**
```markdown
---
name: nitter-scraper
description: Scrape Twitter/X via Nitter instances
version: 1.0.0
tags: [web, scraping, twitter]
platforms: [linux, macos]
required_env: [NITTER_INSTANCE]
---
# Nitter Scraper
Full instructions here...
```

**Gateway primitives:**

| Tool | Signature | Description |
|------|-----------|-------------|
| `skill.list` | `(query?, category?, offset?, limit?) → {skills, total, has_more}` | Paginated metadata search |
| `skill.view` | `(name: string, file?: string) → content` | Full content or linked file |
| `skill.install` | `(source: string, name?: string) → result` | Install from remote source |
| `skill.uninstall` | `(name: string) → result` | Remove a skill |
| `skill.search` | `(query: string, sources?: [string]) → [results]` | Search across sources |

**Pagination is mandatory** — no "dump all skills" mode:
```
skill.list(query="python", category="coding", limit=20, offset=0)
→ { skills: [...], total: 47, has_more: true }
```

**Wake sequence:**
```
1. Load SKILL.md (agent instructions + skills list)
2. Gateway resolves skills: registry.json → store/{hash}/SKILL.md
3. Each skill's SKILL.md injected into context (~500-2000 tokens/skill)
4. Agent can access linked files: skill.view("nitter-scraper", "references/api.md")
```

**Capsule export/import (portability + deduplication):**
```
Export: agent + all resolved skill CONTENT bundled into capsule.zip
        (self-contained, can be emailed/committed/shared)
Import: skills written to shared store, deduplicated by content hash
        (if same skill already exists, no duplicate stored)
```

**Why shared store, not per-agent directories:**
- `python-patterns` skill used by coder.default, researcher.default, architect.default — write once, reference many
- If agent A is deleted, its skills survive (other agents may use them)
- Content-addressable: identical skills never stored twice
- Capsule export bundles actual content — still portable

### 3. Toolset Convention (Agent-level, not gateway-level)

Toolsets are a **convention** agents declare, not a gateway primitive:

```yaml
# In agent's SKILL.md frontmatter:
capabilities:
  - type: ToolInvoke
    allowed: ["content.", "knowledge.", "web.", "sandbox."]
    toolset: research  # Convention name, informational only
```

The gateway checks `allowed` prefixes (capability-based enforcement). The `toolset` name is for humans and discovery. Zero gateway code needed.

### 4. MCP Integration as First-Class Tools

MCP tools register into the same tool registry:

```rust
// MCP tools discovered at startup or on-demand
registry.register(ToolEntry {
    name: "mcp.github.list_issues",
    schema: mcp_tool_schema,
    handler: mcp_handler(server="github", tool="list_issues"),
    capability: Capability::ToolInvoke,
    check_fn: Some(|| mcp_server_connected("github")),
    description: "List GitHub issues via MCP",
});
```

They appear in `skill.list` with a `source: "mcp"` tag. No separate system needed.

### 5. Skill Sources (Multi-registry)

Skills can come from multiple sources, similar to Hermes' hub:

```rust
pub enum SkillSource {
    Local,           // ~/.autonoetic/skills/
    GitHub { repo: String, path: String },
    WellKnown { url: String },  // /.well-known/skills/index.json
    Mcp { server: String },     // MCP server exposes skills
}
```

Gateway primitives handle the complexity:
- `skill.install("github:openai/skills/skill-creator")`
- `skill.install("well-known:https://example.com/.well-known/skills/")`
- `skill.search("twitter scraping", sources: ["local", "github"])`

Trust levels: `local` (trusted), `github:openai/` (trusted), `github:user/` (community)

---

## Comparison: This Design vs CCOS vs Hermes

| Aspect | CCOS Marketplace | Hermes Registry+Skills | Autonoetic (proposed) |
|--------|-----------------|----------------------|----------------------|
| **Tool registration** | Manifest struct, 9 provider types | Self-register at import | Self-register via macro |
| **Registry size** | ~3500 lines (13 files) | ~229 lines (1 file) | ~150 lines (1 file) |
| **Skills** | Part of capability system | Separate from agents | Separate from agents |
| **Progressive disclosure** | None (load all at once) | 3 tiers (categories→metadata→full) | 3 tiers (same) |
| **Toolset composition** | Domain indexing | `includes` field | Convention in SKILL.md |
| **MCP integration** | Separate provider type | Register as tools | Register as tools |
| **Sources** | 9 provider types | 4 source adapters | 4 source types |
| **Synthesis** | LLM pipeline (complex) | Agent creates via skill_manage | Agent creates via sandbox.exec |
| **Discovery** | Discovery agents + marketplace | skills_list + skills_hub | skill.list + skill.search |
| **Trust levels** | Complex tier system | 3 levels (builtin/trusted/community) | 3 levels (local/trusted/community) |

---

## Implementation Estimate

| Component | Lines | File |
|-----------|-------|------|
| Tool registry | ~150 | `autonoetic-gateway/src/runtime/tool_registry.rs` |
| Skill repository (filesystem) | ~200 | `autonoetic-gateway/src/skills/repository.rs` |
| Skill progressive disclosure tools | ~100 | `autonoetic-gateway/src/runtime/tools.rs` (skill.* tools) |
| Skill hub (multi-source) | ~250 | `autonoetic-gateway/src/skills/hub.rs` |
| SKILL.md parser | ~50 | `autonoetic-gateway/src/skills/parser.rs` |
| MCP-as-tools registration | ~100 | `autonoetic-gateway/src/skills/mcp_adapter.rs` |
| **Total** | **~850** | |

Compare to:
- CCOS marketplace: ~3500 lines
- Hermes registry+skills+hub: ~2200 lines (Python)

Autonoetic achieves the same functionality in ~850 lines because:
1. Rust is more concise for this pattern
2. No synthesis pipeline (agents do it via sandbox.exec)
3. Toolsets are convention, not code
4. Gateway mediation means fewer edge cases

---

## Migration Path

1. **Phase 1**: Create `tool_registry.rs`, migrate existing tools from match statements
2. **Phase 2**: Create `skills/` directory structure, add `skill.list`/`skill.view` tools
3. **Phase 3**: Add skill hub (local + GitHub sources)
4. **Phase 4**: Update agent wake sequence for progressive disclosure
5. **Phase 5**: Add MCP-as-tools registration
