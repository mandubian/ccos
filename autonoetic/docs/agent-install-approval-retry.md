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

## File Locations

| File | Description |
|------|-------------|
| `.gateway/scheduler/approvals/pending/<id>.json` | Approval request |
| `.gateway/scheduler/approvals/pending/<id>_payload.json` | Stored install payload |
| `.gateway/scheduler/approvals/approved/<id>.json` | Approved request |

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

## Related Documentation

- [Quickstart: Planner to Specialist Chat](quickstart-planner-specialist-chat.md)
- [Agent Routing and Roles](agent_routing_and_roles.md)
- [CLI Reference](cli-reference.md)
- [AGENTS.md](AGENTS.md) - Agent lifecycle and tools
