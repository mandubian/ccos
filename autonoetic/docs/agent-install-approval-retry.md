# Agent Install Approval and Retry

This document describes how `agent.install` handles approval requirements and ensures deterministic execution after approval.

## Overview

When `agent.install` requires human approval, the gateway:
1. Stores the exact install payload for later use
2. Generates a short, human-friendly approval ID
3. After approval, **automatically executes the install** (no agent retry needed)

## Approval ID Format

Approval IDs use a short, human-friendly format:

```
apr-<8-char-hex>
```

Example: `apr-db51b7ad`

| Feature | UUID (old) | Short ID (new) |
|---------|------------|----------------|
| Format | `db51b7ad-6277-4c88-b4bb-b3fbf8713fdc` | `apr-db51b7ad` |
| Length | 36 chars | 12 chars |
| LLM-safe | ❌ Truncated by Gemini | ✅ Won't truncate |
| Unique | 122 bits | 32 bits (4 billion) |

## How It Works

### 1. Approval Request Created

When `agent.install` is called and approval is required:

```
┌─────────────────────────────────────────────────────────────────┐
│ agent.install (approval required)                               │
│ 1. Create approval request → pending/<id>.json                  │
│ 2. Store install payload → pending/<id>_payload.json            │
│ 3. Return {"approval_required": true, "request_id": "apr-XXX"}  │
└─────────────────────────────────────────────────────────────────┘
```

The stored payload contains the full `InstallAgentArgs`:
- agent_id, name, description, instructions
- capabilities, files
- llm_config, execution_mode, script_entry
- background, scheduled_action

### 2. User Approval

```bash
# List pending approvals
autonoetic gateway approvals list

# Approve (short ID format)
autonoetic gateway approvals approve apr-db51b7ad --reason "Approved"

# Reject
autonoetic gateway approvals reject apr-db51b7ad --reason "Not needed"
```

### 3. Gateway Auto-Executes Install

After approval, the gateway automatically completes the install:

```
┌─────────────────────────────────────────────────────────────────┐
│ Approval resolved                                               │
│ 1. Load stored payload from pending/<id>_payload.json           │
│ 2. Add install_approval_ref to payload                          │
│ 3. Call AgentInstallTool.execute() directly                     │
│ 4. Install succeeds with approved payload                       │
│ 5. Log completion to causal chain                               │
│ 6. Cleanup: delete pending/<id>_payload.json                    │
└─────────────────────────────────────────────────────────────────┘
```

**No agent retry needed** - the gateway handles everything automatically.

### 4. Fallback: Manual Retry

If auto-execute fails (e.g., payload missing), the gateway sends a resume event to the caller as a fallback. The caller can then retry manually with `install_approval_ref`.

## File Locations (gateway root)

All paths below are **under `<agents_dir>/.gateway/`** (the gateway root). They are **not** directly under `agents/` except via that hidden `.gateway` directory.

| File | Description |
|------|-------------|
| `scheduler/approvals/pending/<id>.json` | Approval request metadata (`ApprovalRequest`) |
| `scheduler/approvals/pending/<id>_payload.json` | Install payload only (for `agent.install` auto-execute); **not** a full approval JSON — listing APIs skip this suffix |
| `scheduler/approvals/approved/<id>.json` | Approved request record |

Example (same tree as “File Structure After Install”):

```
agents/
└── .gateway/
    └── scheduler/
        └── approvals/
            ├── approved/
            └── pending/          ← requests + optional *_payload.json
```

## Delegation pause rule (planner + gateway)

**Policy:** While **any** approval tied to the **same root session** is still **pending** (e.g. `sandbox.exec` remote-access review, `agent.install` operator approval), the lead planner **must not** advance the workflow by delegating to another specialist.

**Gateway enforcement:** For **any** caller, `agent.spawn` is **rejected** if `scheduler/approvals/pending/` contains any request whose `session_id` shares the same **root** as this spawn (child sessions look like `<root>/<agent>-<suffix>`). The gateway does not look at agent roles — only root session + pending files. The error lists pending `apr-*` ids so the operator can `approvals approve|reject` and the user can say “continue”.

**Planner behavior:** Treat `approval_required` / pending `apr-*` like a **hard stop** for the next `agent.spawn` until approval resolves or you receive `approval_resolved` / user confirmation — aligned with the spawn guard above.

## Logging

The gateway logs approval and auto-execute operations:

| Event | Level | Description |
|-------|-------|-------------|
| Payload stored | `info` | `"Stored install payload for retry"` |
| Auto-execute start | `info` | `"Executing approved install directly via AgentInstallTool"` |
| Auto-execute success | `info` | `"Successfully auto-completed approved agent.install"` |
| Auto-execute failed | `warn` | `"Failed to execute approved install directly"` |
| Payload cleaned up | `info` | `"Cleaned up stored install payload"` |

View logs with:
```bash
RUST_LOG=approval=info,agent.install=info autonoetic gateway run --config <path>
```

## LLM Truncation Bug (Gemini 3 Flash)

Some LLMs (notably Gemini 3 Flash) truncate the last character of UUIDs when displaying them. This is why we use short IDs:

| LLM | UUID Output | Correct ID | Issue |
|-----|-------------|------------|-------|
| Nemotron 3 | Full UUID | Full UUID | ✅ Works |
| Gemini 3 Flash | Missing last char | Full UUID | ❌ Truncates |

Example from demo-session-2:
- **LLM displayed**: `db51b7ad-6277-4c88-b4bb-b3fbf8713fd` (35 chars)
- **Actual ID**: `db51b7ad-6277-4c88-b4bb-b3fbf8713fdc` (36 chars)
- **Short ID**: `apr-db51b7ad` (12 chars) - not affected

## Example Flow

```
1. specialized_builder calls agent.install for "weather_fetcher.default"
2. Gateway: approval required → store payload → return "apr-db51b7ad"
3. User: autonoetic gateway approvals approve apr-db51b7ad
4. Gateway: auto-execute install → weather_fetcher.default created
5. User sees agent installed immediately
```

## File Structure After Install

```
agents/
├── weather_fetcher.default/
│   ├── SKILL.md                    # Generated from payload
│   ├── runtime.lock                # Generated
│   └── scripts/
│       └── main.py                 # From payload files
└── .gateway/
    └── scheduler/
        └── approvals/
            ├── approved/
            │   └── apr-db51b7ad.json
            └── pending/
                └── (cleaned up after install)
```

## Gateway root for auto-execute

Post-approval `agent.install` runs `AgentInstallTool` with **`gateway_dir = agents_dir/.gateway`** (same as the live gateway). Artifact bundles live under `.gateway/artifacts/`; using the wrong root caused “artifact not found” in older builds.

## Remote access: two different mechanisms

| Mechanism | When it runs | What it detects | Triggers human approval? |
|-----------|--------------|-----------------|-------------------------|
| **`sandbox.exec`** | Before each sandbox run | `RemoteAccessAnalyzer` on the **command string** + resolved script text (imports, `https://`, IPs, …) | Yes, if patterns found and no valid `approval_ref` |
| **`agent.install` (analysis)** | During install | `AnalysisProvider` (default: pattern) on **artifact/SKILL files**: capabilities, security threats, `remote_access_detected` in `SecurityAnalysis` | Indirectly: high-risk install (e.g. `NetworkAccess`) or failed security / capability mismatch can require approval or reject |

They do **not** share the same code path. Code that never mentions `urllib`/`https` in the analyzed snippet may **not** trigger sandbox approval even if it would call HTTP at runtime (e.g. dynamic import). Install-time analysis can still flag network-like patterns in **source files** when installing an agent.

**Logs:** set `RUST_LOG=sandbox.exec=info` to see `will_require_approval` and `summary` on every `sandbox.exec` static scan.

**Duplicate approvals:** If remote access is required and an `apr-*` is **already pending** for that **same** tool `session_id`, the gateway does **not** create another file; the tool response includes `approval_already_pending` and the existing `request_id` so LLMs cannot flood `pending/` by retrying different commands.

## Related Documentation

- [Quickstart: Planner to Specialist Chat](quickstart-planner-specialist-chat.md)
- [Agent Routing and Roles](agent_routing_and_roles.md)
- [CLI Reference](cli-reference.md)
- [AGENTS.md](AGENTS.md) - Agent lifecycle and tools
