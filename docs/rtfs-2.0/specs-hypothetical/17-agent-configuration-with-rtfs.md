# RTFS 2.0 Incoming Spec: Minimal Agent Footprint and RTFS-Native Configuration

Status: Proposed (specs-incoming)
Audience: Runtime/Compiler engineers, Arbiter implementers, Packagers
Related:
- Effect System: docs/rtfs-2.0/specs-incoming/07-effect-system.md
- Capability Contracts: docs/rtfs-2.0/specs-incoming/09-capability-contracts.md
- Admission & Caching: docs/rtfs-2.0/specs-incoming/12-admission-time-compilation-and-caching.md
- Compiler Plan: docs/rtfs-2.0/specs-incoming/14-compiler-enhancement-plan-effects-and-types.md
- Intent & Plan Synthesis: docs/rtfs-2.0/specs-incoming/16-intent-synthesis-and-plan-translation.md
- CCOS Architecture: docs/ccos/specs/000-ccos-architecture.md

Purpose
Define a tiny, composable agent runtime profile and a RTFS-native configuration model that allows:
1) Building and running agents with minimal footprint and isolation.
2) Selecting only the needed subsystems/capabilities per agent (no bloated stack).
3) Letting AI (Arbiter) author/modify config using the same homoiconic substrate as plans, not YAML/JSON.

---

## 1) Minimal Agent Runtime Profile

Tiny core (always-on)
- RTFS evaluator + type/effect validator (pure core, no I/O)
- Orchestrator microkernel:
  - step/call execution engine
  - effect-to-sandbox policy adapter
  - minimal DLP hooks (off by default)
- Governance Kernel verifier:
  - signature checks
  - policy admission (can be local, minimal ruleset)
- Causal Chain lite:
  - content-hash events
  - pluggable storage (in-memory or append-only file)
- Capability marketplace client (stub):
  - local registry (file dir) with contract loader

Optional modules (opt-in by config)
- Network capability set (http/grpc) + egress proxy hooks
- Filesystem capability set (scoped paths)
- LLM local adapter (if present), remote LLM bridge
- GPU bindings (if present)
- Advanced GK policy engine (Cedar/Rego adapter)
- Telemetry/OpenTelemetry exporter
- Attestation/Signature providers (Sigstore/TUF)
- Persistence backends (KV/SQLite/Log-structured)

Isolation options (select one)
- WASM sandbox (preferred; single static runtime)
- Container namespace-lite (rootless, minimal)
- MicroVM (firecracker) for high-risk ops only

Goal: runnable binary ~ few MB + optional WASM engine. No heavy service mesh. All optional subsystems compiled as features.

---

## 2) RTFS-Native Configuration Model

Why RTFS for config (not YAML/JSON)
- Homoiconic, easy for AI to generate/edit with AST transforms.
- Typed schemas with refinements → compile-time validation.
- Inline policy, effects, and resource annotations re-used from program semantics.
- Supports macros/templates for agent profiles.
- Single substrate across Intent, Plan, and Config.

Configuration object (top-level)
```clojure
(agent.config
  :version "0.1"
  :agent-id "agent.sre.minimal"
  :profile :minimal        ; selects a base template (see §3)
  :features [:network :telemetry] ; explicit module flags
  :capabilities
    { :http {:enabled true
             :egress {:allow-domains ["eu.api.example.com" "slack.com"]
                      :mtls true}}
      :fs   {:enabled false}
      :llm  {:enabled false} }
  :governance
    {:policies
      { :default {:risk_tier :medium
                  :requires_approvals 0}
        :high_risk {:requires_approvals 2} }
     :keys {:verify "pubkey-pem"}}
  :orchestrator
    {:isolation {:mode :wasm
                 :fs {:ephemeral true}}
     :dlp {:enabled false}}
  :causal_chain
    {:storage {:mode :append_file :path "./causal.chain"}
     :anchor {:enabled false}}
  :marketplace
    {:registry_paths ["./capabilities/"]} )
```

Validation
- The compiler validates config using RTFS type schemas, same as programs.
- Missing/unknown fields → compile-time diagnostics with repair hints.

---

## 3) Config Profiles and Macros

Profiles as macros (expand into base config)
```clojure
(def profile:minimal
  (fn [agent-id]
    (agent.config
      :version "0.1" :agent-id agent-id :profile :minimal
      :features []
      :capabilities {:http {:enabled false} :fs {:enabled false} :llm {:enabled false}}
      :governance {:policies {:default {:risk_tier :low :requires_approvals 0}}}
      :orchestrator {:isolation {:mode :wasm}}
      :causal_chain {:storage {:mode :in_memory}}
      :marketplace {:registry_paths []})))

(def profile:networked
  (fn [agent-id allowed-domains]
    (let [base (profile:minimal agent-id)]
      (assoc-in base [:features] [:network]
                     [:capabilities :http] {:enabled true
                                            :egress {:allow-domains allowed-domains :mtls true}}))))
```

Usage by Arbiter (AI-friendly)
- Arbiter selects a profile fn and applies arguments inferred from intent/policy:
  (profile:networked "agent.ops.eu" ["eu.api.example.com" "slack.com"])

---

## 4) Declarative Capability Wiring

Capability entries are RTFS maps with contract references and effect/resource scoping.

Example
```clojure
{:capability :com.collaboration:v1.slack-post
 :contract "capabilities/slack-post.edn"
 :enabled true
 :effects [[:network {:domains ["slack.com"] :methods [:POST] :mTLS true}]]
 :resources {:max_time_ms 5000}
 :runtime {:sandbox :wasm}}
```

Compiler checks
- Load contract; validate effect/resource subset ≤ contract.
- Merge into Orchestrator routing table.
- Reject privilege broadening.

---

## 5) Per-Agent Policy and Budgets (Config-level)

Policy as maps with typed fields; GK consumes directly.
```clojure
:governance
  {:policies
    { :default {:risk_tier :medium :requires_approvals 0
                :budgets {:max_cost_usd 50 :token_budget 2e5}}
      :llm_heavy {:risk_tier :high :requires_approvals 1
                  :budgets {:max_cost_usd 200 :token_budget 2e6}}}}
```

Assignment in plan admission
- Admission API resolves policy by risk tier/effect signature and attaches it to the Plan Envelope.

---

## 6) Boot-time Behavior

Startup sequence
1) Load agent.config (RTFS form or compiled artifact).
2) Validate config (type/effects). Hard fail on violations.
3) Initialize minimal subsystems by feature flags.
4) Build capability routing table from registry paths.
5) Start Orchestrator with isolation profile and governance hooks.
6) Advertise capability subset (optional).

Result: single process with only selected modules loaded; no global daemon requirements.

---

## 7) Footprint Targets (non-binding, guiding)

- Core binary: < 10 MB (Rust, LTO, feature-gated)
- WASM runtime: < 5–10 MB (if embedded)
- Memory (idle): < 50–100 MB for minimal agent
- No background heavyweight services; all optional

---

## 8) RTFS Config Type Schema (Conceptual)

```clojure
(def Config
  {:version string
   :agent-id string
   :profile keyword
   :features [:vector keyword]
   :capabilities
     {:http {:enabled boolean
             :egress {:allow-domains [:vector string]
                      :mtls boolean}}
      :fs   {:enabled boolean
             :paths [:vector string]}
      :llm  {:enabled boolean
             :models [:vector string]}}
   :governance
     {:policies {:map keyword {:risk_tier [:enum :low :medium :high]
                               :requires_approvals [:and number [:>= 0]]
                               :budgets {:max_cost_usd number
                                         :token_budget number}}}
      :keys {:verify string}}
   :orchestrator
     {:isolation {:mode [:enum :wasm :container :microvm]
                  :fs {:ephemeral boolean}}
      :dlp {:enabled boolean}}
   :causal_chain
     {:storage {:mode [:enum :in_memory :append_file :sqlite]
                :path [:optional string]}
      :anchor {:enabled boolean}}
   :marketplace {:registry_paths [:vector string]}})
```

---

## 9) AI Ergonomics: Why This is Practical for Agents

- Single substrate (RTFS) → Arbiter can reuse its plan-synthesis machinery (AST rewrites) to author configs.
- Typed feedback loop → The same compiler diagnostics system guides config corrections.
- Macro profiles → Agents can pick base profiles by name and override parameters programmatically.
- No text-escaping games or YAML indentation issues for LLMs.
- Config and plans share effect/resource vocabulary → easy cross-check and policy alignment.

---

## 10) Minimal Build/Deploy Flow

Build
- cargo build --features "wasm,cap_http,cap_fs,cap_llm(optional),otel(optional)"
- Output: single binary + optional WASM engine

Package
- Directory:
  - agent.bin
  - agent.config.rtfs (or compiled .bin)
  - capabilities/ (contracts + wasm modules)
  - policies/ (optional)
  - causal.chain (optional file storage)

Run
- ./agent.bin --config agent.config.rtfs

---

## 11) Examples

11.1 Minimal no-network agent
```clojure
(profile:minimal "agent.airgap")
```

11.2 Networked EU-only notifier
```clojure
(profile:networked "agent.ops.eu" ["eu.api.example.com" "slack.com"])
```

11.3 LLM-heavy analytics agent (explicit features)
```clojure
(-> (profile:minimal "agent.analytics")
    (assoc :features [:network :llm :telemetry]
           :capabilities {:http {:enabled true
                                 :egress {:allow-domains ["eu.api.example.com"] :mtls true}}
                          :llm {:enabled true :models ["local-7b-eu"]}}
           :governance {:policies {:llm_heavy {:risk_tier :high :requires_approvals 1
                                               :budgets {:max_cost_usd 200 :token_budget 2e6}}}}))
```

---

## 12) Acceptance Criteria

- Config loader parses RTFS configuration and validates against the schema.
- Feature flags deterministically include/exclude modules at runtime.
- Capability registry builds route table only for enabled capabilities; effect bounds enforced.
- GK consumes config policies; admission rejects plans that exceed config budgets/effects.
- Minimal agent starts with < 100 MB RSS in minimal profile and no heavy background services.

---

Changelog
- v0.1 (incoming): RTFS-native configuration model, minimal agent profile, feature-gated runtime, and examples.
