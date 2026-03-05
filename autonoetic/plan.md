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
  - [x] Tier 1 Memory (Working State files)
  - [x] Tier 2 Memory (recall, remember, search)
  - [x] Secrets, Coordination, Files, Artifacts, Events APIs
  - [x] Task Board API
- [x] **[data_models.md](data_models.md)** — Concrete schemas
  - [x] Agent Manifest (SKILL.md with runtime block)
  - [x] Runtime Lock (`runtime.lock`)
  - [x] Dynamic Skill Metadata
  - [x] Artifact Handle
  - [x] Cognitive Capsule Manifest
  - [x] Causal Chain Log Entry (.jsonl)
  - [x] OFP WireMessage
  - [x] Tier 2 Memory Object
  - [x] Task Board Entry
- [x] **[cli_interface.md](cli_interface.md)** — CLI surface
  - [x] `autonoetic gateway` (start, stop, status)
  - [x] `autonoetic agent` (init, run, list)
  - [x] `autonoetic skill` (install, uninstall)
  - [x] `autonoetic federate` (join, list)
  - [x] `autonoetic mcp` (add, expose)

### Key Design Decisions
- **Agent = SKILL.md**: Unified format (YAML frontmatter + Markdown body) for both Agents and Tools
- **OFP adopted natively**: Wire-compatible with OpenFang for cross-ecosystem federation
- **MCP yes, external A2A no**: Standard tool ecosystem via MCP; internal agent-to-agent routing via JSON-RPC message bus
- **JSON Schema for UI**: Standard `input_schema` instead of proprietary settings syntax
- **Textual State**: Tier 1 memory is plain text files; Tier 2 is indexed Gateway substrate
- **Artifacts are content-addressed**: Large data, binaries, and shared outputs move by immutable handles
- **Skills declare effects**: `metadata.declared_effects` further narrows what a Skill may do beyond Agent-level capabilities
- **Runtime closure is portable**: `runtime.lock` pins the execution environment; a Cognitive Capsule can embed it for hermetic replay
- **Brand direction**: The standalone project name is `Autonoetic` even while incubation remains in the current `ccos-ng/` folder

### Research & Comparisons
- [x] OpenFang deep comparison ([openfang_comparison.md](file:///Users/pascavoi/.gemini/antigravity/brain/e123ba77-f482-4739-9123-b56efd1d43dc/openfang_comparison.md))
- [x] Gap analysis & closure ([gap_analysis.md](file:///Users/pascavoi/.gemini/antigravity/brain/e123ba77-f482-4739-9123-b56efd1d43dc/gap_analysis.md))
- [x] License review (MIT/Apache 2.0 — safe to adopt concepts)
- [x] A2A vs MCP vs OFP evaluation

---

## Phase 2-4 MVP Boundary

**In-scope for MVP (Phases 2-4):**
- Gateway daemon with JSON-RPC router and policy checks
- `SKILL.md` manifest parsing (frontmatter + body)
- `runtime.lock` parsing and validation
- Bubblewrap sandbox runner with `autonoetic_sdk` IPC (stdio/Unix socket)
- Tier 1 text memory (`state/`), minimal Tier 2 recall (KV + search stubs)
- Minimal content-addressed artifact store and handle-based sharing
- Hash-chain Causal Chain logger (append-only `.jsonl`)
- Loop Guard + Session Repair basics

**Explicitly deferred until after MVP:**
- Full federation polish (OFP extensions, peer resilience, TLS)
- Marketplace publishing, skill quarantine automation, Auditor Agent
- Multi-channel adapters beyond CLI/stdio
- Advanced memory substrate (vector DB, knowledge graph, canonical sessions)
- Multi-runtime sandboxes (Docker/MicroVM/WASM)
- Hermetic Capsule export/import with embedded Gateway binaries and offline replay

---

## Phase 2: Implementation Scaffolding ✅

- [x] Define Rust workspace structure
  - [x] `autonoetic-gateway` (core daemon)
  - [x] `autonoetic-types` (shared data models, capability enums)
  - [x] `autonoetic-ofp` (OFP wire protocol crate)
  - [x] `autonoetic-mcp` (MCP client/server adapter)
  - [x] `autonoetic-sdk` (Python/JS SDK libraries)
- [x] Implement `autonoetic` CLI binary (clap-based)
- [x] Stub Gateway: config loading, Agent directory scanning, `runtime.lock` resolution
- [x] Stub Sandbox: bwrap process spawning with stdio piping

---

## Phase 3: Core Engine 🔜

- [ ] Gateway event loop (tokio async runtime)
- [ ] JSON-RPC message router
- [ ] Policy engine (capability validation)
- [ ] Artifact store (content-addressed cache + handle resolution)
- [ ] LLM Driver abstraction (Anthropic, OpenAI, Gemini)
- [ ] Causal Chain logger (append-only .jsonl with hash-chain linkage)
- [ ] Vault (env var ingestion, secret zeroization)

---

## Phase 4: Agent Runtime 🔜

- [ ] SKILL.md parser (YAML frontmatter + Markdown body)
- [ ] Agent lifecycle (Wake → Context Assembly → Reasoning → Hibernate)
- [ ] Tier 1 memory (state/ directory read/write)
- [ ] Tier 2 memory (SQLite + vector embeddings)
- [ ] Skill declared effects + artifact dependency enforcement
- [ ] Loop Guard & Session Repair
- [ ] Ed25519 Manifest Signing

---

## Phase 5: Networking & Federation 🔜

- [ ] OFP TCP listener + HMAC handshake
- [ ] PeerRegistry + extension negotiation
- [ ] MCP Client (stdio/SSE transport, tool discovery)
- [ ] MCP Server (expose agents as tools)

---

## Phase 6: SDK & Sandbox 🔜

- [ ] Python `autonoetic_sdk` package
- [ ] JavaScript/TypeScript `autonoetic_sdk` package
- [ ] Bubblewrap sandbox driver
- [ ] Docker sandbox driver
- [ ] MicroVM sandbox driver (Firecracker)

---

## Phase 7: Polish & Release 🔜

- [ ] End-to-end integration tests
- [ ] Documentation site
- [ ] Example agents (researcher, coder, auditor)
- [ ] AgentSkills.io marketplace publishing
- [ ] Open-source release (choose license)
