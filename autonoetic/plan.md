# Autonoetic Project Plan

## Phase 1: Specification & Design ✅

### Completed Documents

- [x] **[concepts.md](concepts.md)** — Core philosophy & vision
- [x] **[architecture_modules.md](architecture_modules.md)** — System topology, modules, security
  - [x] Gateway (Vault, Route Engine, LLM Driver Abstraction)
  - [x] Agent Orchestrator (SKILL.md Manifest, Ed25519 Signing)
  - [x] Sandbox Workers (Bubblewrap, Docker, MicroVM, WASM)
  - [x] Two-Tier Memory Architecture
  - [x] Artifact Store, Runtime Lock, Cognitive Capsule
  - [x] Capability-Based Security Model
  - [x] Agent Loop Stability (Loop Guard, Session Repair)
  - [x] Security Hardening (Taint Tracking, SSRF, Scanner)
- [x] **[protocols.md](protocols.md)** — Communication standards
  - [x] Internal JSON-RPC (Gateway ↔ Agent)
  - [x] Sandbox Bridge (Agent ↔ Sandbox via Gateway)
  - [x] Artifact Handles & Capsule Transport
  - [x] MCP Client/Server integration
  - [x] OFP Federation (wire-compatible with OpenFang)
  - [x] Causal Chain with hash-chain audit trail
- [x] **[sandbox_sdk.md](sandbox_sdk.md)** — SDK API surface
- [x] **[data_models.md](data_models.md)** — Concrete schemas
- [x] **[cli_interface.md](cli_interface.md)** — CLI surface

### Key Design Decisions
- **Agent = SKILL.md**: Unified format (YAML frontmatter + Markdown body)
- **OFP adopted natively**: Wire-compatible with OpenFang for cross-ecosystem federation
- **MCP yes, external A2A no**: Standard tool ecosystem via MCP; internal agent-to-agent routing via JSON-RPC message bus
- **Textual State**: Tier 1 memory is plain text files; Tier 2 is indexed Gateway substrate
- **Artifacts are content-addressed**: Large data and shared outputs move by immutable handles
- **Skills declare effects**: `metadata.declared_effects` narrows what a Skill may do beyond Agent-level capabilities

---

## Phase 2: Implementation Scaffolding ✅

- [x] Rust workspace: `autonoetic-gateway`, `autonoetic-types`, `autonoetic-ofp`, `autonoetic-mcp`, `autonoetic-sdk`
- [x] `autonoetic` CLI binary (clap-based)
- [x] Stub Gateway: config loading, Agent directory scanning, `runtime.lock` resolution
- [x] Stub Sandbox: bwrap process spawning with stdio piping

---

## Phase 3: Core Engine ✅

- [x] Gateway event loop (tokio async runtime)
- [x] JSON-RPC message router
- [x] Policy engine (capability validation)
- [x] Artifact store (content-addressed cache + handle resolution)
- [x] Causal Chain logger (append-only .jsonl with hash-chain linkage)
- [x] Vault (env var ingestion, secret zeroization)

---

## Phase 4: Agent Runtime ✅

- [x] SKILL.md parser (YAML frontmatter + Markdown body)
- [x] Agent lifecycle (Wake → Context Assembly → Reasoning → Hibernate)
- [x] Tier 1 memory (state/ directory read/write)
- [x] Tier 2 memory (SQLite + vector embeddings)
- [x] Skill declared effects + artifact dependency enforcement
- [x] Loop Guard & Session Repair
- [x] Ed25519 Manifest Signing

---

## Phase 4.5: LLM Drivers ✅

Complete, modular LLM provider system — thin by design (≤250 LOC per driver), no code duplication, all credential/endpoint resolution centralised.

### Architecture (`llm/`)

| File | Role |
|------|------|
| `mod.rs` | Shared types: `CompletionRequest`, `CompletionResponse`, `Message`, `ToolDefinition`, `ToolCall`, `StreamEvent`, `LlmDriver` trait |
| `provider.rs` | `ResolvedProvider` + `ProviderCapabilities` flags (`supports_streaming`, `supports_tool_stream_deltas`, `supports_system_top_level`, `supports_usage_in_stream`). Drivers never read env vars — all credential/URL resolution happens here |
| `openai.rs` | OpenAI-compatible driver: handles all providers below through one code path |
| `anthropic.rs` | Anthropic Messages API: `tool_use`/`tool_result` content blocks, system at top-level, SSE streaming |
| `gemini.rs` | Google Gemini: `functionDeclarations`/`functionCall`/`functionResponse`, `systemInstruction` |

### Provider support (all via `provider::resolve()`)

**Distinct wire formats:**
- `anthropic` / `claude` — Anthropic Messages API
- `gemini` / `google` — Gemini generateContent API

**OpenAI-compatible (single driver, table-driven URLs):**
- `openai`, `openrouter`, `groq`, `together`, `deepseek`, `mistral`, `fireworks`
- `perplexity`, `cohere`, `ai21`, `cerebras`, `sambanova`, `huggingface`
- `xai`, `replicate`, `moonshot`/`kimi`, `qwen`/`dashscope`
- `ollama`, `vllm`, `lmstudio` (local, no key required)

### Features proven in tests

- Tool call round-trips (ToolUse → ToolResult for all 3 wire formats)
- SSE streaming with delta accumulation (text + tool input deltas)
- Exponential backoff retries (3 attempts, 429/529)
- `max_completion_tokens` for GPT-5/o-series reasoning models
- **22 golden tests** (unit, no network) — request & response shapes for all 3 providers
- **2 live integration tests** vs OpenRouter `google/gemini-3-flash-preview`:
  - Simple completion → `"4 total."` ✅
  - Tool call → `get_weather(city="Paris")` ✅

### Agent lifecycle upgrade
`AgentExecutor` upgraded from a stub to a **real agentic loop**:
- Maintains `history: Vec<Message>` across LLM turns
- Dispatches `ToolUse` turns → executes → pushes `ToolResult` → resets `LoopGuard`
- Hibernates cleanly on `EndTurn`

---

## Phase 5: Networking & Federation ✅

- [x] OFP TCP listener + HMAC handshake
- [x] PeerRegistry + extension negotiation
- [x] MCP Client (stdio/SSE transport, tool discovery)
- [x] MCP Server (expose agents as tools)

### Completion notes

- OFP runtime now starts from normal gateway boot path with strict required federation identity env vars.
- Peer lifecycle is wired into `PeerRegistry` (connected/disconnected) and validated with socket-level integration tests.
- MCP servers are persisted in `mcp_servers.json`, probed on registration and gateway startup, and exposed in `gateway status`/`gateway status --json`.
- Agent runtime now dispatches `mcp_<server>_<tool>` tool calls via loaded MCP registry clients.
- Coverage includes `ofp_integration` and `mcp_integration` test suites in `autonoetic-gateway/tests/`.

---

## Phase 6: SDK & Sandbox ✅

- [x] Python `autonoetic_sdk` package
- [x] JavaScript/TypeScript `autonoetic_sdk` package
- [x] Bubblewrap sandbox driver
- [x] Docker sandbox driver
- [x] MicroVM sandbox driver (Firecracker)

### Completion notes

- Added Python SDK package at `autonoetic-sdk/python/` with `init()` and full API namespaces (`memory`, `state`, `secrets`, `message`, `files`, `artifacts`, `events`, `tasks`) over Unix socket JSON-RPC.
- Added TypeScript SDK package at `autonoetic-sdk/typescript/` with equivalent `init()` and API namespaces over Unix socket JSON-RPC.
- Upgraded gateway sandbox runtime to a multi-driver model (`bubblewrap`, `docker`, `microvm`) with strict env-driven configuration for Docker image and Firecracker config path.
- Added unit coverage for sandbox driver parsing and command construction (`sandbox::tests::*`).
- Wired `autonoetic agent run` to the real lifecycle executor so manifest-declared sandbox runtime is now exercised from CLI.
- Added capability-gated `sandbox.exec` tool dispatch with policy enforcement and hard-fail behavior for unknown tools.
- Added thin dependency bootstrap support (Python/Node), with deterministic defaults read from `runtime.lock` (`dependencies`) and explicit tool-call overrides.

---

## Phase 7: Polish & Release 🔜

- [ ] End-to-end integration tests
- [ ] Documentation site
- [ ] Example agents (researcher, coder, auditor)
- [ ] AgentSkills.io marketplace publishing
- [ ] Open-source release (choose license)
