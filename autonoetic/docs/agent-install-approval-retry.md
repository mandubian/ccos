# Agent Install Approval and Retry

This document describes how `agent.install` handles approval requirements and ensures deterministic retry after approval.

## Overview

When `agent.install` requires human approval, the gateway stores the exact install payload. On retry with `install_approval_ref`, the gateway uses the stored payload instead of the incoming arguments. This ensures fingerprint match and successful installation.

## Problem Solved

Before this feature, when an LLM-based agent (like `specialized_builder`) retried after approval:
1. The LLM might generate a different payload than the original request
2. The fingerprint (SHA256 hash of payload) wouldn't match
3. The gateway would reject the retry with "fingerprint mismatch" error
4. This caused repeated failures (demo-session-2 showed 3 failed retry attempts)

## How It Works

### 1. Approval Request Creates

When `agent.install` is called and approval is required:

```
┌─────────────────────────────────────────────────────────────────┐
│ agent.install (approval required)                               │
│ 1. Create approval request → pending/<request_id>.json          │
│ 2. Store install payload → pending/<request_id>_payload.json    │
│ 3. Return {"approval_required": true, "request_id": "..."}      │
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

# Approve
autonoetic gateway approvals approve <request_id> --reason "Approved"

# Reject
autonoetic gateway approvals reject <request_id> --reason "Not needed"
```

### 3. Retry Uses Stored Payload

When the caller retries with `install_approval_ref`:

```
┌─────────────────────────────────────────────────────────────────┐
│ agent.install (with install_approval_ref)                       │
│ 1. Validate approval exists in approved/<request_id>.json       │
│ 2. Validate agent_id and requester match                        │
│ 3. Load stored payload from pending/<request_id>_payload.json   │
│ 4. Use STORED payload (ignore incoming args)                    │
│ 5. Install succeeds with approved payload                       │
│ 6. Cleanup: delete pending/<request_id>_payload.json            │
└─────────────────────────────────────────────────────────────────┘
```

### 4. Payload Cleanup

After successful install, the stored payload file is automatically deleted. This:
- Prevents stale payloads from accumulating
- Frees disk space
- Maintains consistency (one payload per approval)

## File Locations

| File | Description |
|------|-------------|
| `.gateway/scheduler/approvals/pending/<id>.json` | Approval request |
| `.gateway/scheduler/approvals/pending/<id>_payload.json` | Stored install payload |
| `.gateway/scheduler/approvals/approved/<id>.json` | Approved request |

## Logging

The gateway logs payload storage/retrieval operations:

| Event | Level | Description |
|-------|-------|-------------|
| Payload stored | `info` | `"Stored install payload for retry"` |
| Payload loaded | `info` | `"Using stored install payload for deterministic retry"` |
| Payload cleaned up | `info` | `"Cleaned up stored install payload"` |
| Storage failed | `warn` | `"Failed to store install payload for retry (non-fatal)"` |
| No stored payload | `debug` | `"No stored payload found, using incoming args"` |

View logs with:
```bash
RUST_LOG=agent.install=info autonoetic gateway run --config <path>
```

## Backward Compatibility

If no stored payload exists (e.g., from before this feature was added), the gateway falls back to fingerprint validation. This ensures compatibility with existing approval workflows.

## Concurrency Handling

The system handles multiple concurrent install requests:
- Each approval has a unique UUID `request_id`
- Payloads are keyed by `request_id` in the filesystem
- No interference between concurrent installs
- Cleanup is scoped to the specific `request_id`

## Example Flow

```
1. specialized_builder calls agent.install for "weather.script.default"
2. Gateway: approval required → store payload → return request_id
3. User: autonoetic gateway approvals approve <request_id>
4. specialized_builder retries with install_approval_ref
5. Gateway: load stored payload → install succeeds → cleanup
```

## Related Documentation

- [Quickstart: Planner to Specialist Chat](quickstart-planner-specialist-chat.md)
- [Agent Routing and Roles](agent_routing_and_roles.md)
- [CLI Reference](cli-reference.md)
