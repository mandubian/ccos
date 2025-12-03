# CCOS CLI: Unified Command-Line Tool

**Status**: Implementation (Phases 1-9 complete)  
**Created**: 2025-11-30  
**Updated**: 2025-12-03  
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
| [#173](https://github.com/mandubian/ccos/issues/173) | Expose CLI as Governed Native Capabilities | 6 | Done |
| [#174](https://github.com/mandubian/ccos/issues/174) | [CLI UX] Interactive Mode and Discovery Filtering | 7 | Done |
| [#175](https://github.com/mandubian/ccos/issues/175) | [CLI UX] Intelligent Discovery with LLM Integration | 8 | Done |
| [#176](https://github.com/mandubian/ccos/issues/176) | [Plan] End-to-End Plan Creation & Execution | 9 | Done |

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

Commands marked with `★` are exposed as native RTFS capabilities (Phase 6).

```
ccos
├── discover                    # Capability discovery
│   ├── goal <goal>         ★   # Goal-driven discovery (ccos.cli.discovery.goal)
│   ├── server <name>       ★   # Discover from specific server
│   ├── search <query>      ★   # Search catalog (ccos.cli.discovery.search)
│   └── inspect <id>        ★   # Inspect capability details (ccos.cli.discovery.inspect)
│
├── server                      # Server management
│   ├── list                ★   # List configured servers (ccos.cli.server.list)
│   ├── add <url>           ★   # Add new server (ccos.cli.server.add)
│   ├── remove <id>         ★   # Remove server (ccos.cli.server.remove)
│   ├── search <query>      ★   # Search registries (ccos.cli.server.search)
│   └── health              ★   # Check server health (ccos.cli.server.health)
│
├── approval                    # Approval queue management
│   ├── pending             ★   # List pending approvals (ccos.cli.approval.pending)
│   ├── approve <id>        ★   # Approve a discovery (ccos.cli.approval.approve)
│   ├── reject <id>         ★   # Reject a discovery (ccos.cli.approval.reject)
│   └── timeout             ★   # List timed-out items
│
├── call <capability> [args] ★  # Execute a capability (ccos.cli.call)
│
├── plan                        # Planning
│   ├── create <goal>       ★   # Create plan from goal (ccos.cli.plan.create)
│   ├── execute <plan>      ★   # Execute a plan (ccos.cli.plan.execute)
│   └── validate <plan>     ★   # Validate plan syntax (ccos.cli.plan.validate)
│
├── rtfs                        # RTFS operations
│   ├── eval <expr>             # Evaluate RTFS expression
│   ├── repl                    # Interactive REPL (not exposed as capability)
│   └── run <file>              # Run RTFS file
│
├── governance                  # Governance operations
│   ├── check <action>      ★   # Check if action is allowed (ccos.cli.governance.check)
│   ├── audit               ★   # View audit trail (ccos.cli.governance.audit)
│   └── constitution        ★   # View/edit constitution (ccos.cli.governance.constitution)
│
├── explore                     # Interactive TUI (not exposed as capability)
│
└── config                      # Configuration
    ├── show                ★   # Show current config (ccos.cli.config.show)
    ├── validate            ★   # Validate config (ccos.cli.config.validate)
    └── init                ★   # Initialize new config (ccos.cli.config.init)
```

## Implementation Phases

### Phase 1: CLI Skeleton (Issue #168)
- Create `ccos/src/bin/ccos.rs` main entry point
- Set up clap with subcommand structure
- Implement shared CLI context (config loading, logging)
- Add `ccos config show` and `ccos config validate`

### Phase 2: Approval Queue System (Issue #169)
- Create `ccos/src/discovery/approval_queue.rs`
- Implement directory-based persistence for scalability:
  - `capabilities/servers/pending/<server_name>/`
  - `capabilities/servers/approved/<server_name>/`
  - `capabilities/servers/rejected/<server_name>/`
  - `capabilities/servers/timeout/<server_name>/`
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

### Phase 6: CLI as Governed Native Capabilities ✅

Expose CLI commands as RTFS-callable capabilities under governance control, enabling agents to programmatically discover tools, manage servers, and execute CLI operations while respecting security policies and constitutional rules.

### Phase 7: Interactive Mode and Discovery Filtering (Done)

Improve CLI usability for capability discovery by wiring up existing scoring infrastructure and adding interactive selection modes.
- Default non-interactive mode limits discovery results to top 3 to prevent queue spam.
- Explorer now isolates sessions per server, loading approved capabilities from disk to ensure correct OpenAPI/MCP provider handling.
- Improved JSON pretty-printing for capability results.

(Details moved to done section...)

### Phase 8: Intelligent Discovery with LLM Integration (Done)

LLM-enhanced discovery capabilities that provide "intelligent" search and ranking.

**Goal**: Enable `ccos discover goal "..." --llm` to use an LLM for:
1.  **Intent Analysis**: Understand the user's goal beyond keywords (e.g., "track project progress" -> implies "issues", "tasks", "gantt").
2.  **Query Expansion**: Generate multiple diverse search queries for registries (MCP, APIs.guru, Web).
3.  **Semantic Ranking**: Rank discovery results based on semantic relevance to the intent, providing reasoning for the score.

**Usage**:
```bash
# Standard keyword-based discovery
ccos discover goal "github issues"

# LLM-enhanced discovery with intent analysis and semantic ranking
ccos discover goal "github issues" --llm
```

**Implementation Details**:
- Created `ccos/src/discovery/llm_discovery.rs` with `LlmDiscoveryService`
- `LlmDiscoveryService::analyze_goal()` extracts intent using LLM:
  - Primary action (e.g., "list", "get", "track")
  - Target object (e.g., "issues", "weather")
  - Domain keywords, synonyms, implied concepts
  - Expanded search queries for better registry coverage
- `LlmDiscoveryService::rank_results()` scores candidates semantically:
  - LLM evaluates each candidate against the goal intent
  - Returns scores with reasoning
  - Recommends results above 0.6 threshold
- Integrated into `GoalDiscoveryAgent.search_and_score()`:
  - If `--llm`: analyze goal → search with expanded queries → rank with LLM
  - Fallback to keyword matching on LLM failure
- Cost control: pre-filters candidates before LLM ranking (max 15)

**Key Types**:
- `IntentAnalysis`: Structured goal analysis (action, target, keywords, synonyms, implied concepts)
- `RankedResult`: Discovery result with LLM score and reasoning
- `LlmSearchResult`: Combined results with optional intent analysis

#### Motivation

The current keyword-based scoring (`capability_matcher.rs`) is fast but limited. It fails on semantic gaps (e.g., "finding bugs" doesn't match "issue tracker" if keywords don't overlap). LLM integration bridges this gap.

### Phase 9: End-to-End Plan Execution (Done)

Fully integrated planning and execution pipeline, enabling natural language to executable RTFS plans using native CLI capabilities.

**Features**:
1.  **Plan Creation**: `ccos plan create "goal"` uses LLM (`LlmRtfsPlanGenerationProvider`) to generate RTFS plans.
2.  **Plan Execution**: `ccos plan execute <file>` or `ccos plan execute "rtfs code"` runs plans using the CCOS runtime.
3.  **Native Capability Wiring**: `ccos.cli.*` capabilities are automatically registered and available to the runtime during execution.
4.  **CLI-Runtime Bridge**: Unified `RuntimeContext` and `CapabilityMarketplace` setup for CLI commands.

**Key Components**:
- `ccos/src/ops/plan.rs`: Core planning logic.
- `ccos/src/ops/native.rs`: Centralized native capability registration.
- `ccos/src/cli/commands/plan.rs` & `call.rs`: CLI command wrappers.

### Future Enhancements

1. **Agent Autonomy**: Agents can discover new MCP servers, search for capabilities, and invoke CLI operations programmatically.
2. **Governance Control**: The Governance Kernel controls which CLI operations agents can invoke based on security levels and constitution rules.
3. **Dynamic Registration**: Newly-discovered capabilities become immediately callable without restart.
4. **Unified Interface**: Same operations available via CLI (humans) and RTFS (agents).

#### Implementation Steps

1. **Create `ccos::ops` Module**
   - Extract pure logic from CLI commands into `ccos/src/ops/` returning `RuntimeResult<T>` (serializable structs).
   - CLI commands become thin wrappers: call `ops::*`, format output.
   - Submodules: `server.rs`, `discover.rs`, `approval.rs`, `config.rs`, `plan.rs`.

2. **Add `ProviderType::Native` Variant**
   - Extend `ProviderType` enum with `Native(NativeCapability)`.
   - `NativeCapability` holds: handler closure, security_level, and optional metadata.

3. **Create `NativeCapabilityProvider`**
   - Registry of `ccos.cli.*` capabilities with auto-generated schemas.
   - Each capability declares: ID, input/output schema, security level, domains, categories.

4. **Integrate Governance Checks**
   - Extend `GovernanceKernel::detect_security_level` with `ccos.cli.*` patterns.
   - Add constitution rules for agent restrictions (e.g., no `config.init` without human approval).

5. **Support Dynamic Registration**
   - `CapabilityMarketplace::register_native_capability(id, handler, schema, security_level)`.
   - Hook into discovery so newly-approved servers can register capabilities at runtime.

6. **Update CLI Commands**
   - Refactor commands to call `ccos::ops` functions.
   - Output formatting remains in CLI layer.

#### Security Levels by Command

| Command | Capability ID | Security Level | Notes |
|---------|---------------|----------------|-------|
| `server list` | `ccos.cli.server.list` | low | Read-only |
| `server search` | `ccos.cli.server.search` | low | Read-only |
| `server add` | `ccos.cli.server.add` | medium | Queues for approval |
| `server remove` | `ccos.cli.server.remove` | high | Destructive |
| `discover goal` | `ccos.cli.discovery.goal` | low | Read-only search |
| `discover inspect` | `ccos.cli.discovery.inspect` | low | Read-only |
| `approval pending` | `ccos.cli.approval.pending` | low | Read-only |
| `approval approve` | `ccos.cli.approval.approve` | high | Modifies trust |
| `approval reject` | `ccos.cli.approval.reject` | medium | Modifies trust |
| `call` | `ccos.cli.call` | medium | Delegates to target capability |
| `config show` | `ccos.cli.config.show` | low | Read-only |
| `config init` | `ccos.cli.config.init` | critical | System modification |
| `governance check` | `ccos.cli.governance.check` | low | Read-only |
| `governance constitution` | `ccos.cli.governance.constitution` | critical | System modification |

#### Example Schema (server.list)

```rtfs
;; Capability: ccos.cli.server.list
;; Returns list of configured/approved servers

(deftype ServerInfo
  {:name String
   :endpoint String
   :status (Union :active :dismissed :pending)
   :health-score (Optional Float)})

(deftype ServerListOutput
  {:servers (Vector ServerInfo)
   :count Integer})

;; Input: none (empty map)
;; Output: ServerListOutput
```

#### Governance Integration

```rust
impl GovernanceKernel {
    pub fn detect_security_level(&self, capability_id: &str) -> String {
        let id_lower = capability_id.to_lowercase();
        
        // CLI capability patterns
        if id_lower.starts_with("ccos.cli.") {
            // Critical: system modification
            if id_lower.contains("config.init") 
                || id_lower.contains("governance.constitution") {
                return "critical".to_string();
            }
            // High: destructive or trust-modifying
            if id_lower.contains("remove") 
                || id_lower.contains("approve") {
                return "high".to_string();
            }
            // Medium: state-changing but safe
            if id_lower.contains("add") 
                || id_lower.contains("reject")
                || id_lower.contains("call") {
                return "medium".to_string();
            }
            // Default: read-only CLI operations
            return "low".to_string();
        }
        
        // ... existing patterns ...
    }
}
```

#### Constitution Rules Example

```toml
# In constitution.toml or as RTFS rules

[[rules]]
id = "cli-agent-restrictions"
description = "Agents cannot modify system configuration without human approval"
match = "ccos.cli.config.*"
action = "require-human-approval"

[[rules]]
id = "cli-discovery-allowed"
description = "Agents can freely discover and search capabilities"
match = "ccos.cli.discovery.*"
action = "allow"

[[rules]]
id = "cli-approval-restricted"
description = "Only humans can approve new servers"
match = "ccos.cli.approval.approve"
action = "require-human-approval"
```

#### Dynamic Registration Flow

```
Agent calls: (call :ccos.cli.server.search {:query "weather api"})
                    ↓
        GovernanceKernel checks security level ("low") → ALLOW
                    ↓
        NativeCapabilityProvider executes ops::server::search()
                    ↓
        Returns: [{:name "openweathermap" :endpoint "..." :status :pending}]
                    ↓
Agent calls: (call :ccos.cli.approval.approve {:id "openweathermap"})
                    ↓
        GovernanceKernel checks security level ("high") → REQUIRE APPROVAL
                    ↓
        Human approves via CLI or TUI
                    ↓
        Server approved → capabilities registered dynamically
                    ↓
Agent can now call: (call :openweathermap.get_current_weather {:city "Paris"})
```

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
├── ops/                            # Pure logic functions (Phase 6)
│   ├── mod.rs                      # ops module
│   ├── server.rs                   # server operations (list, search, add, remove)
│   ├── discover.rs                 # discovery operations (goal, inspect, search)
│   ├── approval.rs                 # approval operations (pending, approve, reject)
│   ├── config.rs                   # config operations (show, validate, init)
│   ├── plan.rs                     # plan operations (create, execute, validate)
│   └── governance.rs               # governance operations (check, audit)
├── capabilities/
│   ├── native_provider.rs          # Native capability provider (Phase 6)
│   └── ...
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
4. **Phase 6**: CLI commands callable as RTFS capabilities by agents
5. **Future**: Other examples migrated or deprecated

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
- [Governance Kernel](../ccos/specs/005-governance-kernel.md)
- [Capability System Architecture](../ccos/specs/030-capability-system-architecture.md)
