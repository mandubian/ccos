# CCOS Gateway-Agent Documentation

This directory contains comprehensive documentation for the CCOS Gateway-Agent architecture - a jailed execution model for AI agents implementing the "Sheriff-Deputy" security pattern.

## Overview

The Gateway-Agent architecture separates the **high-privilege Gateway** (Sheriff) from the **low-privilege Agent** (Deputy), ensuring AI agents can only execute capabilities through governed channels with full audit trails.

### Key Documents

| Document | Purpose | Audience |
|----------|---------|----------|
| **[CCOS Gateway-Agent Architecture](ccos-gateway-agent-architecture.md)** | Complete architecture specification | System architects, security engineers |
| **[Gateway-Agent Quick Start](gateway-agent-quickstart.md)** | Step-by-step setup guide | Developers, operators |
| **[Feature Reference](gateway-agent-features.md)** | Detailed feature documentation | Developers, integrators |
| **[Iterative LLM Consultation](iterative-llm-consultation.md)** | Autonomous multi-step task execution | Developers, architects |
| **[Autonomy Implementation Plan](autonomy-implementation-plan.md)** | What's implemented vs next for Runs/autonomy | Developers, architects |
| **[Autonomy Backlog](ccos-chat-gateway-autonomy-backlog.md)** | Remaining work items for autonomy hardening | Maintainers |
| **[Budget Enforcement Spec](spec-resource-budget-enforcement.md)** | Current + planned budget enforcement semantics | Developers, operators |
| **[Skill Onboarding Specification](spec-skill-onboarding.md)** | Multi-step skill onboarding | Skill developers |
| **[Skill Interpreter Specification](spec-skill-interpreter.md)** | Skill parsing and execution | Skill developers |

## Quick Navigation

### Getting Started
1. Read the [Quick Start Guide](gateway-agent-quickstart.md)
2. Build and run the components
3. Test the security model

### Understanding the Architecture
1. Read the [Architecture Specification](ccos-gateway-agent-architecture.md)
2. Review the security model section
3. Understand the communication protocols

### Developing Skills
1. Read the [Skill Onboarding Spec](spec-skill-onboarding.md)
2. Study the [Skill Interpreter Spec](spec-skill-interpreter.md)
3. Reference the [Feature Reference](gateway-agent-features.md)

## Architecture at a Glance

```
┌─────────────────────────────────────────────────────────────┐
│                    CCOS Secure Boundary                       │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│   ┌──────────────────────────────┐                          │
│   │     Gateway (Sheriff)        │                          │
│   │  • Session Management        │                          │
│   │  • Token-based Auth          │                          │
│   │  • Capability Marketplace    │                          │
│   │  • Causal Chain (Audit)      │                          │
│   │  • Approval Queue            │                          │
│   │  • Real-Time Event Stream    │                          │
│   └──────┬───────────┬───────────┘                          │
│          │           │                                       │
│          │ X-Agent-Token                                    │
│          │ (Authenticated)                                  │
│          ▼           │                                       │
│   ┌──────────────────┐│                                      │
│   │    Agent         ││                                      │
│   │   (Deputy)       ││     ┌──────────────────────┐         │
│   │  • Event Polling │└────►│  Gateway Monitor     │         │
│   │  • LLM Integ.    │ WebSocket                  │         │
│   │  • Skill Loading │      │  • Live Dashboard    │         │
│   │  • NO direct     │      │  • Health Tracking   │         │
│   └──────────────────┘      └──────────────────────┘         │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Security Model

- **Gateway has privilege**: Direct access to secrets, network, filesystem
- **Agent is jailed**: No direct access, all requests through Gateway
- **Token-based auth**: Cryptographically secure, session-bound tokens
- **Capability governance**: All external operations through CCOS capabilities
- **Immutable audit**: All actions recorded in Causal Chain

## Core Binaries

| Binary | Purpose | Command |
|--------|---------|---------|
| `ccos-chat-gateway` | Gateway server | `cargo run --bin ccos-chat-gateway` |
| `ccos-agent` | Agent runtime | `cargo run --bin ccos-agent -- --token X --session-id Y [--config-path config/agent_config.toml]` |
| `ccos-chat` | Interactive chat TUI | `cargo run --bin ccos-chat` |
| `ccos-gateway-monitor` | Real-time monitoring dashboard | `cargo run --bin ccos-gateway-monitor` |
| `mock-moltbook` | Test API server | `cargo run --bin mock-moltbook` |

## Key Features

### Gateway Features
- **Session Management**: Isolated contexts per agent with unique tokens
- **Agent Spawning**: Automatic agent process management
- **Capability Gatekeeping**: All capability execution through Gateway APIs
- **Audit Trail**: Complete Causal Chain integration
- **HTTP API**: RESTful endpoints for agent communication
- **Real-Time Tracking**: WebSocket streaming of agent events and health status
- **Persistent Sessions**: Reconnect to existing sessions with auto-respawn
- **Agent Health Monitoring**: Heartbeat-based health checks with crash detection
- **Sandboxed Code Execution**: Python code execution in bubblewrap sandbox (Phase 1-2)
- **Dependency Management**: Package allowlist and dynamic pip install (Phase 2)

### Agent Features
- **Event Polling**: Continuous polling for new messages
- **LLM Integration**: OpenAI/Anthropic support for planning
- **Iterative LLM Consultation**: Autonomous multi-step task execution with dynamic planning
- **Configuration Files**: TOML config support with profile management
- **Skill System**: Dynamic skill loading and onboarding
- **Jailed Execution**: No direct access to secrets or network

## Communication Flow

### Basic Flow
```
1. User sends message via webhook
2. Gateway creates session + token
3. Gateway spawns Agent process
4. Agent polls Gateway for events
5. Agent processes with LLM (optional)
6. Agent executes capabilities through Gateway
7. Gateway injects secrets and makes external calls
8. All actions recorded in Causal Chain
```

### Iterative LLM Consultation Flow
```
1. Agent receives message
2. Iteration 1: LLM plans initial action
3. Agent executes action through Gateway
4. Iteration 2+: LLM analyzes result, plans next action
5. Repeat until LLM marks task_complete
6. Send final response to user
```

See [Iterative LLM Consultation](iterative-llm-consultation.md) for complete details.

## Documentation Structure

```
docs/new_arch/
├── README.md                              # This file
├── ccos-gateway-agent-architecture.md     # Complete architecture spec
├── gateway-agent-quickstart.md            # Step-by-step guide
├── gateway-agent-features.md              # Feature reference
├── iterative-llm-consultation.md          # Autonomous multi-step execution
├── autonomy-implementation-plan.md        # Autonomy plan + status
├── ccos-chat-gateway-autonomy-backlog.md  # Autonomy backlog
├── spec-resource-budget-enforcement.md    # Budget enforcement spec
├── spec-skill-onboarding.md               # Skill onboarding spec
└── spec-skill-interpreter.md              # Skill interpreter spec
```

## Testing

### Quick Automated Demo

The fastest way to see the full Gateway-Agent architecture in action:

```bash
# Run the Moltbook demo (builds, starts services, shows live logs)
./run_demo_moltbook.sh
```

This script demonstrates:
- Mock Moltbook server simulation
- Gateway session management
- Agent spawning and connection
- Message flow from webhook to LLM processing

### Real-Time Monitoring Demo

To see the new real-time tracking features:

```bash
# Terminal 1: Start Gateway + Monitor
./run_demo.sh

# Terminal 2: Start Chat Client
cargo run --bin ccos-chat
```

This demonstrates:
- WebSocket event streaming
- Agent health monitoring
- Session persistence across reconnections
- Real-time dashboard in the monitor TUI

### Manual Testing

For step-by-step manual testing:

```bash
# Run integration test with detailed instructions
./test_integration.sh
```

Or start services manually:
```bash
# Terminal 1: Mock Moltbook server
./target/release/mock-moltbook

# Terminal 2: Chat Gateway
./target/release/ccos-chat-gateway

# Terminal 3: Agent (get token from Gateway logs)
./target/release/ccos-agent --token <TOKEN> --session-id <SESSION_ID>
```

## Contributing

When modifying the Gateway-Agent system:

1. Update relevant specification documents
2. Add tests to `ccos/tests/`
3. Verify all binaries build: `cargo build --bin ccos-chat-gateway --bin ccos-agent`
4. Run integration tests

## Related Specifications

- [CCOS Chat Mode Security](../ccos/specs/037-chat-mode-security-contract.md)
- [CCOS Capability System](../ccos/specs/030-capability-system-architecture.md)
- [CCOS Causal Chain](../ccos/specs/003-causal-chain.md)

## Questions?

- **Architecture questions**: See [Architecture Spec](ccos-gateway-agent-architecture.md)
- **Setup issues**: See [Quick Start](gateway-agent-quickstart.md)
- **Feature details**: See [Feature Reference](gateway-agent-features.md)
- **Iterative execution**: See [Iterative LLM Consultation](iterative-llm-consultation.md)
- **Skill development**: See [Onboarding Spec](spec-skill-onboarding.md)

---

*For the complete CCOS documentation, see [docs/ccos/README.md](../ccos/README.md)*
