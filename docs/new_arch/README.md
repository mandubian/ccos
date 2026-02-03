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
│   │  • Causal Chain (Audit)     │                          │
│   │  • Approval Queue            │                          │
│   └──────────┬───────────────────┘                          │
│              │ X-Agent-Token                                 │
│              │ (Authenticated)                               │
│              ▼                                                │
│   ┌──────────────────────────────┐                          │
│   │       Agent (Deputy)         │                          │
│   │  • Event Polling             │                          │
│   │  • LLM Integration           │                          │
│   │  • Skill Loading             │                          │
│   │  • NO direct access          │                          │
│   └──────────────────────────────┘                          │
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
| `mock-moltbook` | Test API server | `cargo run --bin mock-moltbook` |

## Key Features

### Gateway Features
- **Session Management**: Isolated contexts per agent with unique tokens
- **Agent Spawning**: Automatic agent process management
- **Capability Gatekeeping**: All capability execution through Gateway APIs
- **Audit Trail**: Complete Causal Chain integration
- **HTTP API**: RESTful endpoints for agent communication

### Agent Features
- **Event Polling**: Continuous polling for new messages
- **LLM Integration**: OpenAI/Anthropic support for planning
- **Configuration Files**: TOML config support with profile management
- **Skill System**: Dynamic skill loading and onboarding
- **Jailed Execution**: No direct access to secrets or network

## Communication Flow

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

## Documentation Structure

```
docs/new_arch/
├── README.md                              # This file
├── ccos-gateway-agent-architecture.md     # Complete architecture spec
├── gateway-agent-quickstart.md            # Step-by-step guide
├── gateway-agent-features.md              # Feature reference
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
- **Skill development**: See [Onboarding Spec](spec-skill-onboarding.md)

---

*For the complete CCOS documentation, see [docs/ccos/README.md](../ccos/README.md)*
