# CCOS CLI: Unified Command-Line Tool

**Status**: Planning  
**Created**: 2025-11-30  
**Umbrella Issue**: [#167](https://github.com/mandubian/ccos/issues/167)

## GitHub Issues

| Issue | Title | Phase | Priority |
|-------|-------|-------|----------|
| [#167](https://github.com/mandubian/ccos/issues/167) | [Umbrella] CCOS CLI: Unified Command-Line Tool | - | - |
| [#168](https://github.com/mandubian/ccos/issues/168) | Create CLI skeleton with clap subcommands | 1 | P0 |
| [#169](https://github.com/mandubian/ccos/issues/169) | Implement approval queue for external server discovery | 2 | P0 |
| [#170](https://github.com/mandubian/ccos/issues/170) | Add goal-driven discovery with external registry search | 3 | P1 |
| [#171](https://github.com/mandubian/ccos/issues/171) | Implement server health monitoring and auto-dismissal | 4 | P1 |
| [#172](https://github.com/mandubian/ccos/issues/172) | Port capability_explorer to CLI subcommands | 5 | P2 |

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

## Related Documents

- [Capability Explorer RTFS Mode](../ccos/guides/capability-explorer-rtfs-mode.md)
- [Missing Capability Resolution](../ccos/specs/032-missing-capability-resolution.md)
