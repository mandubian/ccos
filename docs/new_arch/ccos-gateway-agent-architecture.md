# CCOS Gateway-Agent Architecture

**Status**: Implemented and Operational
**Version**: 1.0
**Date**: 2026-02-01
**Authors**: CCOS Team

---

## 1. Executive Summary

The CCOS Gateway-Agent architecture implements a **jailed execution model** for AI agents, inspired by the "Sheriff-Deputy" security pattern. This architecture separates the **high-privilege Gateway** (the Sheriff) from the **low-privilege Agent** (the Deputy), ensuring that AI agents can execute capabilities only through governed channels.

### Key Innovation

Unlike traditional AI agent systems where the agent directly accesses tools and APIs, the CCOS architecture:
- **Jails the agent** - The agent has no direct access to secrets, APIs, or external systems
- **Elevates the Gateway** - The Gateway validates all actions and enforces governance
- **Token-based security** - Agents authenticate with revocable tokens tied to sessions
- **Capability marketplace** - All external interactions flow through CCOS's governed capability system

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CCOS Ecosystem                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   External World                      CCOS Secure Boundary                   │
│   ┌───────────────┐                   ┌──────────────────────────────────┐  │
│   │  Moltbook     │                   │        Gateway (Sheriff)         │  │
│   │  API Server   │◄─────────────────►│  ┌────────────────────────────┐  │  │
│   │  (External)   │    HTTP requests  │  │  Session Registry          │  │  │
│   └───────────────┘                   │  │  • Token generation        │  │  │
│                                       │  │  • Session lifecycle       │  │  │
│   ┌───────────────┐                   │  │  • Inbox management        │  │  │
│   │   Human       │◄─────────────────►│  └────────────────────────────┘  │  │
│   │  (Operator)   │   Approvals       │                                  │  │
│   └───────────────┘                   │  ┌────────────────────────────┐  │  │
│                                       │  │  Capability Marketplace    │  │  │
│   ┌───────────────┐                   │  │  • ccos.secrets.set        │  │  │
│   │   User        │──Webhooks────────►│  │  • ccos.chat.egress.*      │  │  │
│   │  (End User)   │                   │  │  • ccos.skill.*            │  │  │
│   └───────────────┘                   │  └────────────────────────────┘  │  │
│                                       │                                  │  │
│                                       │  ┌────────────────────────────┐  │  │
│                                       │  │  Causal Chain              │  │  │
│                                       │  │  • Immutable audit trail   │  │  │
│                                       │  │  • All actions recorded    │  │  │
│                                       │  └────────────────────────────┘  │  │
│                                       │                                  │  │
│                                       │  ┌────────────────────────────┐  │  │
│                                       │  │  Approval Queue            │  │  │
│                                       │  │  • Human action requests   │  │  │
│                                       │  │  • Secret storage approval │  │  │
│                                       │  └────────────────────────────┘  │  │
│                                       └──────────────────────────────────┘  │
│                                                     │                         │
│                                                     │ X-Agent-Token           │
│                                                     │ (Authenticated)         │
│                                                     ▼                         │
│                                       ┌──────────────────────────────────┐  │
│                                       │        Agent (Deputy)            │  │
│                                       │  ┌────────────────────────────┐  │  │
│                                       │  │  Event Polling Loop        │  │  │
│                                       │  │  • GET /chat/events        │  │  │
│                                       │  │  • Polls every N ms        │  │  │
│                                       │  └────────────────────────────┘  │  │
│                                       │                                  │  │
│                                       │  ┌────────────────────────────┐  │  │
│                                       │  │  LLM Integration           │  │  │
│                                       │  │  • OpenAI/Anthropic APIs   │  │  │
│                                       │  │  • Intent understanding    │  │  │
│                                       │  │  • Capability planning     │  │  │
│                                       │  └────────────────────────────┘  │  │
│                                       │                                  │  │
│                                       │  ┌────────────────────────────┐  │  │
│                                       │  │  Skill Loader              │  │  │
│                                       │  │  • Parse skill definitions │  │  │
│                                       │  │  • Onboarding state mgmt   │  │  │
│                                       │  └────────────────────────────┘  │  │
│                                       │                                  │  │
│                                       │  ┌────────────────────────────┐  │  │
│                                       │  │  No Direct Access          │  │  │
│                                       │  │  • No API keys             │  │  │
│                                       │  │  • No filesystem access    │  │  │
│                                       │  │  • No network egress       │  │  │
│                                       │  └────────────────────────────┘  │  │
│                                       └──────────────────────────────────┘  │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Gateway (Sheriff)

### 2.1 Core Responsibilities

The Gateway is the **security boundary** of the CCOS system. It is the only component with:
- Direct access to the Capability Marketplace
- Access to the SecretStore
- Network egress capabilities
- Approval queue management

**Primary Functions**:
1. **Session Management** - Create, validate, and manage agent sessions
2. **Token-based Authentication** - Issue and validate X-Agent-Token headers
3. **Capability Gatekeeping** - All capability execution flows through Gateway
4. **Message Routing** - Route webhooks to appropriate agent sessions
5. **Audit Trail** - Record all actions in the Causal Chain
6. **Agent Spawning** - Launch agent processes when sessions are created

### 2.2 Architecture Components

```
┌─────────────────────────────────────────────────────────────────┐
│                        Gateway Components                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    HTTP Router (Axum)                     │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  GET  /chat/health          - Health check               │  │
│  │  GET  /chat/inbox           - Debug: view messages       │  │
│  │  POST /chat/execute         - Execute capability         │  │
│  │       └─ Requires X-Agent-Token                          │  │
│  │  GET  /chat/capabilities    - List available caps        │  │
│  │       └─ Requires X-Agent-Token                          │  │
│  │  GET  /chat/events/:session - Poll for messages          │  │
│  │       └─ Requires X-Agent-Token                          │  │
│  │  GET  /chat/session/:id     - Get session info           │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  Session Registry                         │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  • create_session()     → SessionState                   │  │
│  │  • validate_token()     → Option<SessionState>           │  │
│  │  • get_session()        → Option<SessionState>           │  │
│  │  • push_message()       → Add to inbox                   │  │
│  │  • drain_inbox()        → Vec<ChatMessage>               │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  Agent Spawner                            │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  LogOnlySpawner:    Log spawn intent (for testing)       │  │
│  │  ProcessSpawner:    Spawn actual ccos-agent process      │  │
│  │                                                                  │
│  │  SpawnResult: { session_id, token, pid, message }        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              Capability Marketplace                        │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  • execute_capability(id, inputs) → Result<Value>        │  │
│  │  • list_capabilities() → Vec<CapabilityManifest>         │  │
│  │  • register_capability(manifest) → Result<()>            │  │
│  │                                                                  │
│  │  Includes: ccos.secrets.*, ccos.chat.*, ccos.skill.*     │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  Causal Chain                             │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  • append(action)           - Record action              │  │
│  │  • log_capability_call()    - Record capability exec     │  │
│  │  • record_result()          - Record execution result    │  │
│  │  • query_actions()          - Audit queries              │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                Approval Queue                             │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  Categories:                                              │  │
│  │    • SecretWrite          - Storing secrets              │  │
│  │    • HumanActionRequest   - Human intervention           │  │
│  │    • ChatPolicyException  - Override policy              │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.3 Session Lifecycle

```
┌─────────────────────────────────────────────────────────────────┐
│                     Session State Machine                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. CREATED                                                      │
│     Trigger: Webhook received from external source               │
│     Action:                                                      │
│       • Generate session_id (UUID)                               │
│       • Generate auth_token (secure random)                      │
│       • Create SessionState in registry                          │
│       • Spawn agent process with token                           │
│                                                                  │
│  2. ACTIVE                                                       │
│     Trigger: Agent connects and polls /chat/events               │
│     Action:                                                      │
│       • Validate X-Agent-Token header                            │
│       • Return messages from session inbox                       │
│       • Update last_activity timestamp                           │
│                                                                  │
│  3. EXECUTING                                                    │
│     Trigger: Agent calls POST /chat/execute                      │
│     Action:                                                      │
│       • Validate token and session_id match                      │
│       • Check capability exists                                  │
│       • Forward to CapabilityMarketplace                         │
│       • Record execution in Causal Chain                         │
│       • Return result to agent                                   │
│                                                                  │
│  4. PENDING_HUMAN_ACTION                                         │
│     Trigger: Capability creates approval request                 │
│     Action:                                                      │
│       • Add approval to queue                                    │
│       • Notify human via configured channels                     │
│       • Agent continues polling, approval checked each time      │
│                                                                  │
│  5. COMPLETED / EXPIRED                                          │
│     Trigger: Session timeout or explicit completion              │
│     Action:                                                      │
│       • Mark session status                                      │
│       • Optionally terminate agent process                       │
│       • Retain audit trail                                       │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 2.4 Security Model

**Trust Boundaries**:

| Boundary | Trust Level | Verification |
|----------|-------------|--------------|
| External → Gateway | Untrusted | Webhook signatures (configurable) |
| Agent → Gateway | Semi-trusted | X-Agent-Token validation |
| Gateway → Capabilities | Trusted | Internal call |
| Gateway → Causal Chain | Trusted | Internal call |

**Token Security**:
- Tokens are cryptographically secure random strings (32+ bytes)
- Tokens are tied to specific session IDs (cannot be used cross-session)
- Tokens have configurable TTL (default: session lifetime)
- Tokens are never logged or returned in API responses

**Capability Execution Flow**:

```
Agent Request: POST /chat/execute
  Headers:
    X-Agent-Token: <session_token>
    Content-Type: application/json
  Body:
    {
      "capability_id": "ccos.secrets.set",
      "inputs": { "key": "API_KEY", "value": "secret123" },
      "session_id": "sess_abc123"
    }

Gateway Validation:
  1. Extract X-Agent-Token from headers
  2. Lookup session by session_id
  3. Validate token matches session.auth_token
  4. Check session.status == Active
  5. Log: "Executing capability X for session Y"

CapabilityMarketplace:
  1. Lookup capability by capability_id
  2. Check capability.requires_approval
  3. If approval needed, check ApprovalQueue
  4. Execute capability handler
  5. Return result

Audit Trail:
  - Record in CausalChain: capability_call
  - Record inputs (secret values redacted)
  - Record success/failure
  - Link to session_id, run_id, step_id
```

---

## 3. Agent (Deputy)

### 3.1 Core Responsibilities

The Agent is a **jailed process** with minimal privileges. It is designed to be:
- **Stateless** - All state stored in Gateway or external memory
- **Ephemeral** - Can be restarted without data loss
- **Untrusted** - Cannot access secrets or make direct external calls

**Primary Functions**:
1. **Event Polling** - Continuously poll Gateway for new messages
2. **LLM Processing** - Use LLM to understand intent and plan actions
3. **Capability Invocation** - Execute capabilities through Gateway APIs
4. **Skill Loading** - Load and interpret skill definitions
5. **Onboarding Management** - Track and execute multi-step onboarding flows

### 3.2 Architecture Components

```
┌─────────────────────────────────────────────────────────────────┐
│                        Agent Components                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                   Main Runtime Loop                       │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  loop {                                                    │  │
│  │    1. Poll /chat/events/:session_id                      │  │
│  │    2. For each message:                                  │  │
│  │       a. Add to message_history                          │  │
│  │       b. If LLM enabled:                                 │  │
│  │          - Call LLM to generate plan                     │  │
│  │          - Execute each planned capability               │  │
│  │       c. Else:                                           │  │
│  │          - Send simple response                          │  │
│  │    3. sleep(poll_interval_ms)                            │  │
│  │  }                                                        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  LLM Integration                          │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  Providers Supported:                                     │  │
│  │    • OpenAI (GPT-3.5, GPT-4)                             │  │
│  │    • Anthropic (Claude)                                  │  │
│  │    • Local (echo mode for testing)                       │  │
│  │                                                                  │
│  │  Process:                                                   │  │
│  │    1. Build context from message_history                   │  │
│  │    2. Send to LLM with system prompt                     │  │
│  │    3. Parse response as AgentPlan:                       │  │
│  │       { understanding, actions[], response }             │  │
│  │    4. Execute each action through Gateway                │  │
│  │                                                                  │
│  │  System Prompt includes:                                  │  │
│  │    • You are a CCOS agent                                 │  │
│  │    • Available capability categories                      │  │
│  │    • Response format (JSON)                              │  │
  │  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              Configuration Management                     │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  Config File Support:                                     │  │
│  │    • TOML format (same as CCOS agent config)             │  │
│  │    • Load via --config-path or CCOS_AGENT_CONFIG_PATH    │  │
│  │                                                                  │
│  │  Config Overrides (precedence highest to lowest):        │  │
│  │    1. CLI arguments (e.g., --llm-provider)               │  │
│  │    2. Environment variables (e.g., CCOS_LLM_PROVIDER)    │  │
│  │    3. Config file values (e.g., llm_profiles.default)    │  │
│  │                                                                  │
│  │  Applied from Config:                                     │  │
│  │    • LLM profiles → Provider, model, API key env var    │  │
│  │    • capabilities.llm.enabled → --enable-llm             │  │
│  │    • Feature flags → Delegation, self-programming        │  │
│  │                                                                  │
│  │  Example:                                                 │  │
│  │    --config-path config/agent_config.toml                │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                 Capability Client                         │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  execute_capability(capability_id, inputs):              │  │
│  │    1. POST to Gateway /chat/execute                      │  │
│  │    2. Include X-Agent-Token header                       │  │
│  │    3. Send capability_id, inputs, session_id             │  │
│  │    4. Parse ExecuteResponse:                             │  │
│  │       { success, result, error }                         │  │
│  │    5. Return result or propagate error                   │  │
│  │                                                                  │
│  │  list_capabilities():                                     │  │
│  │    1. GET from Gateway /chat/capabilities                │  │
│  │    2. Returns list of available capabilities             │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                  Skill Loader                             │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  load_skill(url):                                         │  │
│  │    1. Fetch skill definition from URL                    │  │
│  │    2. Parse markdown/yaml/json format                    │  │
│  │    3. Extract operations, auth, onboarding steps         │  │
│  │    4. Check onboarding status in memory                  │  │
│  │    5. If onboarding needed, start workflow               │  │
│  │                                                                  │
│  │  Onboarding State Machine:                                │  │
│  │    • NOT_LOADED → LOADED → READY/NEEDS_SETUP             │  │
│  │    • PENDING_HUMAN_ACTION → OPERATIONAL                  │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              Security Boundaries (No Access)              │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  ❌ No filesystem access                                   │  │
│  │  ❌ No network egress (except to Gateway)                │  │
│  │  ❌ No secret access (secrets injected by Gateway)       │  │
│  │  ❌ No direct API calls (all through capabilities)       │  │
│  │  ❌ No persistent state (state in Gateway/memory)        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 3.3 CLI Arguments

```bash
ccos-agent \
  --config-path <PATH>               # Optional: TOML config file
  --token <TOKEN>                    # Required: Auth token from Gateway
  --session-id <SESSION_ID>          # Required: Session identifier
  --gateway-url <URL>                # Default: http://localhost:8080
  --poll-interval-ms <MS>            # Default: 1000
  --enable-llm                       # Enable LLM processing
  --llm-provider <openai|anthropic|local>  # Default: local
  --llm-api-key <KEY>                # Required if LLM enabled
  --llm-model <MODEL>                # Default: gpt-3.5-turbo
```

**Configuration File Support**:

The agent can load default settings from a TOML configuration file:

```bash
# Use default config
ccos-agent --config-path config/agent_config.toml --token ... --session-id ...

# Override config with CLI args
ccos-agent \
  --config-path config/agent_config.toml \
  --llm-provider anthropic \
  --llm-model claude-3-opus \
  --token ... \
  --session-id ...
```

**Config Precedence** (highest to lowest):
1. CLI arguments (e.g., `--llm-provider openai`)
2. Environment variables (e.g., `CCOS_LLM_PROVIDER=openai`)
3. Config file values (e.g., `llm_profiles.default = "openrouter_free:balanced"`)

**Loading Config from agent_config.toml**:

When `--config-path` is provided, the agent:
1. Parses the TOML configuration file
2. Applies `llm_profiles.default` settings (if CLI uses defaults)
3. Sets `enable_llm` based on `capabilities.llm.enabled`
4. Loads API keys from environment variables specified in `api_key_env`
5. CLI arguments override any config file values

### 3.4 Agent Planning Loop

```
┌─────────────────────────────────────────────────────────────────┐
│                    Agent Processing Flow                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  User Message Received                                           │
│         │                                                        │
│         ▼                                                        │
│  ┌─────────────────────────┐                                    │
│  │  1. Context Building    │                                    │
│  │     • message_history   │                                    │
│  │     • current session   │                                    │
│  │     • loaded skills     │                                    │
│  └───────────┬─────────────┘                                    │
│              │                                                   │
│              ▼                                                   │
│  ┌─────────────────────────┐                                    │
│  │  2. LLM Processing      │                                    │
│  │     (if enabled)        │                                    │
│  │                         │                                    │
│  │  System Prompt:         │                                    │
│  │  "You are a CCOS agent  │                                    │
│  │   with capabilities:    │                                    │
│  │   - ccos.chat.transform │                                    │
│  │   - ccos.skill.*        │                                    │
│  │   - ccos.secrets.*      │                                    │
│  │                         │                                    │
│  │   User said: {...}      │                                    │
│  │                         │                                    │
│  │   Plan actions in JSON" │                                    │
│  └───────────┬─────────────┘                                    │
│              │                                                   │
│              ▼                                                   │
│  ┌─────────────────────────┐                                    │
│  │  3. Plan Execution      │                                    │
│  │                         │                                    │
│  │  AgentPlan: {           │                                    │
│  │    understanding,       │                                    │
│  │    actions: [           │                                    │
│  │      { capability_id,   │                                    │
│  │        reasoning,       │                                    │
│  │        inputs }         │                                    │
│  │    ],                   │                                    │
│  │    response             │                                    │
│  │  }                      │                                    │
│  └───────────┬─────────────┘                                    │
│              │                                                   │
│              ▼                                                   │
│  ┌─────────────────────────┐                                    │
│  │  4. Capability Loop     │                                    │
│  │                         │                                    │
│  │  for action in plan:    │                                    │
│  │    result = execute(    │                                    │
│  │      action.cap_id,     │                                    │
│  │      action.inputs      │                                    │
│  │    )                    │                                    │
│  │    handle result/error  │                                    │
│  │  end                    │                                    │
│  └───────────┬─────────────┘                                    │
│              │                                                   │
│              ▼                                                   │
│  ┌─────────────────────────┐                                    │
│  │  5. Response            │                                    │
│  │     Send plan.response  │                                    │
│  │     back to user        │                                    │
│  └─────────────────────────┘                                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4. Gateway-Agent Communication Protocol

### 4.1 Authentication

All Agent → Gateway requests must include:

```http
X-Agent-Token: <cryptographically_secure_random_token>
```

The Gateway validates:
1. Token exists and is valid base64
2. Token matches an active session
3. Token has not expired (if TTL configured)
4. Session status is Active (not Suspended/Expired)

### 4.2 Event Polling Protocol

```
Request:
  GET /chat/events/:session_id
  Headers:
    X-Agent-Token: <token>

Response (200 OK):
  {
    "messages": [
      {
        "id": "msg_123",
        "content": "Hello agent!",
        "sender": "user@example.com",
        "timestamp": "2026-02-01T10:00:00Z"
      }
    ],
    "has_more": false
  }

Response (401 Unauthorized):
  Invalid or missing token

Response (404 Not Found):
  Session not found
```

### 4.3 Capability Execution Protocol

```
Request:
  POST /chat/execute
  Headers:
    X-Agent-Token: <token>
    Content-Type: application/json
  Body:
    {
      "capability_id": "ccos.skill.load",
      "inputs": {
        "url": "https://example.com/skill.md",
        "session_id": "sess_abc123",
        "run_id": "run_001",
        "step_id": "step_001"
      },
      "session_id": "sess_abc123"
    }

Response (200 OK - Success):
  {
    "success": true,
    "result": {
      "skill_id": "example_skill",
      "capabilities": [...]
    },
    "error": null
  }

Response (200 OK - Capability Error):
  {
    "success": false,
    "result": null,
    "error": "Skill definition not found at URL"
  }

Response (401 Unauthorized):
  Token invalid or expired

Response (403 Forbidden):
  Capability requires approval not yet granted
```

### 4.4 Capability Discovery Protocol

```
Request:
  GET /chat/capabilities
  Headers:
    X-Agent-Token: <token>

Response:
  {
    "capabilities": [
      {
        "id": "ccos.secrets.set",
        "name": "Store Secret",
        "description": "Store a secret value securely",
        "version": "1.0.0"
      },
      {
        "id": "ccos.chat.egress.prepare_outbound",
        "name": "Prepare Outbound Message",
        "description": "Prepare a message for egress",
        "version": "0.1.0"
      }
    ]
  }
```

---

## 5. Relationship to CCOS

### 5.1 CCOS Architectural Context

The Gateway-Agent architecture is an **extension** of CCOS's core "Separation of Powers" design:

```
┌─────────────────────────────────────────────────────────────────┐
│                    CCOS Core Architecture                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                 CCOS Governance Kernel                     │  │
│  │  (Pre-existing, unchanged by Gateway-Agent)              │  │
│  ├──────────────────────────────────────────────────────────┤  │
│  │  • Capability Marketplace                                │  │
│  │  • Causal Chain (immutable audit)                        │  │
│  │  • Approval Queue                                        │  │
│  │  • Policy Engine                                         │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              ▲                                   │
│                              │                                   │
│  ┌───────────────────────────┼──────────────────────────────┐  │
│  │           Gateway         │        (New Component)       │  │
│  │  ┌──────────────────────┐ │ ┌──────────────────────────┐ │  │
│  │  │ Session Management   │─┘ │ Agent Spawner            │ │  │
│  │  │ Token Auth           │   │ HTTP Router              │ │  │
│  │  │ Message Routing      │   │                          │ │  │
│  │  └──────────────────────┘   └──────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                              │ X-Agent-Token                     │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                      Agent                                 │  │
│  │  ┌──────────────────────┐   ┌──────────────────────────┐ │  │
│  │  │ Event Polling        │   │ LLM Integration          │ │  │
│  │  │ Skill Loader         │   │ Capability Client        │ │  │
│  │  └──────────────────────┘   └──────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  The Gateway-Agent system wraps the CCOS core, adding:          │
│  • Session-based isolation                                       │
│  • Token-based authentication                                    │
│  • Agent process management                                      │
│  • Multi-agent support                                           │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 CCOS Capabilities Used

The Gateway leverages these existing CCOS capabilities:

| Capability | Used For | Gateway Role |
|------------|----------|--------------|
| `ccos.secrets.set` | Storing agent credentials | Forwards agent requests |
| *(no direct secrets-get capability)* | Secret injection | Secrets are resolved from `SecretStore` (`.ccos/secrets.toml`) and injected at execution time by the capability marketplace/executors; secret values are not returned to the agent |
| `ccos.memory.store` | Persisting onboarding state | Forwards agent requests |
| `ccos.memory.get` | Retrieving onboarding state | Forwards agent requests |
| `ccos.chat.egress.prepare_outbound` | Preparing messages for send | Direct use |
| `ccos.chat.transform.summarize_message` | Transforming messages | Direct use |
| `ccos.skill.load` | Loading skill definitions | Forwards agent requests |
| `ccos.approval.request_human_action` | Requesting human intervention | Forwards agent requests |
| `ccos.network.http-fetch` | Making HTTP requests | Internal use for skills |

### 5.3 Causal Chain Integration

Every Gateway-Agent interaction is recorded in the Causal Chain:

```json
{
  "event": "capability_call",
  "capability": "ccos.skill.load",
  "invoked_by": "agent",
  "session_id": "sess_abc123",
  "run_id": "run_001",
  "step_id": "step_001",
  "timestamp": 1706784000000,
  "inputs_hash": "sha256:abc...",
  "result": "success",
  "audit_trail": [
    { "event": "token_validated", "session_id": "sess_abc123" },
    { "event": "capability_executed", "capability": "ccos.skill.load" },
    { "event": "result_recorded", "success": true }
  ]
}
```

### 5.4 Approval Queue Integration

The Gateway routes approval-related capabilities:

```
Agent: ccos.approval.request_human_action
  ↓
Gateway: Validates token, forwards to CapabilityMarketplace
  ↓
CapabilityMarketplace: Creates approval in queue
  ↓
Human: Sees approval in UI/CLI, grants approval
  ↓
Agent: Next poll checks approval status
  ↓
Gateway: Returns approval status to agent
  ↓
Agent: Continues with approved action
```

---

## 6. Security Architecture

### 6.1 Threat Model

| Threat | Mitigation |
|--------|------------|
| Agent escapes jail | Agent runs with no filesystem/network access; all calls through Gateway |
| Token theft | Tokens are session-bound and can be revoked; short TTLs |
| Token replay | Tokens tied to specific session IDs; cross-session use rejected |
| Man-in-the-middle | Use TLS for Gateway-Agent communication (production) |
| Agent executes unauthorized capability | Gateway validates all capability calls against registry |
| Secret exfiltration | Secrets never returned to agent; injected by Gateway at execution |
| Agent impersonation | Cryptographically secure token generation |
| Session fixation | New session = new token; old tokens invalidated |

### 6.2 Security Boundaries

```
┌─────────────────────────────────────────────────────────────────┐
│                     Security Boundaries                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  Layer 0: Host OS                                                │
│    ├─ Gateway runs as privileged process                        │
│    ├─ Agent runs as unprivileged, sandboxed process             │
│    └─ Network policies block agent egress except to Gateway     │
│                                                                  │
│  Layer 1: Authentication                                         │
│    ├─ Token validation on every request                         │
│    ├─ Session status verification                               │
│    └─ Capability existence and permission checks                │
│                                                                  │
│  Layer 2: Authorization                                          │
│    ├─ Capability-level approvals                                │
│    ├─ Per-operation governance                                  │
│    └─ Resource budget enforcement                               │
│                                                                  │
│  Layer 3: Audit                                                  │
│    ├─ All actions recorded in Causal Chain                      │
│    ├─ Secret values redacted in logs                            │
│    └─ Tamper-evident audit trail                                │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### 6.3 Secrets Management

**Principle**: Agents never handle secrets directly

```
Scenario: Agent wants to call Moltbook API

1. Agent executes an operation (e.g. `ccos.skill.execute` or a derived per-skill capability)
2. Gateway executes the operation via the capability marketplace
3. Executor resolves required secrets from `SecretStore` (`.ccos/secrets.toml`) with env-var fallback
4. Gateway injects secrets into HTTP request headers (e.g. `Authorization: Bearer ...`)
5. Gateway makes the actual HTTP call
6. Gateway returns response to Agent (secrets never exposed)
```

---

## 7. Operational Features

### 7.1 Multi-Agent Support

The Gateway supports multiple concurrent agents:

```
Session A ─┐
           ├─► Gateway ──► Capability Marketplace ──► External APIs
Session B ─┘              (isolated by session_id)

Each session has:
  • Unique session_id
  • Unique auth_token
  • Isolated inbox
  • Separate agent process
  • Independent lifecycle
```

### 7.2 Graceful Degradation

| Failure Mode | Behavior |
|--------------|----------|
| Agent crashes | Gateway marks session as Suspended; can respawn |
| Gateway restarts | Sessions lost (in-memory), agents must reconnect with new sessions |
| Network partition | Agent retries with exponential backoff |
| Capability timeout | Returns error to agent; agent can retry |
| LLM unavailable | Falls back to simple response mode |

### 7.3 Observability

**Metrics**:
- Active sessions count
- Messages processed per second
- Capability execution latency
- Token validation failures
- Agent spawn success rate

**Logs**:
- Session lifecycle events
- Capability execution (secret values redacted)
- Token validation attempts
- Approval queue operations

**Tracing**:
- Cross-component request tracing (session_id, run_id, step_id)
- Causal chain links

---

## 8. Implementation Details

### 8.1 Key Source Files

| Component | File | Purpose |
|-----------|------|---------|
| Gateway | `ccos/src/chat/gateway.rs` | HTTP router, session management |
| Gateway | `ccos/src/chat/session.rs` | SessionRegistry implementation |
| Gateway | `ccos/src/chat/spawner.rs` | AgentSpawner trait and implementations |
| Agent | `ccos/src/bin/ccos_agent.rs` | Agent binary and main loop |
| Agent | `ccos/src/chat/agent_llm.rs` | LLM client integration |
| Shared | `ccos/src/chat/connector.rs` | Webhook handling |
| Shared | `ccos/src/chat/mod.rs` | Common types and helpers |

### 8.2 Binaries

| Binary | Purpose | Usage |
|--------|---------|-------|
| `ccos-chat-gateway` | Gateway server | `cargo run --bin ccos-chat-gateway` |
| `ccos-agent` | Agent runtime | `cargo run --bin ccos-agent -- --token X --session-id Y` |
| `mock-moltbook` | Test server | `cargo run --bin mock-moltbook` |

### 8.3 Testing

**Automated Demo Script**: `run_demo_moltbook.sh`
- Builds all binaries automatically
- Starts Mock Moltbook server (port 8765)
- Starts Chat Gateway (ports 8822/8833)
- Sends trigger message to create session
- Shows live logs for 15 seconds
- Demonstrates full Gateway-Agent-Moltbook flow
- Best for: Quick verification of the architecture

**Integration Test Script**: `test_integration.sh`
- Provides step-by-step manual testing instructions
- Explains each component's role
- Best for: Understanding how to test manually

**Unit Tests**:
- Session registry operations
- Token validation
- Capability execution mocking
- LLM client parsing
- Config file loading

---

## 9. Future Extensions

### 9.1 Planned Enhancements

| Feature | Description | Priority |
|---------|-------------|----------|
| WebSocket Support | Real-time push instead of polling | High |
| Persistent Sessions | Store sessions in database for Gateway restart recovery | Medium |
| Agent Clustering | Multiple agents per session for load balancing | Low |
| Plugin System | Custom capability providers loaded at runtime | Low |
| OAuth Integration | Built-in OAuth flow for skill onboarding | Medium |

### 9.2 Research Areas

- **Federated Gateways**: Multiple Gateway instances coordinating
- **Agent Sandboxing**: Firecracker/VM-based agent isolation
- **Skill Registry**: Centralized skill discovery and versioning
- **Human-in-the-Loop UI**: Web interface for approvals

---

## 10. Glossary

| Term | Definition |
|------|------------|
| Gateway (Sheriff) | High-privilege component that manages sessions and executes capabilities |
| Agent (Deputy) | Low-privilege AI process that plans and requests capability execution |
| Session | Isolated context for a single agent, with unique ID and token |
| X-Agent-Token | Authentication header for Agent → Gateway requests |
| Capability | Governed function exposed through Capability Marketplace |
| Causal Chain | Immutable audit log of all system actions |
| Skill | External API integration definition with operations and onboarding |
| Onboarding | Multi-step process to activate a skill (may require human actions) |

---

## 11. References

- [Skill Onboarding Specification](spec-skill-onboarding.md)
- [Skill Interpreter Specification](spec-skill-interpreter.md)
- [CCOS Chat Mode Security Contract](../ccos/specs/037-chat-mode-security-contract.md)
- [CCOS Capability System](../ccos/specs/030-capability-system-architecture.md)
- [CCOS Causal Chain](../ccos/specs/003-causal-chain.md)

---

*End of Document*
