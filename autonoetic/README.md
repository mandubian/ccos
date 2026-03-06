# Autonoetic

Autonoetic is a Rust-first runtime for autonomous, self-evolving agents with durable memory, portable identity, and reproducible execution.

The project is currently incubated in the `autonoetic/` directory inside the broader `ccos` repository, but it is intended to become a standalone project once the architecture and implementation stabilize.

## Why Autonoetic

The name comes from cognitive science. "Autonoetic" refers to self-aware, time-spanning cognition: not just storing facts, but relating memory, action, and future intent to a continuing self. That maps directly to the kind of agents this project aims to support:

- agents with durable working memory
- agents that can evolve their own skills
- agents that can collaborate without losing continuity
- agents that can be exported and relaunched with the same runtime closure

## Core Thesis

Autonoetic is not trying to be a generic chatbot framework or a thin LLM wrapper. It is a runtime for agents that:

- reason through text-native working state
- execute through a strict Gateway security boundary
- learn by promoting successful tactics into reusable Skills
- share large content through immutable artifact handles instead of bloated inline payloads
- remain portable through `runtime.lock` and Cognitive Capsules

## Main Concepts

- `SKILL.md`: the unified manifest for agents and skills
- `runtime.lock`: the pinned execution closure for reproducible runtime resolution
- `autonoetic_sdk`: the sandbox bridge for memory, artifacts, messaging, and secrets
- Artifact Store: a content-addressed store for binaries, datasets, outputs, and runtime dependencies
- Cognitive Capsule: a portable export containing an agent bundle plus its runtime closure

Autonoetic now accepts AgentSkills-compliant top-level `SKILL.md` frontmatter (`name`, `description`, `metadata`) and stores Autonoetic-specific runtime fields under `metadata.autonoetic`.

## Document Map

- [`concepts.md`](concepts.md): philosophy, agent model, memory model, evolution model
- [`architecture_modules.md`](architecture_modules.md): Gateway, sandbox, artifact store, capsule manager
- [`protocols.md`](protocols.md): JSON-RPC methods, artifact/capsule transport, OFP interoperability
- [`data_models.md`](data_models.md): `SKILL.md`, `runtime.lock`, artifact handles, capsule manifest
- [`sandbox_sdk.md`](sandbox_sdk.md): `autonoetic_sdk` API surface
- [`cli_interface.md`](cli_interface.md): `autonoetic` CLI shape
- [`plan.md`](plan.md): implementation roadmap and MVP boundary

## Current Direction

The current MVP is intentionally narrow:

- Gateway daemon
- `SKILL.md` and `runtime.lock` parsing
- Bubblewrap sandboxing
- text-first Tier 1 memory
- minimal Tier 2 recall
- content-addressed artifact handles
- hash-chain causal logging
- OFP federation listener with HMAC handshake + extension negotiation
- MCP client/server plumbing (registry, discovery, and agent exposure)

More advanced features like full marketplace workflows, hermetic capsule replay, advanced memory substrate, and richer federation polish are deferred until the base runtime is proven.

## Positioning

Autonoetic takes inspiration from systems like OpenFang, but it is differentiated by:

- text-first working memory
- stronger emphasis on self-evolution
- portable runtime closures
- explicit artifact and capsule semantics
- a sharper separation between logical agent identity and execution runtime

We are also actively trying to reuse the Openfang Protocol (OFP) as much as possible, as it provides a robust and well-designed foundation for agent interoperability.

## Status

Phases 1 through 6 are implemented, including OFP networking/federation, MCP integration foundations, SDK package scaffolding, and multi-driver sandbox support. Phase 7 now focuses on polish, end-to-end coverage, and release readiness as tracked in [`plan.md`](plan.md).

## Quickstart Example

A runnable smoke example now lives at [`examples/quickstart`](examples/quickstart/README.md).

From `autonoetic/`:

```bash
bash examples/quickstart/run.sh
```

It initializes an agent in an isolated `/tmp` workspace, switches the scaffolded provider to `ollama` (no API key required for the smoke flow), and starts interactive mode with a scripted `/exit` so you can validate basic behavior out of the box.
