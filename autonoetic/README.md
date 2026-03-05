# Autonoetic

Autonoetic is a Rust-first runtime for autonomous, self-evolving agents with durable memory, portable identity, and reproducible execution.

The project is currently incubated in the `ccos-ng/` directory inside the broader `ccos` repository, but it is intended to become a standalone project once the architecture and implementation stabilize.

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

More advanced features like full marketplace workflows, hermetic capsule replay, advanced memory substrate, and richer federation polish are deferred until the base runtime is proven.

## Positioning

Autonoetic takes inspiration from systems like OpenFang, but it is differentiated by:

- text-first working memory
- stronger emphasis on self-evolution
- portable runtime closures
- explicit artifact and capsule semantics
- a sharper separation between logical agent identity and execution runtime

## Status

Specification and design are substantially defined. The next phase is implementation scaffolding around the constrained MVP in [`plan.md`](plan.md).
