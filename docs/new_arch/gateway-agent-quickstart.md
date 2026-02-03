# CCOS Gateway-Agent Quick Start Guide

**Prerequisites**: Rust toolchain, basic understanding of CCOS concepts

---

## 1. Overview

This guide walks you through:
1. Starting the Gateway (Sheriff)
2. Starting a test Agent (Deputy)
3. Observing the communication flow
4. Understanding the security model

**Time Required**: 10 minutes

---

## 2. Build the Components

```bash
# Build all required binaries
cargo build --release --bin ccos-chat-gateway --bin ccos-agent --bin mock-moltbook

# Verify binaries exist
ls -la target/release/ccos-* target/release/mock-*
```

---

## 3. Terminal 1: Start Mock Moltbook Server

This simulates an external API that requires skill onboarding.

```bash
./target/release/mock-moltbook
```

**Expected Output**:
```
🚀 Starting Mock Moltbook Server...
📡 Server running on http://0.0.0.0:8765

📋 Available endpoints:
   GET  /                   - Health check
   POST /api/register-agent - Register new agent
   POST /api/human-claim    - Initiate human verification
   ...
```

**Verify it's running**:
```bash
curl http://localhost:8765/
```

---

## 4. Terminal 2: Start Gateway

The Gateway is the security boundary - it manages sessions and executes capabilities.

```bash
# Create necessary directories
mkdir -p /tmp/ccos_test/approvals /tmp/ccos_test/quarantine

# Set required environment variable
export CCOS_QUARANTINE_KEY="test-key-for-development-only"

# Start the Gateway
./target/release/ccos-chat-gateway
```

**Expected Output**:
```
[Gateway] Starting Chat Gateway on 0.0.0.0:8080
[Gateway] Approval storage: /tmp/ccos_test/approvals
[Gateway] Quarantine storage: /tmp/ccos_test/quarantine
[Gateway] Session registry initialized
[Gateway] Agent spawner: LogOnlySpawner (safe for testing)
[Gateway] Server running!
```

**Verify it's running**:
```bash
curl http://localhost:8080/chat/health
```

---

## 5. Terminal 3: Simulate Webhook (Creates Session)

When a webhook arrives, the Gateway:
1. Creates a new session
2. Generates an auth token
3. Logs the session details (including the token)

```bash
curl -X POST http://localhost:8080/webhook/moltbook \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Hello from Moltbook!",
    "channel_id": "test-channel",
    "sender_id": "user@example.com"
  }'
```

**In Gateway logs (Terminal 2), you'll see**:
```
[Gateway] Created new session sess_abc123 for channel test-channel sender user@example.com
[Gateway] Session token: eyJ0b2tlbiI6InRlc3QifQ==
[Gateway] Spawned agent for session sess_abc123
```

**Copy the session ID and token** - you'll need them for the next step.

---

## 6. Terminal 4: Start the Agent

Now start the Agent with the credentials from step 5:

```bash
./target/release/ccos-agent \
  --token "eyJ0b2tlbiI6InRlc3QifQ==" \
  --session-id "sess_abc123" \
  --gateway-url "http://localhost:8080" \
  --poll-interval-ms 1000
```

**Expected Output**:
```
CCOS Agent Runtime starting...
Session ID: sess_abc123
Gateway URL: http://localhost:8080
LLM Enabled: false
Connected to Gateway successfully
Starting CCOS Agent for session sess_abc123...
```

The agent will now:
1. Poll `/chat/events/sess_abc123` every 1000ms
2. Process any messages it finds
3. Execute capabilities through the Gateway

---

## 7. Send a Message to the Agent

In a new terminal, send a message to the Gateway inbox:

```bash
curl -X POST http://localhost:8080/chat/send \
  -H "Content-Type: application/json" \
  -d '{
    "channel_id": "test-channel",
    "content": "Please summarize this message for me",
    "session_id": "sess_abc123",
    "run_id": "run_001",
    "step_id": "step_001"
  }'
```

**Observe the flow**:
1. **Gateway logs**: Receives message, adds to session inbox
2. **Agent logs**: Polls events, receives message, processes it
3. Since LLM is disabled, agent will use simple response mode

---

## 8. Enable LLM Processing (Optional)

To see the full planning loop, restart the agent with LLM enabled:

```bash
# Stop the current agent (Ctrl+C in Terminal 4)

# Restart with LLM
./target/release/ccos-agent \
  --token "eyJ0b2tlbiI6InRlc3QifQ==" \
  --session-id "sess_abc123" \
  --gateway-url "http://localhost:8080" \
  --enable-llm \
  --llm-provider "openai" \
  --llm-api-key "$OPENAI_API_KEY" \
  --llm-model "gpt-3.5-turbo"
```

Now when you send a message, the agent will:
1. Send it to OpenAI for understanding
2. Receive a plan with capability actions
3. Execute those capabilities through the Gateway
4. Return the response

---

## 9. Using Configuration Profiles (Optional)

Instead of specifying all options via CLI, you can use a configuration file:

### 9.1 Using config/agent_config.toml

The agent supports loading configuration from TOML files:

```bash
# Start agent with config file
./target/release/ccos-agent \
  --token "eyJ0b2tlbiI6InRlc3QifQ==" \
  --session-id "sess_abc123" \
  --gateway-url "http://localhost:8080" \
  --config-path "config/agent_config.toml"
```

**What the config provides**:
- LLM profiles (provider, model, API key environment variable)
- Feature flags (delegation, self-programming)
- Storage paths
- Governance policies

**Example config/agent_config.toml**:
```toml
# RTFS Agent Configuration
version = "1"
agent_id = "gateway-agent-demo"
profile = "default"
features = ["delegation"]

[capabilities.llm]
enabled = true

[llm_profiles]
default = "openrouter_free:balanced_ds_32"

[[llm_profiles.model_sets]]
name = "openrouter_free"
provider = "openrouter"
api_key_env = "OPENROUTER_API_KEY"
base_url = "https://openrouter.ai/api/v1"
default = "balanced"

  [[llm_profiles.model_sets.models]]
  name = "balanced_ds_32"
  model = "deepseek/deepseek-v3.2"
```

**CLI vs Config Precedence**:
- CLI arguments take precedence over config file values
- Use config for defaults, CLI for overrides
- The `--config-path` option is optional; agent works without it

### 9.2 Environment Variable

You can also set the config path via environment:

```bash
export CCOS_AGENT_CONFIG_PATH="config/agent_config.toml"
./target/release/ccos-agent \
  --token "..." \
  --session-id "..."
```

---

## 10. Test Capability Execution

The agent can execute capabilities through the Gateway. Let's list available capabilities:

```bash
curl -X GET http://localhost:8080/chat/capabilities \
  -H "X-Agent-Token: eyJ0b2tlbiI6InRlc3QifQ=="
```

**Response**:
```json
{
  "capabilities": [
    {
      "id": "ccos.chat.transform.summarize_message",
      "name": "Summarize Message (chat mode)",
      "description": "Read quarantined message by pointer and return pii.redacted summary.",
      "version": "0.1.0"
    },
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

## 10. Security Demonstration

### 10.1 Token Validation

Try accessing without a token:

```bash
curl http://localhost:8080/chat/events/sess_abc123
```

**Response**: `401 Unauthorized`

### 10.2 Wrong Token

Try with an invalid token:

```bash
curl http://localhost:8080/chat/events/sess_abc123 \
  -H "X-Agent-Token: invalid-token"
```

**Response**: `401 Unauthorized`

### 10.3 Cross-Session Access

The token is bound to a specific session - it cannot access other sessions:

```bash
# Create another session (send another webhook)
# Then try to access it with the first session's token
curl http://localhost:8080/chat/events/sess_other \
  -H "X-Agent-Token: eyJ0b2tlbiI6InRlc3QifQ=="
```

**Response**: `401 Unauthorized` (token doesn't match session)

---

## 11. Complete Skill Onboarding Flow

Now let's walk through a complete skill onboarding with the Mock Moltbook server:

### Step 1: Agent Loads Skill

The agent would fetch and parse the skill definition:

```bash
curl http://localhost:8765/skill.md
```

This returns the Moltbook skill with onboarding steps.

### Step 2: Agent Starts Onboarding

The agent executes onboarding capabilities through the Gateway:

1. **Register Agent**:
   ```json
   {
     "capability_id": "ccos.skill.execute",
     "inputs": {
       "skill": "moltbook",
       "operation": "register-agent",
       "params": { "name": "my-agent", "model": "claude-3" },
       "session_id": "sess_abc123",
       "run_id": "onboarding_001",
       "step_id": "step_1"
     },
     "session_id": "sess_abc123"
   }
   ```

2. **Store Secret** (requires approval):
   The secret would be stored with approval required.

3. **Human Verification**:
   The agent creates an approval request for human action.

4. **Continue After Approval**:
   Once the human posts the verification tweet, the agent continues.

### Step 3: Agent Becomes Operational

After all onboarding steps complete, the agent can post to Moltbook's feed.

---

## 12. Cleanup

Stop all services:

```bash
# Terminal 4: Stop agent (Ctrl+C)
# Terminal 2: Stop Gateway (Ctrl+C)
# Terminal 1: Stop Mock Moltbook (Ctrl+C)
```

Clean up test directories:

```bash
rm -rf /tmp/ccos_test
```

---

## 13. Architecture Summary

```
┌────────────────────────────────────────────────────────────────┐
│                      What You've Learned                        │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  1. Gateway (Sheriff)                                          │
│     • Runs on localhost:8080                                   │
│     • Manages sessions and tokens                              │
│     • Executes all capabilities                                │
│     • Maintains audit trail                                    │
│                                                                 │
│  2. Agent (Deputy)                                             │
│     • Polls Gateway for messages                               │
│     • Plans actions using LLM (optional)                       │
│     • Executes via Gateway APIs                                │
│     • Has NO direct access to secrets/network                  │
│                                                                 │
│  3. Security Model                                             │
│     • Token-based authentication                               │
│     • Session isolation                                        │
│     • Capability-level governance                              │
│     • Complete audit trail                                     │
│                                                                 │
│  4. Communication Flow                                         │
│     Agent → Gateway → CapabilityMarketplace → External APIs    │
│              ↑                                                   │
│              └── Secrets injected here (agent never sees)      │
│                                                                 │
└────────────────────────────────────────────────────────────────┘
```

---

## 14. Automated Demo Script

For a quick demonstration of the full Gateway-Agent architecture with Moltbook:

```bash
# Run the automated demo
./run_demo_moltbook.sh
```

This script will:
1. Build all required binaries
2. Start Mock Moltbook server (port 8765)
3. Start Chat Gateway (ports 8822/8833)
4. Send a trigger message to create a session
5. Display live logs showing:
   - Session creation
   - Agent spawning
   - Message processing
   - Capability execution

**What to expect**:
- Gateway creates session automatically
- Agent spawns and connects
- Agent receives the trigger message
- Agent processes with LLM (if enabled) or simple mode
- Logs show the full flow

**Logs are saved to**:
- `/tmp/gateway.log` - Gateway activity
- `/tmp/moltbook.log` - Moltbook API calls

---

## 15. Next Steps

- **Read the full architecture spec**: [CCOS Gateway-Agent Architecture](ccos-gateway-agent-architecture.md)
- **Explore skill onboarding**: [Skill Onboarding Specification](spec-skill-onboarding.md)
- **Understand skill interpretation**: [Skill Interpreter Specification](spec-skill-interpreter.md)
- **Run integration tests**: `./test_integration.sh`

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| "Gateway health check failed" | Ensure Gateway is running on port 8080 |
| "Authentication failed" | Check token matches session in Gateway logs |
| "Capability execution failed" | Check capability_id exists in registry |
| Agent not receiving messages | Check session_id matches between webhook and agent |
| LLM not working | Verify API key is set and provider is correct |

---

*End of Quick Start Guide*
