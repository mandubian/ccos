# CCOS-NG Project Plan

## Phase 1: Specification & Design ✅

### Completed Documents

- [x] **[concepts.md](file:///Users/pascavoi/workspaces/mandubian/ccos/ccos-ng/concepts.md)** — Core philosophy & vision
- [x] **[architecture_modules.md](file:///Users/pascavoi/workspaces/mandubian/ccos/ccos-ng/architecture_modules.md)** — System topology, modules, security
  - [x] Gateway (Vault, Route Engine, LLM Driver Abstraction)
  - [x] Agent Orchestrator (SKILL.md Manifest, Ed25519 Signing)
  - [x] Sandbox Workers (Bubblewrap, Docker, MicroVM, WASM)
  - [x] Two-Tier Memory Architecture
  - [x] Capability-Based Security Model
  - [x] Agent Loop Stability (Loop Guard, Session Repair)
  - [x] Security Hardening (Taint Tracking, SSRF, Scanner)
- [x] **[protocols.md](file:///Users/pascavoi/workspaces/mandubian/ccos/ccos-ng/protocols.md)** — Communication standards
  - [x] Internal JSON-RPC (Gateway ↔ Agent)
  - [x] Sandbox Bridge (Agent ↔ Sandbox via Gateway)
  - [x] MCP Client/Server integration
  - [x] OFP Federation (wire-compatible with OpenFang)
  - [x] Causal Chain with Merkle Audit Trail
- [x] **[sandbox_sdk.md](file:///Users/pascavoi/workspaces/mandubian/ccos/ccos-ng/sandbox_sdk.md)** — SDK API surface
  - [x] Tier 1 Memory (Working State files)
  - [x] Tier 2 Memory (recall, remember, search)
  - [x] Secrets, Coordination, Files, Events APIs
  - [x] Task Board API
- [x] **[data_models.md](file:///Users/pascavoi/workspaces/mandubian/ccos/ccos-ng/data_models.md)** — Concrete schemas
  - [x] Agent Manifest (SKILL.md with runtime block)
  - [x] Dynamic Skill Metadata
  - [x] Causal Chain Log Entry (.jsonl)
  - [x] OFP WireMessage
  - [x] Tier 2 Memory Object
  - [x] Task Board Entry
- [x] **[cli_interface.md](file:///Users/pascavoi/workspaces/mandubian/ccos/ccos-ng/cli_interface.md)** — CLI surface
  - [x] `ccos gateway` (start, stop, status)
  - [x] `ccos agent` (init, run, list)
  - [x] `ccos skill` (install, uninstall)
  - [x] `ccos federate` (join, list)
  - [x] `ccos mcp` (add, expose)

### Key Design Decisions
- **Agent = SKILL.md**: Unified format (YAML frontmatter + Markdown body) for both Agents and Tools
- **OFP adopted natively**: Wire-compatible with OpenFang for cross-ecosystem federation
- **MCP yes, A2A no**: Standard tool ecosystem via MCP; internal routing via JSON-RPC message bus
- **JSON Schema for UI**: Standard `input_schema` instead of proprietary settings syntax
- **Textual State**: Tier 1 memory is plain text files; Tier 2 is indexed Gateway substrate

### Research & Comparisons
- [x] OpenFang deep comparison ([openfang_comparison.md](file:///Users/pascavoi/.gemini/antigravity/brain/e123ba77-f482-4739-9123-b56efd1d43dc/openfang_comparison.md))
- [x] Gap analysis & closure ([gap_analysis.md](file:///Users/pascavoi/.gemini/antigravity/brain/e123ba77-f482-4739-9123-b56efd1d43dc/gap_analysis.md))
- [x] License review (MIT/Apache 2.0 — safe to adopt concepts)
- [x] A2A vs MCP vs OFP evaluation

---

## Phase 2: Implementation Scaffolding 🔜

- [ ] Define Rust workspace structure
  - [ ] `ccos-gateway` (core daemon)
  - [ ] `ccos-types` (shared data models, capability enums)
  - [ ] `ccos-ofp` (OFP wire protocol crate)
  - [ ] `ccos-mcp` (MCP client/server adapter)
  - [ ] `ccos-sdk` (Python/JS SDK libraries)
- [ ] Implement `ccos` CLI binary (clap-based)
- [ ] Stub Gateway: config loading, Agent directory scanning
- [ ] Stub Sandbox: bwrap process spawning with stdio piping

---

## Phase 3: Core Engine 🔜

- [ ] Gateway event loop (tokio async runtime)
- [ ] JSON-RPC message router
- [ ] Policy engine (capability validation)
- [ ] LLM Driver abstraction (Anthropic, OpenAI, Gemini)
- [ ] Causal Chain logger (append-only .jsonl with Merkle hashing)
- [ ] Vault (env var ingestion, secret zeroization)

---

## Phase 4: Agent Runtime 🔜

- [ ] SKILL.md parser (YAML frontmatter + Markdown body)
- [ ] Agent lifecycle (Wake → Context Assembly → Reasoning → Hibernate)
- [ ] Tier 1 memory (state/ directory read/write)
- [ ] Tier 2 memory (SQLite + vector embeddings)
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

- [ ] Python `ccos_sdk` package
- [ ] JavaScript/TypeScript `ccos_sdk` package
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
