# Content Storage And Artifact Boundary Plan

## Goal

Replace the current brittle content-sharing strategy with a two-layer model:

1. `content.write` is for collaborative working state inside a session tree
2. `artifact.build` is the mandatory boundary for anything that will be reviewed, installed, executed, or handed across trust boundaries

Core rule:

- no artifact, no review
- no artifact, no install
- no artifact, no executable promotion

This keeps the gateway dumb in the right way:

- it does not know what a planner/coder/evaluator/auditor is
- it does not infer workflows
- it only enforces generic visibility, storage, and closure rules

This aligns with [plan_extended.md](./plan_extended.md): the gateway should provide generic primitives and security boundaries, not orchestration logic.

## Why The Current Strategy Is Brittle

Today the system relies on session-scoped content plus planner-mediated forwarding:

- coder writes files
- planner receives handles/names
- planner must remember to pass the right things to evaluator/auditor

That is fragile for two separate reasons:

1. **Transport fragility**
   - long SHA handles are hard for LLMs
   - filenames are session-scoped and easy to lose across agent boundaries

2. **Trust fragility**
   - even if evaluator can see content, the planner may forget a file
   - a reviewed subset is not the same as a reviewed executable closure
   - code can depend on omitted files unless the gateway enforces a closed boundary

The real safety property we need is not “all files are visible.”
It is:

**the exact thing that gets used is the exact thing that got reviewed.**

That requires artifacts, not just better content passing.

## Target Model

## Layer 1: Session Content

`content.write` remains the primitive for creating files during collaboration.

It should become:

```json
{
  "name": "weather.py",
  "content": "print('hello')",
  "visibility": "private" | "session" | "global"
}
```

Recommended semantics:

- `private`
  - visible only to the writing session
  - scratchpads, drafts, notes, intermediate outputs
- `session`
  - visible to all agents under the same root session
  - normal collaboration outputs
- `global`
  - durable and cross-session readable
  - explicit publication / shared knowledge artifact

Session content solves collaboration and discoverability.
It does **not** define what is safe to execute.

## Layer 2: Artifacts

Introduce a generic artifact primitive:

- `artifact.build(...) -> artifact_id`

An artifact is a **closed bundle / closure** of files intended for downstream use.

Artifacts are the only units that may:

- be reviewed by evaluator/auditor
- be installed
- be executed beyond scratch use
- cross trust boundaries

Raw session content must not be directly promotable.

## Mandatory Rule

For any code-producing workflow:

1. coder writes files via `content.write`
2. coder builds an artifact from the intended deliverable set
3. evaluator/auditor review the artifact ID
4. install / run / publish consumes only the artifact ID

If no artifact exists, the gateway refuses the next stage.

## Trust Boundary

The important invariant is:

**reviewed artifact = executable closure**

Not:

- “reviewed filenames”
- “reviewed handles”
- “reviewed session content”

If a file is omitted from the artifact:

- it cannot be used at execution/install time

If code inside the artifact tries to import/open files outside the artifact:

- execution fails because the runtime boundary is closed

This is what makes the system robust even if an agent is sloppy or adversarial.

## Non-Goals

- no gateway awareness of evaluator/auditor/planner roles
- no automatic “send all coder files to evaluator” behavior
- no requirement that planner manually forward every file handle
- no compatibility layer for `content.persist`
- no assumption that file lists declared by agents are trustworthy on their own

## High-Level Design

## 1. Simplify Content Semantics

Replace:

- `content.write(name, content)`
- `content.persist(handle)`
- hierarchical parent-child manifest tricks as the main sharing story

With:

- `content.write(name, content, visibility)`
- visibility-aware `content.read(name_or_handle)`
- root-session collaboration visibility

Content handles should no longer bypass visibility rules.

## 2. Introduce Artifact As The Only Promotable Unit

Add:

- `artifact.build(inputs, entrypoints?, mode?) -> artifact_id`
- `artifact.read(artifact_id)` or `artifact.inspect(artifact_id)`

The artifact must record:

- included files
- content handles
- short aliases
- optional entrypoints
- digest of the whole artifact manifest

Later tools such as install/run/review should reference the artifact, not ad hoc session files.

## 3. Closed Execution Boundary

When the gateway executes or installs an artifact:

- only files in the artifact are mounted / available
- imports or file opens outside the artifact fail
- reviewed artifact ID is bound to what actually runs

This is the generic safety rule the gateway should enforce.

## 4. Session Sharing Still Matters

We still want easy collaboration:

- coder should not need to pass long SHA handles to evaluator
- evaluator should be able to read session-visible files naturally

So session-visible content is still useful.

But it is **not** enough for trust.
Artifacts remain mandatory for the next stage.

## Tool Surface

## `content.write`

New shape:

```json
{
  "name": "src/geocoding.py",
  "content": "...",
  "visibility": "session"
}
```

Response:

```json
{
  "ok": true,
  "handle": "sha256:...",
  "alias": "a1b2c3d4",
  "name": "src/geocoding.py",
  "visibility": "session"
}
```

## `content.read`

`content.read(name_or_handle)` stays, but:

- access is checked against visibility rules
- full handles are identifiers, not bearer tokens
- short aliases are preferred for agent UX

## `artifact.build`

Possible request:

```json
{
  "inputs": [
    "src/main.py",
    "src/geocoding.py",
    "src/weather_api.py"
  ],
  "entrypoints": ["src/main.py"]
}
```

Possible response:

```json
{
  "ok": true,
  "artifact_id": "art_92ab13ef",
  "digest": "sha256:...",
  "files": [
    {"name": "src/main.py", "alias": "m1a2b3c4"},
    {"name": "src/geocoding.py", "alias": "g5d6e7f8"},
    {"name": "src/weather_api.py", "alias": "w9h0i1j2"}
  ]
}
```

The exact schema can change, but the key is that the gateway returns a short artifact ID suitable for LLM use.

## `artifact.inspect`

Used by evaluator/auditor to understand what they are reviewing:

- file list
- entrypoints
- digests
- maybe manifest metadata

## Generic Enforcement Rules

The gateway should enforce these generic rules:

1. Raw session content is collaborative, not promotable
2. Only artifacts can be reviewed/installed/executed beyond scratch use
3. Artifact execution/install uses a closed file boundary
4. Full content handles do not bypass visibility rules
5. Root-session content visibility is generic infrastructure, not workflow logic

## Implementation Phases

## Phase 1: Simplify Content Visibility

### 1.1 Replace `content.persist`

**Files**:

- `autonoetic-gateway/src/runtime/tools.rs`
- `autonoetic-gateway/src/runtime/content_store.rs`
- docs referencing `content.persist`

Tasks:

- [ ] remove `content.persist` tool completely
- [ ] remove `persisted` tracking from content manifests
- [ ] move to `content.write(... visibility=...)`
- [ ] make visibility explicit in write responses

### 1.2 Root-session visibility

Tasks:

- [ ] add explicit `root_session_id` to session metadata / manifest state
- [ ] when `agent.spawn` creates child sessions, inherit the same root session
- [ ] make `session` visibility readable anywhere inside that root session
- [ ] keep `private` local to the writing session
- [ ] keep `global` readable outside the root session

### 1.3 Handle authorization

Tasks:

- [ ] require visibility authorization even for `sha256:...` reads
- [ ] require visibility authorization for short alias reads
- [ ] update errors so access failures are clear

## Phase 2: Introduce Artifacts

### 2.1 Artifact data model

**New files likely needed**:

- `autonoetic-gateway/src/runtime/artifact_store.rs`
- maybe `autonoetic-types/src/artifact.rs`

Tasks:

- [ ] define `ArtifactManifest`
- [ ] define short `artifact_id`
- [ ] store artifact metadata under gateway-controlled storage
- [ ] map artifact ID to content handles + names + digest + entrypoints

### 2.2 `artifact.build` tool

**File**:

- `autonoetic-gateway/src/runtime/tools.rs`

Tasks:

- [ ] add `artifact.build(inputs, entrypoints?, metadata?)`
- [ ] resolve inputs from session-visible content
- [ ] produce one short artifact ID
- [ ] ensure resulting artifact is immutable once built

### 2.3 `artifact.inspect` tool

Tasks:

- [ ] add `artifact.inspect(artifact_id)`
- [ ] expose file list, digests, entrypoints, manifest metadata
- [ ] make this the normal evaluator/auditor inspection primitive

## Phase 3: Closed Runtime Boundary

### 3.1 Artifact-based execution/install

Tasks:

- [ ] add or adapt execution/install paths so they can consume artifact IDs
- [ ] forbid promotion/install/run from raw session content
- [ ] mount only artifact files during artifact execution
- [ ] fail fast if code tries to read/import files outside the artifact boundary

### 3.2 Closure validation

Optional but strongly recommended:

- [ ] add static dependency/closure checks where possible
- [ ] at minimum, enforce runtime closure through sandbox mounts

The runtime boundary is the must-have. Static analysis is a bonus.

## Phase 4: Review / Approval Binding

This phase is what makes the strategy robust.

Tasks:

- [ ] evaluator reviews `artifact_id`, not arbitrary content names
- [ ] auditor reviews `artifact_id`, not arbitrary content names
- [ ] promotion / approval records bind to `artifact_id` (or its digest), not loose file handles
- [ ] install/run validates that the artifact being consumed is the reviewed artifact

This should replace the current brittle “planner remembers the right files” strategy.

## Phase 5: Docs Rewrite

### 5.1 Rewrite `content-store.md`

**File**:

- `autonoetic/docs/content-store.md`

Tasks:

- [ ] remove hierarchical parent-child sharing as the main model
- [ ] explain `private` / `session` / `global`
- [ ] explain that handles do not bypass authorization
- [ ] explain session sharing as collaboration convenience only
- [ ] explain that artifacts are mandatory for review/install/run

### 5.2 Update architecture / agent docs

Files:

- `autonoetic/docs/AGENTS.md`
- `autonoetic/docs/ARCHITECTURE.md`
- any workflow docs mentioning `content.persist`

Tasks:

- [ ] remove `content.persist`
- [ ] document `artifact.build`
- [ ] document “no artifact, no promotion”
- [ ] update examples so coder builds artifact before evaluator/auditor stages

## Phase 6: Tests

### 6.1 Content visibility tests

Tasks:

- [ ] `private` content unreadable outside writer session
- [ ] `session` content readable by sibling agents under same root session
- [ ] `global` content readable cross-session
- [ ] full handle read denied when visibility forbids it

### 6.2 Artifact tests

Tasks:

- [ ] build artifact from session content
- [ ] inspect artifact and verify file list
- [ ] reviewed artifact ID can be bound to downstream approval/promotion
- [ ] install/run rejects raw session content
- [ ] install/run accepts artifact only

### 6.3 Closed-boundary tests

Tasks:

- [ ] code inside artifact cannot import/open missing external helper file
- [ ] omitted dependency causes execution failure
- [ ] artifact with complete closure runs successfully

## Open Decisions

These should be settled before implementation starts:

1. **Visibility default**
   - recommended: `private`

2. **Should `global` mean durable + readable by any session with `ReadAccess`?**
   - recommended for Phase 1: yes

3. **Should artifact IDs be globally unique short IDs or digest-derived short aliases?**
   - recommended: short stable IDs backed by full digest in metadata

4. **Should `artifact.build` require explicit file list or support directory/entrypoint expansion?**
   - recommended Phase 1: explicit file list
   - Phase 2: optional closure expansion from entrypoints

5. **Should raw `sandbox.exec` remain allowed for scratch code?**
   - recommended:
     - yes for scratch/manual experimentation
     - no for reviewed/promotion/install paths

## Acceptance Criteria

- `content.persist` is gone
- `content.write(... visibility=...)` is the only content creation primitive
- handles no longer bypass visibility rules
- sibling agents in the same root session can collaborate through `session` visibility
- `artifact.build` exists and returns a short artifact ID
- evaluator/auditor review artifacts, not loose session content
- install/run/promotion paths require artifacts
- artifact execution/install is closed to files outside the artifact
- the gateway remains role-agnostic and enforces only generic visibility and closure rules

## Short Version

The robust model is:

- `content.write` for working files
- `artifact.build` for exact reviewable/executable closure
- only artifacts cross trust boundaries

That is much stronger than the current “planner passes the right file handles” strategy.
