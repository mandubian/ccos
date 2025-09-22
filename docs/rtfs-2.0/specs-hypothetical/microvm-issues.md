
Issue 1: RTFS example: minimal MicroVM agent.config
Title:
RTFS example: minimal MicroVM agent.config with proxy egress, RO capabilities, vsock, attestation

Body:
Summary
Add a minimal RTFS-native agent.config demonstrating the MicroVM deployment profile as defined in docs/rtfs-2.0/specs-incoming/19-microvm-deployment-profile.md.

Deliverables
- docs/examples/agent.config.microvm.rtfs with:
  - :orchestrator.isolation.mode = :microvm
  - :orchestrator.isolation.fs {:ephemeral true :mounts {:capabilities {:mode :ro}}}
  - :dlp {:enabled true :policy :strict}
  - :network {:enabled true :egress {:via :proxy :allow-domains ["example.com"] :mtls true :tls_pins ["sha256/..."]}}
  - :microvm {:kernel {:image "kernels/vmlinuz-min" :cmdline "console=none"}
              :rootfs {:image "images/agent-rootfs.img" :ro true}
              :resources {:vcpus 1 :mem_mb 256}
              :devices {:nic {:enabled true :proxy_ns "egress-proxy-ns"}
                        :vsock {:enabled true :cid 3}}
              :attestation {:enabled true :expect_rootfs_hash "sha256:..."}}}
- Inline comments that map each field to Acceptance Criteria from the spec.

Acceptance Criteria
- File exists and validates conceptually against the type schema in the spec.
- Comments clearly reference the spec sections (Networking, Control plane, Attestation).
- Example can be consumed by future validation code without structural changes.

Labels
docs, config, microvm, good first issue

---

Issue 2: Spec macros: profile:microvm-min and profile:microvm-networked snippets
Title:
Spec macros: add profile:microvm-min and profile:microvm-networked snippets (docs examples)

Body:
Summary
Provide copy-pastable RTFS macro snippets for generating minimal MicroVM profiles.

Deliverables
- Create docs/rtfs-2.0/specs-incoming/examples/ directory.
- Add macros.rtfs with:
  - profile:microvm-min(agent-id)
  - profile:microvm-networked(agent-id, allowed-domains)
- Keep them aligned with the spec’s Example section (kernel/rootfs/resources/devices/attestation, DLP strict, FS ephemeral, RO caps).

Acceptance Criteria
- Snippets compile as RTFS S-expr (syntactically).
- Mirrors the spec’s example shape and constraints.
- Comments indicate where to override domains, cid, and resource bounds.

Labels
docs, examples, microvm

---

Issue 3: Compiler validation: microvm schema and policy checks
Title:
Compiler validation: MicroVM schema and policy checks for agent.config

Body:
Summary
Add a validation module to enforce MicroVM-specific constraints during config load/compile.

Deliverables
- New module: rtfs_compiler/src/config/validation_microvm.rs
- Validation rules:
  - When :microvm present → :orchestrator.isolation.mode must be :microvm
  - :orchestrator.isolation.fs.ephemeral must be true
  - Capabilities mount must be RO
  - :resources.vcpus >= 1; :resources.mem_mb >= 64
  - If :devices.vsock.enabled → :cid > 0
  - If :devices.nic.enabled → require :network.egress {:via :proxy, :allow-domains non-empty}
  - If :attestation.enabled → expect_rootfs_hash present (or document how policy resolves it)
- Unit tests:
  - Valid config passes
  - Each invalid case produces deterministic, auditable errors

Acceptance Criteria
- Validation errors use Result<T, RuntimeError> and include actionable context.
- Tests cover both valid and invalid shapes and pass with cargo test.

Labels
compiler, validation, microvm, tests

---

Issue 4: Orchestrator: per-step profile derivation skeleton
Title:
Orchestrator: derive per-step MicroVM profile (network ACL, FS policy, determinism flags)

Body:
Summary
Add a derivation layer that maps RTFS step effects/resources to a per-step profile structure for MicroVM execution.

Deliverables
- New module: rtfs_compiler/src/orchestrator/step_profile.rs
- Function signature idea:
  derive_step_profile(intent, plan_step, runtime_policy) -> StepProfile
- StepProfile should include:
  - network_acl: {allow_domains: [...], allow_methods: [...]} for proxy programming
  - fs_policy: {workdir: tmpfs path, capabilities_mount: RO, allowed_paths: []}
  - determinism: {time: deterministic|real, random: deterministic|real}
  - feature_gates: {llm: bool, gpu: bool}
- TODO stubs for integrating with the egress/DLP proxy control plane.

Tests
- Unit tests asserting structure output for a synthetic step with network+fs+determinism effects.

Acceptance Criteria
- Compiles and tests pass.
- Emits clear TODOs to wire network_acl to proxy API.
- Uses immutable data and Result<T, RuntimeError> where relevant.

Labels
orchestrator, runtime, microvm, tests

---

Issue 5: Supervisor: synthesize Firecracker spec from agent.config (stub)
Title:
Supervisor: synthesize Firecracker/Cloud Hypervisor JSON spec from agent.config (stub)

Body:
Summary
Create a module that transforms agent.config.microvm into a Firecracker-style machine configuration JSON (no actual spawn yet).

Deliverables
- New module: rtfs_compiler/src/supervisor/spec_synth.rs
- Structs for kernel, rootfs(RO), vsock(cid), nic(proxy_ns).
- Function:
  synthesize_vm_spec(agent_config: &AgentConfig) -> Result<FirecrackerSpec, RuntimeError>
- Serialize to JSON string with serde, add unit test snapshot.

Acceptance Criteria
- Serializes expected fields with RO rootfs, vsock cid, NIC bridged metadata.
- Unit tests pass and verify the JSON shape.
- Clear comments for where Firecracker API socket integration would occur.

Labels
supervisor, runtime, microvm, tests

---

Issue 6: Docs: runbook + acceptance checklist crosswalk in MicroVM spec
Title:
Docs: add runbook and acceptance checklist crosswalk to MicroVM spec

Body:
Summary
Enhance 19-microvm-deployment-profile.md with a practical runbook and a crosswalk mapping config fields to acceptance criteria.

Deliverables
- Runbook section:
  - Build: cargo build --features "microvm,wasm,cap_http,cap_fs"
  - Package: kernel, rootfs, supervisor, agent.config.rtfs, capabilities/, keys/
  - Deploy: supervisor … (cmd example)
  - Update: blue/green with attest + switch
- Acceptance crosswalk table/list mapping:
  - RO rootfs → :microvm.rootfs.ro
  - Proxy egress → :network.egress and :devices.nic.enabled
  - Vsock/control plane → :devices.vsock.cid and control-plane endpoints
  - DLP strict → :orchestrator.dlp.policy
  - FS ephemeral/RO caps → :orchestrator.isolation.fs.*
- Add notes on attestation and causal chain buffering.

Acceptance Criteria
- Spec updated with clear, actionable steps.
- Crosswalk maps each acceptance bullet to concrete config fields.
- No contradictions with the rest of the spec.

Labels
docs, microvm, runbook

---

If you want, I can also draft the PR description template to reference these issues and explain the added files.