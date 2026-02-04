# CCOS Gateway-Agent Feature Reference

**Version**: 1.0  
**Last Updated**: 2026-02-04

---

## Table of Contents

1. [Gateway Features](#gateway-features)
2. [Agent Features](#agent-features)
3. [Communication Protocols](#communication-protocols)
4. [Security Features](#security-features)
5. [Skill System](#skill-system)
6. [Capability Registry](#capability-registry)
7. [Configuration Options](#configuration-options)

---

## Gateway Features

### Session Management

| Feature | Description | API Endpoint |
|---------|-------------|--------------|
| **Session Creation** | Creates isolated context for each agent | Webhook trigger |
| **Token Generation** | Cryptographically secure random tokens | Automatic on session create |
| **Session Validation** | Validates token + session_id pairs | All endpoints |
| **Session Lifecycle** | Active → Suspended → Expired | Automatic/CLI |
| **Inbox Management** | Per-session message queues | `/chat/events/:id` |
| **Session Query** | Get session info and status | `/chat/session/:id` |

### Agent Spawning

| Feature | Description | Implementation |
|---------|-------------|----------------|
| **LogOnlySpawner** | Logs spawn intent (testing) | `spawner.rs` |
| **ProcessSpawner** | Spawns actual agent processes | `spawner.rs` |
| **PID Tracking** | Tracks spawned agent process IDs | Session registry |
| **Auto-spawn** | Automatically spawns agents on session create | `gateway.rs` |

### HTTP API Endpoints

| Method | Endpoint | Auth Required | Description |
|--------|----------|---------------|-------------|
| GET | `/chat/health` | No | Health check and queue depth |
| GET | `/chat/inbox` | No | Debug: view global inbox |
| POST | `/chat/execute` | Yes (X-Agent-Token) | Execute capability |
| GET | `/chat/capabilities` | Yes | List available capabilities |
| GET | `/chat/events/:session_id` | Yes | Poll for new messages |
| GET | `/chat/session/:session_id` | Optional | Get session information |
| GET | `/chat/audit` | No | View audit trail (supports `?session_id=...` and/or `?run_id=...`) |
| POST | `/chat/send` | No | Send outbound message |
| POST | `/chat/run` | Yes (X-Agent-Token) | Create a Run (goal) for a session; enqueues a kickoff system message |
| GET | `/chat/run/:run_id` | Yes (X-Agent-Token) | Get a Run by id (session-bound) |
| GET | `/chat/run?session_id=...` | Yes (X-Agent-Token) | List Runs for a session (latest first) |
| GET | `/chat/run/:run_id/actions` | Yes (X-Agent-Token) | List causal-chain actions for a Run (latest first) |
| POST | `/chat/run/:run_id/transition` | Yes (X-Agent-Token) | Transition Run state (optionally update budget; budget window resets when transitioning to Active) |
| POST | `/chat/run/:run_id/cancel` | Yes (X-Agent-Token) | Cancel a Run |

### Authentication & Authorization

| Feature | Description |
|---------|-------------|
| **X-Agent-Token Header** | Primary authentication mechanism |
| **Session-Bound Tokens** | Tokens only valid for their session |
| **Token Validation** | Every request validated against session registry |
| **Status Checking** | Validates session is Active, not Suspended/Expired |
| **Capability Checking** | Validates capability exists and is registered |
| **Approval Enforcement** | Blocks capabilities requiring unapproved approvals |

### Runs (Autonomy Core)

| Feature | Description |
|---------|-------------|
| **Run Lifecycle** | Runs are session-bound goals with a state machine (Active / PausedApproval / PausedExternalEvent / Done / Failed / Cancelled). |
| **Single Active Run** | `POST /chat/run` returns `409` if the session already has an active run (prevents competing orchestration loops). |
| **Kickoff Message** | Creating a run enqueues a synthetic system message into the session inbox: `Run started (<run_id>). Goal: ...`. This starts execution without requiring a user chat message. |
| **Inbound Correlation** | Inbound chat messages correlate to the active run; otherwise to the latest paused run (PausedExternalEvent auto-resumes to Active; PausedApproval correlates without resuming). |
| **Run Budget Gate** | `/chat/execute` enforces run state + budget: non-chat capabilities are refused while paused/terminal; if budget exceeded, the run transitions to PausedApproval and the call is refused. Chat egress/transform capabilities remain allowed so the system can communicate pause/next-steps. |
| **Completion Predicates** | Gateway enforces `completion_predicate=never` (Done transition rejected) and supports `capability_succeeded:<capability_id>` (Done allowed only if that capability succeeded within the run). |

### Audit & Logging

| Feature | Description |
|---------|-------------|
| **Causal Chain Integration** | All actions recorded in immutable audit trail |
| **Session Attribution** | Every action linked to session_id, run_id, step_id |
| **Secret Redaction** | Secret values never logged or returned |
| **Structured Logging** | `tracing` framework with configurable levels |
| **Request Logging** | All HTTP requests logged with metadata |

---

## Agent Features

### Event Processing

| Feature | Description | Configuration |
|---------|-------------|---------------|
| **Event Polling** | Continuous polling of Gateway events | `--poll-interval-ms` (default: 1000) |
| **Message History** | Maintains history of processed messages | In-memory |
| **Event Processing** | Handles each message through LLM or simple mode | Automatic |
| **Acknowledgment** | Events atomically drained from inbox | Gateway-managed |

### LLM Integration

| Feature | Description | Options |
|---------|-------------|---------|
| **OpenAI Support** | GPT-3.5, GPT-4, etc. | `--llm-provider openai` |
| **Anthropic Support** | Claude models | `--llm-provider anthropic` |
| **Local Mode** | Echo/ testing mode | `--llm-provider local` (default) |
| **Custom Models** | Configurable model names | `--llm-model gpt-4` |
| **Context Building** | Builds context from message history | Automatic |
| **Plan Generation** | LLM generates AgentPlan with actions | JSON format |
| **Action Execution** | Executes planned capabilities sequentially | Automatic |
| **Fallback Mode** | Falls back to simple response on LLM error | Automatic |

### LLM Planning Format

The LLM returns plans in this JSON structure:

```json
{
  "understanding": "brief description of user intent",
  "actions": [
    {
      "capability_id": "ccos.skill.load",
      "reasoning": "why this capability",
      "inputs": { "url": "https://example.com/skill.md" }
    }
  ],
  "response": "natural language response to user"
}
```

### Capability Client

| Feature | Description |
|---------|-------------|
| **Capability Execution** | POST to Gateway `/chat/execute` |
| **Capability Discovery** | GET from Gateway `/chat/capabilities` |
| **Error Handling** | Handles success/failure responses |
| **Input Augmentation** | Auto-adds session_id, run_id, step_id to inputs |
| **Retry Logic** | Configurable retry on transient failures |
| **Plan Safety Checks** | Skips executing malformed actions (e.g. refuses to call `ccos.skill.execute` if `operation` is missing) to avoid failing runs on planner mistakes. |

### Run Awareness

| Feature | Description |
|---------|-------------|
| **Run ID Propagation** | Agent can be started with `--run-id` / `CCOS_RUN_ID`; outbound capability calls include `run_id` for correlation. |
| **Run State Transitions** | Agent calls the Gateway `/chat/run/:run_id/transition` endpoint to pause/resume (e.g. on budget exhaustion) and to mark basic outcomes (Done/Failed) when predicates allow it. |
| **No Demo Coupling** | Agent summaries and next-steps messaging are generic; demo-specific onboarding text is handled by the demo skill and/or external system. |

### Skill System

| Feature | Description |
|---------|-------------|
| **Skill Loading** | Fetches skill definitions from URLs |
| **Markdown Parsing** | Parses `###` headers and code blocks |
| **YAML Support** | Structured skill definitions |
| **Onboarding Tracking** | Maintains onboarding state |
| **State Machine** | NOT_LOADED → LOADED → NEEDS_SETUP → OPERATIONAL |
| **Human Action Requests** | Creates approval requests for human steps |
| **Secret Management** | Secrets are stored in the Gateway’s `SecretStore` (`.ccos/secrets.toml`) and injected at execution time; the agent does not receive secret values. The agent can optionally persist per-skill bearer tokens for reuse across restarts. |
| **Authorization Injection (Bearer)** | When a per-skill bearer token is known (from onboarding or SecretStore), the agent automatically adds `Authorization: Bearer ...` to skill calls (both direct per-skill capabilities and the `ccos.skill.execute` wrapper). |
| **Skill Load Guardrails** | `ccos.skill.load` rejects non-skill-looking URLs by default; use `force=true` to override (e.g. to avoid accidentally treating arbitrary `x.com/...` links as skills). |

---

## Communication Protocols

### Authentication Protocol

All Agent → Gateway requests:

```http
X-Agent-Token: <base64_encoded_token>
```

Validation:
1. Header must be present
2. Token must decode successfully
3. Token must match session's stored token
4. Session must be in Active status

### Event Polling Protocol

**Request**:
```http
GET /chat/events/:session_id
X-Agent-Token: <token>
```

**Success Response (200)**:
```json
{
  "messages": [
    {
      "id": "msg_123",
      "content": "Hello!",
      "sender": "user@example.com",
      "timestamp": "2026-02-01T10:00:00Z"
    }
  ],
  "has_more": false
}
```

**Unauthorized (401)**:
- Missing token header
- Invalid token format
- Token doesn't match session

**Not Found (404)**:
- Session doesn't exist

### Capability Execution Protocol

**Request**:
```http
POST /chat/execute
Content-Type: application/json
X-Agent-Token: <token>

{
  "capability_id": "ccos.secrets.set",
  "inputs": {
    "key": "API_KEY",
    "value": "secret_value",
    "session_id": "sess_123",
    "run_id": "run_001",
    "step_id": "step_001"
  },
  "session_id": "sess_123"
}
```

**Success Response (200)**:
```json
{
  "success": true,
  "result": { "stored": true },
  "error": null
}
```

**Failure Response (200)**:
```json
{
  "success": false,
  "result": null,
  "error": "Approval required for SecretWrite"
}
```

### Capability Discovery Protocol

**Request**:
```http
GET /chat/capabilities
X-Agent-Token: <token>
```

**Response (200)**:
```json
{
  "capabilities": [
    {
      "id": "ccos.secrets.set",
      "name": "Store Secret",
      "description": "Store a secret value securely",
      "version": "1.0.0"
    }
  ]
}
```

---

## Security Features

### Jailed Execution Model

| Feature | Gateway | Agent |
|---------|---------|-------|
| Filesystem Access | Yes (managed) | No |
| Network Egress | Yes (controlled) | Limited (talks only to Gateway in the default deployment) |
| Secret Access | Yes (injected) | No |
| Direct API Calls | Yes (via capabilities) | No |
| Persistent State | Yes | Optional: per-skill bearer token persistence to `.ccos/secrets.toml` when enabled |

### Token Security

| Property | Implementation |
|----------|----------------|
| **Generation** | Cryptographically secure random (32+ bytes) |
| **Binding** | Tied to specific session_id |
| **Transmission** | HTTP header only |
| **Storage** | Never logged, memory-only |
| **TTL** | Session lifetime (configurable) |
| **Revocation** | Session deletion invalidates token |

### Audit Trail

Every action recorded in Causal Chain:

```json
{
  "action_id": "uuid",
  "session_id": "sess_123",
  "plan_id": "plan_001",
  "intent_id": "intent_001",
  "action_type": "CapabilityCall",
  "function_name": "ccos.secrets.set",
  "timestamp": 1706784000000,
  "metadata": {
    "event_type": "capability_call",
    "inputs_hash": "sha256:...",
    "approval_id": "app_123"
  }
}
```

### Approval System Integration

| Category | Description | Approval Required |
|----------|-------------|-------------------|
| **SecretWrite** | Storing new secrets | Yes |
| **HumanActionRequest** | Human intervention | Yes (human must act) |
| **ChatPolicyException** | Override policy | Yes |
| **EffectApproval** | Side-effect operations | Configurable |

---

## Skill System

### Skill Definition Format

Skills can be defined in:

**Markdown** (`skill.md`):
```markdown
# Skill Name

### Operation Name
```bash
curl -X POST https://api.example.com/endpoint
```

## Operations
- name: operation_name
  endpoint: https://api.example.com/endpoint
```

**YAML** (`skill.yaml`):
```yaml
skill:
  name: example_skill
  version: "1.0.0"
  operations:
    - name: search
      method: POST
      endpoint: https://api.example.com/search
      auth:
        type: bearer
        env_var: API_KEY
```

### Onboarding Steps

Skills can declare multi-step onboarding:

```yaml
onboarding:
  required: true
  steps:
    - id: register
      type: api_call
      operation: register-agent
      store:
        - from: response.agent_id
          to: memory:skill.agent_id
        - from: response.secret
          to: secret:SKILL_SECRET
          requires_approval: true
    
    - id: human-verification
      type: human_action
      action:
        type: tweet_verification
        title: "Verify Ownership"
        instructions: "Post this tweet: ..."
        required_response:
          tweet_url: url
```

### Onboarding State Machine

```
NOT_LOADED → LOADED → NEEDS_SETUP → PENDING_HUMAN_ACTION → OPERATIONAL
                ↓
            READY (no onboarding needed)
```

---

## Capability Registry

### Core Capabilities

| Capability ID | Purpose | Approval |
|--------------|---------|----------|
| `ccos.secrets.set` | Store secret value in SecretStore (approval recorded) | SecretWrite |
| `ccos.memory.store` | Persist state | No |
| `ccos.memory.get` | Retrieve state | No |
| `ccos.approval.request_human_action` | Request human action | Creates approval |
| `ccos.approval.complete` | Complete human action | No |
| `ccos.skill.load` | Load skill definition | No |
| `ccos.skill.execute` | Execute skill operation | Per-skill |

### Chat Capabilities

| Capability ID | Purpose | Data Classification |
|--------------|---------|---------------------|
| `ccos.chat.transform.summarize_message` | Summarize quarantined message | pii.redacted |
| `ccos.chat.transform.extract_entities` | Extract entities from message | pii.redacted |
| `ccos.chat.transform.redact_message` | Redact sensitive content | pii.redacted |
| `ccos.chat.transform.verify_redaction` | Verify redaction for public | public (if approved) |
| `ccos.chat.egress.prepare_outbound` | Prepare message for egress | Depends on input |

### Capability Properties

All capabilities have:
- **ID**: Unique identifier
- **Name**: Human-readable name
- **Description**: Purpose and behavior
- **Version**: Semantic version
- **Input Schema**: Expected input structure
- **Output Schema**: Output structure
- **Effects**: Side effects (network, filesystem, etc.)
- **Security Level**: low/medium/high
- **Approval Requirements**: When approval is needed

---

## Configuration Options

### Gateway Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `CCOS_GATEWAY_BIND_ADDR` | `0.0.0.0:8080` | Gateway listen address |
| `CCOS_APPROVALS_DIR` | `./approvals` | Approval storage directory |
| `CCOS_QUARANTINE_DIR` | `./quarantine` | Quarantine storage directory |
| `CCOS_QUARANTINE_KEY` | Required | Encryption key for quarantine |
| `CCOS_QUARANTINE_TTL_SECONDS` | `86400` | Default quarantine TTL |
| `CCOS_LOG_LEVEL` | `info` | Log level (trace/debug/info/warn/error) |

### Agent CLI Arguments

| Argument | Default | Description |
|----------|---------|-------------|
| `--config-path` | None | Path to TOML configuration file |
| `--token` | Required | Authentication token from Gateway |
| `--session-id` | Required | Session identifier |
| `--gateway-url` | `http://localhost:8080` | Gateway URL |
| `--poll-interval-ms` | `1000` | Polling interval in milliseconds |
| `--run-id` | None | Correlate all capability calls to a Run (for autonomy workflows) |
| `--enable-llm` | `false` | Enable LLM processing |
| `--llm-provider` | `local` | LLM provider (openai/anthropic/local) |
| `--llm-api-key` | None | API key for LLM provider |
| `--llm-model` | `gpt-3.5-turbo` | Model name |
| `--persist-skill-secrets` | `false` | Persist discovered per-skill bearer tokens to SecretStore (`.ccos/secrets.toml`) for reuse across restarts |

### Agent Configuration File

The agent supports loading configuration from TOML files (same format as other CCOS components):

**Config Loading Priority**:
1. CLI arguments (highest priority)
2. Environment variables
3. Config file values (lowest priority)

**Example with Config**:
```bash
ccos-agent \
  --config-path config/agent_config.toml \
  --token "..." \
  --session-id "..."
```

**Config File Values Applied**:
- `llm_profiles.default` → Sets `--llm-provider` and `--llm-model` (if CLI uses defaults)
- `capabilities.llm.enabled` → Sets `--enable-llm` (if not explicitly disabled via CLI)
- `llm_profiles.profiles[].api_key_env` → Loads API key from specified environment variable

**Common Use Cases**:
- **Development**: Use `--config-path config/agent_config.toml` with pre-configured profiles
- **Production**: Use CLI args for dynamic values, config file for defaults
- **Testing**: Use `--llm-provider local` (no config needed) or stub profile

### Agent Environment Variables

All CLI arguments can also be set via environment:

| Variable | Maps to |
|----------|---------|
| `CCOS_AGENT_CONFIG_PATH` | `--config-path` |
| `CCOS_AGENT_TOKEN` | `--token` |
| `CCOS_SESSION_ID` | `--session-id` |
| `CCOS_GATEWAY_URL` | `--gateway-url` |
| `CCOS_RUN_ID` | `--run-id` |
| `CCOS_AGENT_ENABLE_LLM` | `--enable-llm` |
| `CCOS_LLM_PROVIDER` | `--llm-provider` |
| `CCOS_LLM_API_KEY` | `--llm-api-key` |
| `CCOS_LLM_MODEL` | `--llm-model` |
| `CCOS_AGENT_PERSIST_SKILL_SECRETS` | `--persist-skill-secrets` |

---

## Metrics & Observability

### Gateway Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `active_sessions` | Gauge | Number of active sessions |
| `session_creations_total` | Counter | Total sessions created |
| `session_terminations_total` | Counter | Total sessions terminated |
| `messages_processed_total` | Counter | Total messages processed |
| `capability_executions_total` | Counter | Total capability executions |
| `capability_execution_duration` | Histogram | Capability execution time |
| `token_validation_failures_total` | Counter | Failed token validations |

### Agent Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `polls_total` | Counter | Total event polls |
| `messages_received_total` | Counter | Total messages received |
| `llm_requests_total` | Counter | Total LLM API calls |
| `llm_request_duration` | Histogram | LLM request latency |
| `capabilities_executed_total` | Counter | Total capability executions |
| `capability_execution_errors_total` | Counter | Failed capability executions |

### Logging

Structured logging with `tracing`:

```
[Gateway] Session created: sess_abc123, token: [REDACTED]
[Gateway] Executing capability ccos.secrets.set for session sess_abc123
[Agent] Polling events for session sess_abc123
[Agent] Processing message with LLM: "Please summarize..."
[Agent] Executing action 1/3: ccos.chat.transform.summarize_message
```

---

## Troubleshooting Guide

### Gateway Issues

| Symptom | Cause | Solution |
|---------|-------|----------|
| "bind error" | Port 8080 in use | Change `CCOS_GATEWAY_BIND_ADDR` |
| "Failed to init approval storage" | Permissions | Check directory permissions |
| "Token validation failed" | Wrong token | Check Gateway logs for correct token |

### Agent Issues

| Symptom | Cause | Solution |
|---------|-------|----------|
| "Authentication failed" | Invalid token | Verify token from Gateway logs |
| "Session not found" | Wrong session_id | Check session exists in Gateway |
| "LLM processing failed" | Invalid API key | Verify `CCOS_LLM_API_KEY` |
| "Capability execution failed" | Unregistered capability | Check capability_id spelling |
| "No events received" | Polling too fast | Increase `--poll-interval-ms` |

### Common Problems

**Agent can't connect to Gateway**:
```bash
# Test connectivity
curl http://localhost:8080/chat/health

# Check Gateway is listening
netstat -tlnp | grep 8080
```

**Token rejected**:
```bash
# View Gateway logs to get correct token
grep "Session token" gateway.log

# Verify token format (should be base64)
echo "<token>" | base64 -d
```

**Capability not found**:
```bash
# List available capabilities
curl -H "X-Agent-Token: <token>" http://localhost:8080/chat/capabilities
```

---

## API Reference Summary

### Gateway Endpoints

| Endpoint | Auth | Request | Response |
|----------|------|---------|----------|
| `GET /chat/health` | No | - | `{ ok: bool, queue_depth: int }` |
| `GET /chat/capabilities` | Token | - | `{ capabilities: [...] }` |
| `GET /chat/events/:id` | Token | - | `{ messages: [...], has_more: bool }` |
| `GET /chat/session/:id` | Optional | - | Session info |
| `POST /chat/execute` | Token | Capability request | Execution result |
| `POST /chat/send` | No | Send request | Send response |
| `GET /chat/audit` | No | Query params | Audit events |

### Data Types

**CapabilityRequest**:
```json
{
  "capability_id": "string",
  "inputs": "object",
  "session_id": "string"
}
```

**CapabilityResponse**:
```json
{
  "success": "boolean",
  "result": "object|null",
  "error": "string|null"
}
```

**ChatMessage**:
```json
{
  "id": "string",
  "content": "string",
  "sender": "string",
  "timestamp": "string (ISO 8601)"
}
```

**SessionInfo**:
```json
{
  "session_id": "string",
  "status": "string",
  "created_at": "string",
  "last_activity": "string",
  "inbox_size": "number",
  "agent_pid": "number|null"
}
```

---

*End of Feature Reference*
