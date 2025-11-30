# CCOS CLI: Unified Command-Line Tool

**Status**: Implementation (Phases 1-5 complete)  
**Created**: 2025-11-30  
**Umbrella Issue**: [#167](https://github.com/mandubian/ccos/issues/167)

## GitHub Issues

| Issue | Title | Phase | Status |
|-------|-------|-------|----------|
| [#167](https://github.com/mandubian/ccos/issues/167) | [Umbrella] CCOS CLI: Unified Command-Line Tool | - | - |
| [#168](https://github.com/mandubian/ccos/issues/168) | Create CLI skeleton with clap subcommands | 1 | Done |
| [#169](https://github.com/mandubian/ccos/issues/169) | Implement approval queue for external server discovery | 2 | Done |
| [#170](https://github.com/mandubian/ccos/issues/170) | Add goal-driven discovery with external registry search | 3 | Done |
| [#171](https://github.com/mandubian/ccos/issues/171) | Implement server health monitoring and auto-dismissal | 4 | Done |
| [#172](https://github.com/mandubian/ccos/issues/172) | Port capability_explorer to CLI subcommands | 5 | Done |

## Overview

The `ccos` CLI will become the single entry point for all CCOS operations, replacing scattered examples with a unified, consistent interface.

## Motivation

### Current State: Scattered Tools
```
ccos/examples/
├── capability_explorer.rs      # Discovery, testing, RTFS mode
├── single_mcp_discovery.rs     # One-off MCP discovery
├── smart_assistant_demo.rs     # Assistant demo
├── run_planner.rs              # Planning
└── ... (many more examples)
```

Each example is standalone with its own CLI parsing, config loading, and initialization, leading to:
- Duplicated boilerplate
- Inconsistent UX (different flags, output formats)
- Hard to discover what's available

### Proposed: Unified `ccos` CLI
```bash
ccos <subcommand> [options]
```

## Command Structure

```
ccos
├── discover                    # Capability discovery
│   ├── goal <goal>             # Goal-driven discovery
│   ├── server <name>           # Discover from specific server
│   ├── search <query>          # Search catalog
│   └── inspect <id>            # Inspect capability details
│
├── server                      # Server management
│   ├── list                    # List configured servers
│   ├── add <url>               # Add new server (queues for approval)
│   ├── remove <id>             # Remove server
│   └── health                  # Check server health
│
├── approval                    # Approval queue management
│   ├── pending                 # List pending approvals
│   ├── approve <id>            # Approve a discovery
│   ├── reject <id>             # Reject a discovery
│   └── timeout                 # List timed-out items
│
├── call <capability> [args]    # Execute a capability
│
├── plan                        # Planning
│   ├── create <goal>           # Create plan from goal
│   ├── execute <plan>          # Execute a plan
│   └── validate <plan>         # Validate plan syntax
│
├── rtfs                        # RTFS operations
│   ├── eval <expr>             # Evaluate RTFS expression
│   ├── repl                    # Interactive REPL
│   └── run <file>              # Run RTFS file
│
├── governance                  # Governance operations
│   ├── check <action>          # Check if action is allowed
│   ├── audit                   # View audit trail
│   └── constitution            # View/edit constitution
│
└── config                      # Configuration
    ├── show                    # Show current config
    ├── validate                # Validate config
    └── init                    # Initialize new config
```

## Implementation Phases

### Phase 1: CLI Skeleton (Issue #168)
- Create `ccos/src/bin/ccos.rs` main entry point
- Set up clap with subcommand structure
- Implement shared CLI context (config loading, logging)
- Add `ccos config show` and `ccos config validate`

### Phase 2: Approval Queue System (Issue #169)
- Create `ccos/src/discovery/approval_queue.rs`
- Implement file-based persistence:
  - `capabilities/servers/pending.json`
  - `capabilities/servers/approved.json`
  - `capabilities/servers/rejected.json`
  - `capabilities/servers/timeout/`
- Add governance integration for auto-approval
- Implement `ccos approval` subcommands

### Phase 3: Goal-Driven Discovery (Issue #170)
- Implement domain extraction from natural language goals
- Add external registry search:
  - MCP Registry (registry.modelcontextprotocol.io)
  - apis.guru (OpenAPI directory)
  - Web search fallback
- Queue discovered servers for approval
- Implement `ccos discover goal` command

### Phase 4: Server Health Monitoring (Issue #171)
- Track server health (success/failure counts)
- Auto-dismiss after N consecutive failures
- Implement `ccos server health` command
- Add `ccos server dismiss` and `ccos server retry`

### Phase 5: Port capability_explorer (Issue #172)
- Move discovery logic to `ccos discover`
- Move call logic to `ccos call`
- Keep TUI as `ccos explore` (interactive mode)
- Deprecate standalone example

## File Structure

```
ccos/src/
├── bin/
│   └── ccos.rs                     # Main entry point
├── cli/
│   ├── mod.rs                      # CLI module
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── discover.rs             # discover subcommand
│   │   ├── server.rs               # server subcommand
│   │   ├── approval.rs             # approval subcommand
│   │   ├── call.rs                 # call subcommand
│   │   ├── plan.rs                 # plan subcommand
│   │   ├── rtfs.rs                 # rtfs subcommand
│   │   ├── governance.rs           # governance subcommand
│   │   └── config.rs               # config subcommand
│   ├── output.rs                   # Output formatting (table, json, rtfs)
│   └── context.rs                  # Shared CLI context
├── discovery/
│   ├── approval_queue.rs           # Approval queue system
│   ├── goal_discovery.rs           # Goal-driven discovery
│   ├── registry_search.rs          # External registry search
│   └── server_health.rs            # Server health monitoring
```

## Approval Queue Design

### States
```
pending → approved → (active server)
        → rejected → (dismissed)
        → timeout → (moved to timeout/)
```

### Risk Assessment
- **Low risk**: Known registries (MCP, apis.guru) with trusted domains → auto-approve via constitution
- **Medium risk**: Unknown but verified sources → queue for human approval (24h timeout)
- **High risk**: Web search results, unverified sources → require explicit human approval

### File Format

```json
// capabilities/servers/pending.json
{
  "items": [
    {
      "id": "discovery-abc123",
      "source": {
        "type": "mcp_registry",
        "entry": { ... }
      },
      "server_info": {
        "name": "twilio-mcp",
        "endpoint": "https://mcp.twilio.com/",
        "description": "Twilio MCP server for SMS/voice"
      },
      "domain_match": ["sms", "messaging"],
      "risk_assessment": {
        "level": "medium",
        "reasons": ["external_registry", "requires_auth"]
      },
      "requested_at": "2025-11-30T10:00:00Z",
      "expires_at": "2025-12-01T10:00:00Z",
      "requesting_goal": "send SMS notifications to customers"
    }
  ]
}
```

### Governance Integration

```rust
// Check if governance allows auto-approval
async fn check_governance_approval(
    pending: &PendingDiscovery,
    governance: &GovernanceKernel,
) -> Option<ApprovalDecision> {
    let context = GovernanceContext {
        action: "discover_external_server",
        risk_level: pending.risk_assessment.level,
        source: pending.source.name(),
        domain: pending.domain_match.clone(),
    };
    
    match governance.evaluate(&context).await {
        GovernanceResult::Allow { rule_id } => {
            Some(ApprovalDecision::Approved {
                by: ApprovalAuthority::Constitution { rule_id },
                at: Utc::now(),
            })
        }
        GovernanceResult::RequireHumanApproval => None,
        _ => None,
    }
}
```

## Discovery Sources

### Priority Order
1. **Local configured servers** (agent_config.toml)
2. **Local aliases** (capabilities/mcp/aliases.json)
3. **Discovered cache** (capabilities/discovered/)
4. **MCP Registry** (registry.modelcontextprotocol.io)
5. **apis.guru** (OpenAPI directory)
6. **Web search** (fallback)

### MCP Registry Search
```rust
// Already exists: ccos/src/mcp/registry.rs
let client = MCPRegistryClient::new();
let results = client.search_registry("sms messaging").await?;
```

### apis.guru Search
```rust
// New: search OpenAPI directory
let client = ApisGuruClient::new();
let results = client.search("twilio sms").await?;
// Returns: OpenAPI specs with endpoints
```

## Server Health Monitoring

```rust
pub struct ApprovedServer {
    pub id: String,
    pub source: DiscoverySource,
    pub server_config: ServerConfig,
    pub approved_at: DateTime<Utc>,
    pub approved_by: ApprovalAuthority,
    
    // Health tracking
    pub last_successful_call: Option<DateTime<Utc>>,
    pub consecutive_failures: u32,
    pub total_calls: u64,
    pub total_errors: u64,
}

impl ApprovedServer {
    pub fn should_dismiss(&self) -> bool {
        // Dismiss if > 5 consecutive failures
        // Or error rate > 50% over last 100 calls
        self.consecutive_failures > 5 || 
        (self.total_calls > 100 && self.error_rate() > 0.5)
    }
}
```

## Causal Chain Integration

Log events for audit trail:
- `DiscoveryQueued` - new server queued for approval
- `DiscoveryApproved` - server approved (by whom)
- `DiscoveryRejected` - server rejected (reason)
- `DiscoveryTimeout` - approval expired
- `ServerDismissed` - server auto-dismissed (health failure)
- `ServerRetried` - server manually re-enabled

## Migration Path

1. **Phase 1-2**: `ccos` CLI exists alongside examples
2. **Phase 3-4**: Feature parity with capability_explorer
3. **Phase 5**: capability_explorer becomes thin wrapper or deprecated
4. **Future**: Other examples migrated or deprecated

---

## Future Vision: CCOS Control Center TUI

The current `ccos explore` command provides capability exploration. The long-term vision is to evolve this into a full **CCOS Control Center** - an interactive dashboard for the entire cognitive OS lifecycle.

### Evolution Roadmap

#### Phase A: Capability Explorer (Current)
- Browse/search capabilities
- Inspect MCP servers and tools
- Test capability calls
- View capability metadata and schemas

#### Phase B: Goal → Plan Construction
- Natural language goal input panel
- Real-time planner visualization
- Watch decomposition into steps
- Intent graph construction visualization
- Interactive plan refinement (accept/reject/modify steps)
- Plan saving and versioning

#### Phase C: Execution Runtime Dashboard
- Live execution monitoring panel
- Step-by-step or continuous execution mode
- Pause/resume/abort controls
- Real-time causal chain visualization
- Variable/state inspection
- Execution timeline view
- Error handling and recovery options

#### Phase D: MicroVM/Container Deployment
- Package plan as deployable unit
- Configure isolation level:
  - MicroVM (Firecracker) for maximum isolation
  - Container (Docker/Podman) for lighter weight
  - Native execution for development
- Resource limits configuration (CPU, memory, network)
- Environment variable and secret injection
- Deploy to local or remote targets
- Deployment history and rollback

#### Phase E: Autonomous Agent Dashboard
- Multi-agent monitoring view
- Agent communication/delegation visualization
- Real-time message flow between agents
- Governance audit trail panel
- Health/performance metrics
- Agent lifecycle management (start/stop/restart)
- Capability marketplace integration (agent discovery)

### TUI Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ CCOS Control Center                              [Help] [Quit]  │
├─────────────┬───────────────────────────────────────────────────┤
│ Navigation  │  Main Panel                                       │
│             │                                                   │
│ ▶ Goals     │  ┌─────────────────────────────────────────────┐  │
│   Plans     │  │ Goal: "Search GitHub for Rust MCP libs"     │  │
│   Execution │  │                                             │  │
│   Deploy    │  │ Plan Steps:                                 │  │
│   Agents    │  │  1. [✓] Discover GitHub capabilities        │  │
│   ───────── │  │  2. [▶] Call github.search_repositories     │  │
│   Discover  │  │  3. [ ] Filter results by language          │  │
│   Servers   │  │  4. [ ] Format output                       │  │
│   Approvals │  │                                             │  │
│   Config    │  └─────────────────────────────────────────────┘  │
│             │                                                   │
├─────────────┼───────────────────────────────────────────────────┤
│ Status Bar  │ Step 2/4 | Runtime: 1.2s | Causal: 3 events      │
└─────────────┴───────────────────────────────────────────────────┘
```

### Key Features

1. **Keyboard-Driven Navigation**
   - Vim-style keybindings (j/k, h/l)
   - Quick-jump shortcuts (g+g, G, etc.)
   - Command palette (`:` prefix)

2. **Split Panes**
   - Resizable panels
   - Multiple views side-by-side
   - Focus switching with Tab

3. **Real-Time Updates**
   - Streaming execution output
   - Live causal chain updates
   - Agent status polling

4. **Persistence**
   - Session save/restore
   - Plan history
   - Execution logs

### Implementation Notes

- Built with `ratatui` (current capability_explorer foundation)
- Async runtime for non-blocking operations
- WebSocket support for remote agent monitoring
- Configuration via `agent_config.toml` TUI section

---

## Related Documents

- [Capability Explorer RTFS Mode](../ccos/guides/capability-explorer-rtfs-mode.md)
- [Missing Capability Resolution](../ccos/specs/032-missing-capability-resolution.md)
