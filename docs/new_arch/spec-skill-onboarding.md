# Skill Onboarding System Specification

**Status**: Partially Implemented (Core capabilities exist; UX + stronger governance semantics still evolving)  
**Version**: 0.3  
**Date**: 2026-02-04  
**Authors**: CCOS Team

---

## 1. Overview

### 1.1 Problem Statement

Many real-world skills (API integrations) require multi-step onboarding workflows before they become operational. These workflows often include:

- **Agent-autonomous steps**: Registration, API key retrieval, configuration
- **Human-in-the-loop steps**: Email verification, OAuth consent, tweet verification, payment setup
- **State persistence**: Tracking onboarding progress across sessions
- **Credential management**: Securely storing and injecting API keys/tokens

The current skill system handles simple skills (load → execute) but lacks primitives for complex onboarding flows.

### 1.2 Design Principles

1. **Data-driven, not hardcoded**: The agent should reason about onboarding requirements from skill metadata, not follow hardcoded workflows
2. **Governance umbrella**: All operations go through CCOS governance (approval queue, audit trail, capability restrictions)
3. **Human-in-the-loop**: Human actions are requested via the approval system with clear instructions
4. **Resumable state**: Onboarding state persists in agent memory, allowing resumption after human actions
5. **Security-first**: Secrets never logged, credentials injected at execution time, sensitive operations require approval

---

## 2. Real-World Example: Moltbook

Moltbook demonstrates a complex onboarding flow that our system must support:

### 2.1 Moltbook Skill Structure

```markdown
# Moltbook Agent Skill

## Operations

### Register Agent
POST https://moltbook.com/api/register-agent
Body: { "name": "...", "model": "...", "created_by": "..." }
Returns: { "agent_id": "...", "secret": "..." }

### Human Claim (Tweet Verification)
POST https://moltbook.com/api/human-claim
Headers: Authorization: Bearer {agent_secret}
Body: { "human_x_username": "@user" }
Returns: { "verification_tweet_text": "I'm verifying..." }

### Verify Human Claim
POST https://moltbook.com/api/verify-human-claim
Headers: Authorization: Bearer {agent_secret}
Body: { "tweet_url": "https://x.com/..." }

### Setup Heartbeat
POST https://moltbook.com/api/setup-heartbeat
Headers: Authorization: Bearer {agent_secret}
Body: { "prompt_id": "...", "interval_hours": 24 }

### Operational Commands
- POST /api/post-to-feed (requires verified agent)
- POST /api/reply (requires verified agent)
- GET /api/feed (requires verified agent)
```

### 2.2 Onboarding Phases

```
Phase 1: Registration (Agent Autonomous)
├── Call POST /api/register-agent
├── Store agent_id in memory
└── Store secret in SecretStore (requires approval)

Phase 2: Human Claim (Human-in-the-Loop)
├── Call POST /api/human-claim
├── Present verification tweet text to human
├── Request human action: "Post this tweet from your X account"
└── Wait for human to provide tweet_url

Phase 3: Verification (Agent Autonomous)
├── Call POST /api/verify-human-claim with tweet_url
└── Update memory: onboarding_complete = true

Phase 4: Operational Setup (Agent Autonomous)
├── Call POST /api/setup-heartbeat
└── Agent is now fully operational
```

---

## 3. CCOS Capability Architecture

### 3.1 Required Capabilities

| Capability ID | Purpose | Governance |
|--------------|---------|------------|
| `ccos.skill.load` | Load skill definition | None |
| `ccos.skill.execute` | Execute skill operation | Per-operation approval |
| `ccos.secrets.set` | Store credential | Requires approval |
| `ccos.memory.store` | Persist onboarding state | None |
| `ccos.memory.get` | Retrieve onboarding state | None |
| `ccos.approval.request_human_action` | Request human intervention | Creates approval |
| `ccos.approval.complete` | Mark human action done | Resolves approval |

### 3.2 Capability Specifications

#### 3.2.1 `ccos.secrets.set`

Store a secret value in the SecretStore under governance.

```yaml
capability_id: ccos.secrets.set
inputs:
  key: string      # Secret name (e.g., "MOLTBOOK_SECRET")
  value: string    # Secret value
  scope: string    # "skill" | "global" (default: "skill")
outputs:
  success: boolean
  approval_id: string  # If approval required
governance:
  requires_approval: true
  category: SecretWrite
  audit: true
```

**Security Notes**:
- Value is never logged or returned in responses
- Stored in local `.ccos/secrets.toml` with restrictive filesystem permissions (0600 on Unix). This file is currently plain TOML (not encrypted at rest).
- Scope "skill" prefixes key with `skill_id:` for isolation (implementation detail).
- Current caveat: `ccos.secrets.set` records an approval request and persists the value immediately; the approval is an audit/governance hook, not a hard gate yet.

#### 3.2.2 `ccos.memory.store`

Persist key-value data in agent working memory.

```yaml
capability_id: ccos.memory.store
inputs:
  key: string      # Memory key (e.g., "moltbook.agent_id")
  value: any       # Value to store (JSON-serializable)
  ttl: number      # Optional TTL in seconds
outputs:
  success: boolean
governance:
  requires_approval: false
  audit: true
```

#### 3.2.3 `ccos.memory.get`

Retrieve value from agent working memory.

```yaml
capability_id: ccos.memory.get
inputs:
  key: string      # Memory key
  default: any     # Optional default if not found
outputs:
  value: any
  found: boolean
governance:
  requires_approval: false
  audit: false
```

#### 3.2.4 `ccos.approval.request_human_action`

Request human intervention with clear instructions.

```yaml
capability_id: ccos.approval.request_human_action
inputs:
  action_type: string       # "tweet_verification" | "email_verification" | "oauth_consent" | "custom"
  title: string             # Short title for approval UI
  instructions: string      # Detailed human instructions (markdown)
  required_response: object # Schema for expected human response
    fields:
      - name: string
        type: string
        description: string
  timeout_hours: number     # Optional timeout (default: 24h)
outputs:
  approval_id: string       # ID to poll/wait for completion
  status: string            # "pending"
governance:
  requires_approval: false  # Creates approval, doesn't require one
  category: HumanActionRequest
  audit: true
```

**Example Usage**:

```json
{
  "action_type": "tweet_verification",
  "title": "Verify Moltbook Agent Ownership",
  "instructions": "Please post the following tweet from your X account:\n\n> I'm verifying my AI agent moltbook_agent_123 on @moltbook. Code: ABC123\n\nThen paste the tweet URL below.",
  "required_response": {
    "fields": [
      { "name": "tweet_url", "type": "url", "description": "URL of the verification tweet" }
    ]
  },
  "timeout_hours": 48
}
```

#### 3.2.5 `ccos.approval.complete`

Mark a human action approval as complete with response data.

```yaml
capability_id: ccos.approval.complete
inputs:
  approval_id: string       # ID from request_human_action
  response: object          # Human-provided response data
outputs:
  success: boolean
  validation_errors: array  # If response doesn't match schema
governance:
  requires_approval: false
  audit: true
```

---

## 4. Onboarding State Machine

### 4.1 State Model

```
┌─────────────────────────────────────────────────────────────────┐
│                     Skill Onboarding States                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────┐    load     ┌───────────┐                         │
│  │ NOT_LOADED│ ─────────► │  LOADED   │                         │
│  └──────────┘             └─────┬─────┘                         │
│                                 │                                │
│                          check requirements                      │
│                                 │                                │
│              ┌──────────────────┼──────────────────┐            │
│              ▼                  ▼                  ▼            │
│       ┌─────────────┐   ┌─────────────┐   ┌─────────────┐      │
│       │  READY      │   │ NEEDS_SETUP │   │NEEDS_SECRETS│      │
│       │(no onboard) │   │             │   │             │      │
│       └─────────────┘   └──────┬──────┘   └──────┬──────┘      │
│                                │                  │             │
│                         run setup steps    request secrets      │
│                                │                  │             │
│                                ▼                  ▼             │
│                        ┌─────────────┐   ┌─────────────┐       │
│                        │ PENDING_    │   │ PENDING_    │       │
│                        │ HUMAN_ACTION│   │ SECRET_     │       │
│                        │             │   │ APPROVAL    │       │
│                        └──────┬──────┘   └──────┬──────┘       │
│                               │                  │              │
│                         human completes    approval granted     │
│                               │                  │              │
│                               ▼                  ▼              │
│                        ┌─────────────────────────────┐         │
│                        │       OPERATIONAL           │         │
│                        │  (all setup complete)       │         │
│                        └─────────────────────────────┘         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 Memory Schema

```json
{
  "skill:{skill_name}:state": {
    "status": "PENDING_HUMAN_ACTION",
    "current_step": 2,
    "total_steps": 4,
    "completed_steps": ["register"],
    "pending_approval_id": "abc-123",
    "data": {
      "agent_id": "moltbook_agent_456",
      "registered_at": "2026-02-01T10:00:00Z"
    },
    "last_updated": "2026-02-01T10:05:00Z"
  }
}
```

---

## 5. Implementation Plan

### Phase 1: Core Primitives (Priority: High)

**Goal**: Implement the foundational capabilities that enable multi-step onboarding.

| Task | Description | Effort | Status |
|------|-------------|--------|--------|
| 1.1 | Implement `ccos.secrets.set` capability | 2h | ✅ Done (see caveat in §3.2.1) |
| 1.2 | Implement `ccos.memory.store` capability | 1h | ✅ Done |
| 1.3 | Implement `ccos.memory.get` capability | 1h | ✅ Done |
| 1.4 | Add SecretWrite approval category | 1h | ✅ Done |
| 1.5 | Unit tests for new capabilities | 2h | ✅ Done |

**Files to modify**:
- `ccos/src/skills/mapper.rs` - Add capability handlers
- `ccos/src/ccos/approval.rs` - Add SecretWrite category
- `ccos/src/runtime/capability_marketplace.rs` - Register capabilities

### Phase 2: Human-in-the-Loop (Priority: High)

**Goal**: Enable requesting and completing human actions through the approval system.

| Task | Description | Effort | Status |
|------|-------------|--------|--------|
| 2.1 | Implement `ccos.approval.request_human_action` | 3h | ✅ Done |
| 2.2 | Implement `ccos.approval.complete` | 2h | ✅ Done |
| 2.3 | Add HumanActionRequest approval category | 1h | ✅ Done |
| 2.4 | Response validation against schema | 2h | 🔶 Partial (schema support exists; tighten validation + UX) |
| 2.5 | Integration tests with mock human | 2h | ⏳ Pending |

**Files to modify**:
- `ccos/src/ccos/approval.rs` - New approval type and handlers
- `ccos/src/skills/mapper.rs` - Expose as capabilities
- `ccos/src/mcp_server.rs` - MCP tool exposure

### Phase 3: Onboarding State Machine (Priority: Medium)

**Goal**: Track onboarding progress and enable resumption.

| Task | Description | Effort |
|------|-------------|--------|
| 3.1 | Design onboarding metadata in skill format | 2h |
| 3.2 | Implement state machine transitions | 3h |
| 3.3 | Auto-detect onboarding requirements from skill | 2h |
| 3.4 | Resume onboarding after restart | 2h |
| 3.5 | Skill status reporting via MCP | 1h |

**Files to modify**:
- `ccos/src/skills/mod.rs` - Add onboarding module
- `ccos/src/skills/onboarding.rs` - New state machine
- `ccos/src/skills/parser.rs` - Parse onboarding metadata

### Phase 4: Agent Planning Integration (Priority: Medium)

**Goal**: Enable the planner to reason about onboarding steps.

| Task | Description | Effort |
|------|-------------|--------|
| 4.1 | Expose skill status to planner context | 2h |
| 4.2 | Generate onboarding sub-goals from skill metadata | 3h |
| 4.3 | Handle "waiting for human" in plan execution | 2h |
| 4.4 | Onboarding completion triggers | 1h |

**Files to modify**:
- `ccos/src/ccos/planner.rs` - Skill-aware planning
- `ccos/src/ccos/orchestrator.rs` - Handle waiting states

### Phase 5: Skill.md Parser Enhancement (Priority: Low)

**Goal**: Extract structured operations from markdown skill definitions.

| Task | Description | Effort |
|------|-------------|--------|
| 5.1 | Parse curl commands from markdown | 3h |
| 5.2 | Extract operation metadata (auth, params) | 2h |
| 5.3 | Generate structured skill from markdown | 2h |
| 5.4 | Handle variations in markdown format | 2h |

**Files to modify**:
- `ccos/src/skills/parser.rs` - Markdown parsing
- `ccos/src/skills/skill.rs` - Unified skill model

---

## 6. Skill Metadata Extensions

### 6.1 Onboarding Section

Extend skill YAML format to declare onboarding requirements:

```yaml
name: moltbook
description: Moltbook social platform for AI agents

onboarding:
  required: true
  steps:
    - id: register
      type: api_call
      operation: register-agent
      store:
        - from: response.agent_id
          to: memory:moltbook.agent_id
        - from: response.secret
          to: secret:MOLTBOOK_SECRET
          requires_approval: true
    
    - id: human-claim
      type: api_call
      operation: human-claim
      depends_on: [register]
      
    - id: tweet-verification
      type: human_action
      depends_on: [human-claim]
      action:
        type: tweet_verification
        title: Verify Moltbook Ownership
        instructions: |
          Post this tweet from your X account:
          
          > {{verification_tweet_text}}
          
          Then provide the tweet URL.
        required_response:
          tweet_url: url
      
    - id: verify
      type: api_call
      operation: verify-human-claim
      depends_on: [tweet-verification]
      params:
        tweet_url: "{{human_response.tweet_url}}"
    
    - id: setup-heartbeat
      type: api_call
      operation: setup-heartbeat
      depends_on: [verify]

operations:
  register-agent:
    method: POST
    path: /api/register-agent
    # ...
```

### 6.2 Conditional Operations

```yaml
operations:
  post-to-feed:
    method: POST
    path: /api/post-to-feed
    requires:
      onboarding_complete: true
      # Agent will see error if onboarding not done
```

---

## 7. Security Considerations

### 7.1 Governance Controls

| Operation | Approval Required | Audit Logged | Notes |
|-----------|------------------|--------------|-------|
| Load skill | No | Yes | Read-only |
| Execute operation | Configurable | Yes | Per-skill/operation |
| Store secret | Yes | Yes (no value) | SecretWrite category |
| Read secret | No | Yes (no value) | Injected, not returned |
| Request human action | No | Yes | Creates approval |
| Complete human action | No | Yes | Human provides data |
| Store to memory | No | Yes | State persistence |
| Read from memory | No | No | Frequent access |

### 7.2 Secret Handling

1. **Never log secret values**: All logging must redact secret content
2. **At rest**: SecretStore uses a local `.ccos/secrets.toml` file with restrictive filesystem permissions (not encrypted at rest yet)
3. **Scoped access**: Skill-scoped secrets isolated from other skills
4. **Approval audit trail**: Who approved storing which secret (not the value)
5. **Injection only**: Secrets injected into headers, never returned to agent

**Optional convenience (agent-side)**:
- The agent can be started with `--persist-skill-secrets` / `CCOS_AGENT_PERSIST_SKILL_SECRETS=1` to persist discovered per-skill bearer tokens to `SecretStore` (`.ccos/secrets.toml`) and reuse them across restarts. This is primarily to support multi-step onboarding flows where later operations require `Authorization: Bearer ...`.

### 7.3 Human Action Safety

1. **Clear instructions**: Human must understand what they're being asked
2. **No auto-posting**: Agent cannot post to social media without explicit human action
3. **Timeout limits**: Human actions expire after configurable timeout
4. **Verification display**: Show human what data will be sent after their action

---

## 8. Example: Full Moltbook Onboarding Flow

### 8.1 Agent Perspective

```
User: "Set up my Moltbook agent"

Agent reasoning:
1. Load moltbook skill
2. Check onboarding status → NOT_STARTED
3. Execute step 1: register-agent
   - Call POST /api/register-agent
   - Store agent_id in memory
   - Request approval to store secret
   - [BLOCKED: Waiting for secret approval]

--- Human approves secret storage ---

Agent resumes:
4. Secret stored, continue to step 2
5. Execute step 2: human-claim
   - Call POST /api/human-claim
   - Get verification_tweet_text
6. Execute step 3: request human action
   - Create approval with tweet instructions
   - [BLOCKED: Waiting for human action]

--- Human posts tweet, provides URL ---

Agent resumes:
7. Execute step 4: verify-human-claim
   - Call POST /api/verify-human-claim with tweet_url
8. Execute step 5: setup-heartbeat
   - Call POST /api/setup-heartbeat
9. Onboarding complete!

Agent: "Your Moltbook agent is now set up and operational."
```

### 8.2 MCP Tool Calls

```json
// Step 1: Load skill
{"method": "tools/call", "params": {"name": "ccos.skill.load", "arguments": {"url": "https://moltbook.com/skill.md"}}}

// Step 2: Check status
{"method": "tools/call", "params": {"name": "ccos.memory.get", "arguments": {"key": "skill:moltbook:state"}}}

// Step 3: Execute register
{"method": "tools/call", "params": {"name": "ccos.skill.execute", "arguments": {"skill": "moltbook", "operation": "register-agent", "params": {"name": "my-agent", "model": "claude-3"}}}}

// Step 4: Store secret (creates approval)
{"method": "tools/call", "params": {"name": "ccos.secrets.set", "arguments": {"key": "MOLTBOOK_SECRET", "value": "...", "scope": "skill"}}}

// Step 5: Request human action
{"method": "tools/call", "params": {"name": "ccos.approval.request_human_action", "arguments": {"action_type": "tweet_verification", "title": "Verify Moltbook", "instructions": "Post: I'm verifying..."}}}

// ... human completes, agent continues ...
```

---

## 9. Success Criteria

### 9.1 Functional Requirements

- [ ] Agent can complete Moltbook onboarding end-to-end
- [ ] Onboarding state persists across agent restarts
- [ ] Human actions clearly communicated via approval system
- [ ] Secrets stored securely with approval audit trail
- [ ] Agent can resume after human action completion

### 9.2 Non-Functional Requirements

- [ ] No hardcoded skill-specific logic in CCOS core
- [ ] All operations governed by approval system
- [ ] Secret values never appear in logs or responses
- [ ] Onboarding progress visible in MCP responses
- [ ] Timeout handling for abandoned onboarding flows

---

## 10. Future Considerations

### 10.1 OAuth Integration

For skills requiring OAuth, extend human action types:

```yaml
- id: oauth-consent
  type: human_action
  action:
    type: oauth_consent
    provider: github
    scopes: [repo, user:email]
    callback_url: http://localhost:8080/oauth/callback
```

### 10.2 Batch Onboarding

For organizations setting up multiple agents:

```yaml
onboarding:
  mode: batch
  max_concurrent: 5
  shared_secrets: [ORG_API_KEY]
```

### 10.3 Onboarding Templates

Reusable onboarding patterns:

```yaml
onboarding:
  template: oauth2-with-refresh
  params:
    provider: google
    scopes: [calendar.readonly]
```

---

## Appendix A: Approval Categories

| Category | Description | Auto-Approve Option |
|----------|-------------|---------------------|
| EffectApproval | Side-effect operations (HTTP POST, etc.) | Per-skill config |
| SecretRequired | Skill needs secret not yet configured | No |
| SecretWrite | Storing new secret value | No |
| HumanActionRequest | Requires human intervention | No |
| BudgetExceeded | Operation exceeds cost budget | No |

---

## Appendix B: Related Documents

- [Skill Interpreter Spec](spec-skill-interpreter.md)
- [CCOS Governance Kernel](../ccos/specs/governance.md)
- [Approval System](../ccos/specs/approval.md)
- [Secret Management](../ccos/specs/secrets.md)
