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
- [`docs/agent_routing_and_roles.md`](docs/agent_routing_and_roles.md): default lead-agent routing, specialist roles, delegation, and evolution-oriented role selection
- [`docs/quickstart-planner-specialist-chat.md`](docs/quickstart-planner-specialist-chat.md): end-to-end CLI quickstart for implicit planner routing and specialist delegation
- [`docs/remote-agents-http-api.md`](docs/remote-agents-http-api.md): HTTP API for remote agents, SDK HTTP transport, authentication
- [`protocols.md`](protocols.md): JSON-RPC methods, artifact/capsule transport, OFP interoperability
- [`data_models.md`](data_models.md): `SKILL.md`, `runtime.lock`, artifact handles, capsule manifest
- [`sandbox_sdk.md`](sandbox_sdk.md): `autonoetic_sdk` API surface
- [`cli_interface.md`](cli_interface.md): `autonoetic` CLI shape
- [`plan.md`](plan.md): implementation roadmap and MVP boundary

## Reference Agent Bundles

Reference agent bundles are grouped under [`agents/`](agents/):

- `agents/lead/` for front-door/orchestration agents
- `agents/specialists/` for hand roles
- `agents/evolution/` for builder and evolution flows

Current bundles:

- Lead: `agents/lead/planner.default/`
- Specialists:
  - `agents/specialists/researcher.default/`
  - `agents/specialists/architect.default/`
  - `agents/specialists/coder.default/`
  - `agents/specialists/debugger.default/`
  - `agents/specialists/evaluator.default/`
  - `agents/specialists/auditor.default/`
- Evolution:
  - `agents/evolution/specialized_builder.default/`
  - `agents/evolution/evolution-steward.default/`
  - `agents/evolution/memory-curator.default/`

To install these into your active runtime directory, run:

`autonoetic agent bootstrap [--from <path>] [--overwrite]`

## Current Direction

The current MVP is intentionally narrow:

- Gateway daemon with JSON-RPC and HTTP REST APIs
- `SKILL.md` and `runtime.lock` parsing
- Bubblewrap sandboxing
- text-first Tier 1 memory
- minimal Tier 2 recall
- content-addressed artifact handles
- hash-chain causal logging
- OFP federation listener with HMAC handshake + extension negotiation
- MCP client/server plumbing (registry, discovery, and agent exposure)

## HTTP Content API (for Remote Agents)

The gateway exposes REST endpoints for remote agents to access content. See [docs/remote-agents-http-api.md](docs/remote-agents-http-api.md) for full documentation.

**Quick start for remote agents:**

```python
# On the remote agent machine
export AUTONOETIC_HTTP_URL="http://gateway-host:8080"
export AUTONOETIC_SHARED_SECRET="your-secret"

from autonoetic_sdk import Client
sdk = Client()  # Automatically uses HTTP mode
sdk.files.write("main.py", "print(42)")
```

**Endpoints:**

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/content/write` | Write content (UTF-8 or base64) |
| GET | `/api/content/read/{session_id}/{name}` | Read content by name/handle |
| POST | `/api/content/read` | Read content (body params) |
| POST | `/api/content/persist` | Mark content as persistent |
| GET | `/api/content/names?session_id=X` | List content names with handles |

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

For planner/specialist implicit routing through CLI chat, see:

- [`docs/quickstart-planner-specialist-chat.md`](docs/quickstart-planner-specialist-chat.md)

From `autonoetic/`:

```bash
bash examples/quickstart/run.sh
```

By default it initializes an agent in an isolated `/tmp` workspace and runs a real headless call against OpenRouter `google/gemini-3-flash-preview` (requires `OPENROUTER_API_KEY`). You can also run `smoke` mode for local interactive startup/exit without a remote model call.

Each run appends lifecycle/tool events to the agent causal trace at `agents/<agent_id>/history/causal_chain.jsonl`.
Set `AUTONOETIC_EVIDENCE_MODE=full` to additionally capture redacted full evidence payloads with `evidence_ref` pointers in causal entries.
Causal entries expose top-level `session_id`, `turn_id`, and `event_seq` fields for multi-run/multi-turn introspection, plus `entry_hash` / `prev_hash` linkage for chain integrity.
You can inspect traces with:
- `autonoetic trace sessions [--agent <agent_id>] [--json]`
- `autonoetic trace show <session_id> [--agent <agent_id>] [--json]`
- `autonoetic trace event <log_id> [--agent <agent_id>] [--json]`
- `autonoetic trace follow <session_id> [--agent <agent_id>] [--json]`
- `autonoetic trace fork <session_id> [--message <text>] [--at-turn N] [--interactive]`
- `autonoetic trace history <session_id> [--agent <agent_id>] [--json]`

## Specialized Builder Example

A second runnable example now lives at [`examples/specialized_builder`](examples/specialized_builder/README.md).

From `autonoetic/`:

```bash
bash examples/specialized_builder/run.sh
```

This example promotes the builder flow into a real agent: you chat with a builder agent, it uses `agent.install` to create a durable child worker, and the background scheduler picks that worker up automatically. The default scripted demo sends:

```text
schedule every 20sec next fibonacci series element from previous element computed in last turn
```

and verifies that the spawned `fib_worker` runs its first scheduled tick and persists Fibonacci state/history under `agents/fib_worker/`.

## License

Autonoetic is licensed under the [Apache License 2.0](LICENSE).

This license provides explicit patent protections for users and contributors, making it suitable for both open-source and commercial use.
