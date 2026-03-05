# CCOS-NG: Sandbox SDK API Design (`ccos_sdk`)

This document defines the interface for the lightweight Python and Javascript `ccos_sdk` libraries injected into every ephemeral Sandbox created by the CCOS-NG Gateway. 

Because code executed in the Sandbox is frequently generated dynamically by Agents (varying from tiny scripts to fully structured `uv`/`npm` projects residing in an AgentSkills `scripts/` directory), the SDK must prioritize simplicity, determinism, and strong security typing over flexibility.

## 1. Zero-Trust Interaction Model

The Sandbox environment exists in zero trust.
* The script cannot directly read the Agent Manifest directory.
* The script cannot directly retrieve system variables holding secrets.
* The script cannot arbitrarily message APIs through loopback localhost ports securely.

Instead, the single interface bridging the sandbox to the outer CCOS-NG Gateway is the `ccos_sdk`.

Every SDK call ultimately maps to a synchronous IPC/Unix Socket request made back to the Gateway. The Gateway runs the Agent Policy Engine on every method invocation before answering.

## 2. Global Initialization

```python
import ccos_sdk

# The SDK automatically binds to the Gateway Unix Socket passed as 
# an environment variable ($CCOS_SOCKET_PATH) during microVM initialization.
# In a multi-file complex project, you only need to run init() once at your entry point.
sdk = ccos_sdk.init()
```

## 3. The Library Surface

### 1. The Memory API (Two-Tier)
The SDK exposes two tiers of memory access corresponding to the CCOS-NG Two-Tier Memory Architecture.

#### Tier 1 — Working State (Direct File Access)
Read/write to the immediate working directory or Manifest files of the parent Agent (if authorized).

```python
# Retrieve the parent agent's task list (task.md) as a string.
text_content = sdk.memory.read("task.md")

# Write an artifact directly to the agent's safe /state dir.
sdk.memory.write("output/parsed_data.csv", b"year,revenue\n2025,120M")

# Fetch all keys the Sandbox is allowed to access
files = sdk.memory.list_keys()
```

#### Tier 2 — Long-Term Recall (Gateway Substrate)
The Gateway maintains an indexed backend (KV store, vector DB, knowledge graph). The Agent interacts with it through the SDK, always receiving text back.

```python
# Store a fact in the Gateway's indexed long-term memory
sdk.memory.remember("user_preferences", {"format": "CSV", "timezone": "CET"})

# Recall a specific key from long-term memory
prefs = sdk.memory.recall("user_preferences")

# Semantic search across all stored knowledge (vector similarity)
results = sdk.memory.search("What format does the client prefer?")
# Returns a list of relevant text snippets ranked by relevance
```

### 2. The State API
Persists internal variables explicitly, enabling crash recovery for complex, long-running (Cold Path) scripts without polluting working text files.

```python
# Save arbitrary JSON checkpoint data
sdk.state.checkpoint({"last_page_scraped": 42})

# Retrieve the latest checkpoint upon a crashed script restarting
data = sdk.state.get_checkpoint()
```

### 3. The Secrets API (The Vault Interface)
Retrieves ephemeral credentials for use in API requests safely.

```python
try:
    token = sdk.secrets.get("GITHUB_API_TOKEN")
    # Use token in requests...
except ccos_sdk.errors.ApprovalRequiredError as e:
    # If the secret requires human approval, the SDK raises an immediate 
    # typed error. The script must cleanly exit or handle the absence of the key.
    print(f"Cannot proceed without {e.secret_name}, exiting.")
```

### 4. The Coordination & Messaging API
Allows a generated script to relay mid-execution information to other Agents using the Gateway Message Bus.

```python
# Fire-and-forget an outbound structured message
sdk.message.send(agent_id="research_agent_9", payload={"status": "parsing", "pct": 50})

# Suspend script execution indefinitely until the Agent or Human replies
# Enables "interruptible" conversational workflows.
answer = sdk.message.ask(agent_id="human_owner", question="Do you prefer CSV or JSON?")
```

### 5. File Transfers and Network I/O
If explicit network policies deny HTTP access, the Gateway handles I/O on behalf of the sandbox.

```python
# Asks the Gateway to fetch an external resource. 
# Bypasses local bwrap network isolation if explicitly permitted by the policy engine.
file_handle = sdk.files.download("https://example.com/data.tar.gz")

# Send an internal buffer to the remote user via Gateway Adapters
sdk.files.upload("results.pdf", target="human_owner")
```

### 6. The Observability Stream
Manually emit critical application-level events into the Causal Chain without relying solely on raw `stdout`.

```python
# Emits a strictly structured JSON payload directly to the immutable logger
sdk.events.emit(type="scraping_complete", data={"rows_extracted": 5000})
```

### 7. The Task Board API (Multi-Agent Coordination)
A shared task queue enabling peer-to-peer collaboration between Agents without routing through a parent.

```python
# Post a task for any available Agent to claim
sdk.tasks.post(title="Parse competitor PDF", description="Extract tables from report.pdf", assignee=None)

# Claim the next available task from the board
task = sdk.tasks.claim()

# Mark a claimed task as completed with results
sdk.tasks.complete(task_id=task["id"], result={"tables": [...]})

# List tasks filtered by status
pending = sdk.tasks.list(status="pending")
```

## 4. Security Constraints & Error Handling

To govern LLM hallucination and ensure stability, the SDK uses strict runtime exceptions tied directly to the `SKILL.md` contracts.

### Strict Schema Validation
If the Sandbox script is a dynamically generated AgentSkill, the Gateway intercepts all SDK outputs (e.g., standard output artifacts or `sdk.events.emit` payloads) and validates them against the `metadata.output_schema` defined in the `SKILL.md` frontmatter. If the script hallucinates invalid JSON types, the Gateway drops it and raises a fatal exception to trigger the LLM's Punishment loop.

### Exceptions
* `ccos_sdk.errors.PolicyViolation`: Thrown when the script attempts an action strictly prohibited by `policy.yaml` (e.g., calling `sdk.message.send("*")`).
* `ccos_sdk.errors.RateLimitExceeded`: Thrown when the script hits Gateway rate-limit governors or exceeds the `max_memory_mb` / `timeout_seconds` defined in its own `metadata.resource_limits`.
* `ccos_sdk.errors.ApprovalRequiredError`: Thrown immediately when an operation (like fetching a secret) requires an asynchronous human Approval Event. The script cannot "block" awaiting the secret; it must cleanly exit and respawn later if authorized.

### Token Governor
All strings pushed through the SDK APIs are monitored by the Gateway. If a script attempts to push 500MB of raw HTML logs into `sdk.events.emit()`, the Gateway automatically truncates the payload and flags the event in the Causal Chain to prevent upstream context-window exhaustion.
