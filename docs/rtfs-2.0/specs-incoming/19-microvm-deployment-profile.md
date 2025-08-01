# RTFS 2.0 Incoming Spec: MicroVM Deployment Profile (Ultra-Light, Host-Hardened)

Status: Proposed (specs-incoming)
Audience: Runtime engineers, platform operators, security architects
Related:
- 17-agent-configuration-with-rtfs.md (RTFS-native agent config)
- 07-effect-system.md (effects → sandbox policy)
- 09-capability-contracts.md (privilege scoping)
- 12-admission-time-compilation-and-caching.md (admission caching)
- docs/ccos/specs/000-ccos-architecture.md

Purpose
Define a Firecracker-style MicroVM deployment profile for CCOS/RTFS agents that:
1) Keeps the runtime extremely small and simple.
2) Provides strong isolation and egress control to protect the host.
3) Remains AI-friendly: config, intents, plans all in RTFS; minimal moving parts.

---

## 1) Threat Model and Goals

Threats addressed
- Arbitrary code execution within the agent process.
- Data exfiltration via uncontrolled egress.
- Privilege escalation to host.
- Persistence and lateral movement from agent instance.

Non-goals
- Full-blown multi-tenant PaaS.
- Heavy-weight orchestration stacks.

Goals
- One-agent-per-MicroVM with minimal kernel and virtio devices.
- Deterministic boot with measured images.
- Tight egress/DLP control and FS scoping per step based on effects.
- Minimal guest footprint; fast start/stop; ephemeral by default.

---

## 2) Image Composition (Guest OS)

Base
- Minimal micro-OS image (Alpine/musl or Buildroot) or a solo-kernel WASM runtime guest.
- Userspace: only what is required to run “agent.bin” and WASM engine (if embedded).
- No package manager in production images. Immutable image builds.

Filesystem
- agent.bin (feature-gated) + rtfs runtime + optional embedded WASM runtime.
- capabilities/ (contracts + wasm modules) — RO image segment or verifiable store.
- policy/ (GK ruleset, optional) — RO.
- agent.config.rtfs — RW mount or baked; prefer baked + runtime overrides via virtio-vsock.
- No general /usr/bin tools; no shell inside production instances.

Devices
- Virtio-net (optional, gated).
- Virtio-fs or virtio-blk for RO capability store.
- Virtio-vsock for host control plane (admission cache, logs).
- No serial console in prod unless break-glass.

---

## 3) Host Firecracker/Cloud Hypervisor Setup

VM spec
- vCPU: 1–2, Memory: 128–512 MB (profile-dependent).
- Kernel: minimal, static; include only required drivers.
- Rootfs: read-only where possible; tmpfs for ephemeral paths (/tmp/agent).
- Seccomp: default, deny-most; whitelist Firecracker syscalls only.

Networking
- One of:
  - No NIC (air-gapped), use vsock for control plane only.
  - NIC bridged to egress proxy namespace with DNAT to proxy (no direct internet).
- Egress proxy namespace:
  - Domain allowlists, TLS pinning/mTLS, DLP filters (regex and structured), rate limits.
  - DNS pinning or inline DNS with policy.

Control plane (vsock)
- Admission cache fetch/push.
- Causal Chain event streaming (append-only).
- Metrics/health.
- No arbitrary exec; strict message schema.

Attestation
- Optional: measured boot; sign/verify rootfs and agent.bin digests.
- Store measurements in Causal Chain before admitting execution.

---

## 4) RTFS Config Extensions for MicroVM

Add a microvm block in agent.config:
```clojure
(agent.config
  :version "0.1"
  :agent-id "agent.ops.microvm"
  :profile :minimal
  :orchestrator {:isolation {:mode :microvm
                             :fs {:ephemeral true :mounts {:capabilities {:mode :ro}}}}
                 :dlp {:enabled true :policy :strict}}
  :network {:enabled true
            :egress {:via :proxy
                     :allow-domains ["eu.api.example.com" "slack.com"]
                     :mtls true
                     :tls_pins ["sha256/abc..."]}}
  :microvm
    {:kernel {:image "kernels/vmlinuz-min" :cmdline "console=none"}
     :rootfs {:image "images/agent-rootfs.img" :ro true}
     :resources {:vcpus 1 :mem_mb 256}
     :devices {:nic {:enabled true :proxy_ns "egress-proxy-ns"}
               :vsock {:enabled true :cid 3}}
     :attestation {:enabled true :expect_rootfs_hash "sha256:..."}}
  :capabilities {...} :governance {...} :marketplace {...})
```

Type schema (conceptual diff)
- :microvm {:kernel {:image string :cmdline string}
            :rootfs {:image string :ro boolean}
            :resources {:vcpus [:and number [:>= 1]] :mem_mb [:and number [:>= 64]]}
            :devices {:nic {:enabled boolean :proxy_ns [:optional string]}
                      :vsock {:enabled boolean :cid [:and number [:> 0]]}}
            :attestation {:enabled boolean :expect_rootfs_hash [:optional string]}}

Compiler validation
- Ensure :orchestrator.isolation.mode = :microvm when :microvm present.
- Validate :network.egress presence when :devices.nic.enabled = true.
- Enforce RO mounts for capabilities; require :fs.ephemeral true.

---

## 5) Execution Semantics inside the MicroVM

Step-level profile derivation
- For each step, the Orchestrator generates a profile from effects/resources:
  - Network: per-step domain/method whitelist sent to proxy via control API.
  - FS: ephemeral working dir; mount RO capabilities; no other paths.
  - Time/Random: bind to deterministic sources when policy requires.
  - LLM/GPU: only if enabled and bound; otherwise rejected at admission.

Idempotency and compensations
- Persist idempotency keys in a small WAL (tmpfs) to dedupe retries.
- Compensations must not require extended privileges beyond the original step.

Logging and Causal Chain
- Stream action events via vsock; buffer locally if control plane backpressure.

Crash/recovery
- On crash, host kills VM; supervisor restarts from a clean image; optionally replays last admitted plan if policy allows.

---

## 6) Host Supervisor (Tiny)

Responsibilities
- Prepare VM spec from agent.config (microvm block).
- Generate per-step egress ACL updates for the proxy.
- Manage vsock endpoints (admission cache, chain appender, metrics).
- Rotate images/keys; apply updates via blue/green images.

Non-responsibilities
- Do not run arbitrary code on behalf of agent.
- Do not mount host filesystems into guest except explicit RO capability store.

Footprint
- Single static binary (Rust), sub-10 MB; communicates with Firecracker API socket.

---

## 7) Egress/DLP Proxy (Per-Host or Shared)

Policy
- Domain allowlist per-agent and per-step.
- TLS pinning/mTLS with SPIFFE IDs if available.
- DLP filters: PII patterns, org secrets, prompt-injection stripping.
- Rate limiting, request/response size caps.

Integration
- Controlled namespace with only the proxy having egress.
- MicroVM NIC bridged into that namespace; no other routes.

---

## 8) Build/Deploy Pipeline (Minimal)

Build
- cargo build --features "microvm,wasm,cap_http,cap_fs"
- Produce agent.bin + rootfs image (Buildroot/Alpine) via OCI-to-raw tooling or disk builder.

Package
- kernel image, rootfs image, supervisor binary, agent.config.rtfs, capabilities/, keys/.

Deploy
- supervisor --config agent.config.rtfs --kernel kernels/vmlinuz-min --rootfs images/agent-rootfs.img
- Supervisor programs Firecracker VM, sets vsock and NIC, configures proxy ACLs.

Update
- Blue/green: create new rootfs; attest; switch VM; retire old.

---

## 9) Security Properties and Checks

- Rootfs and agent.bin hashes match expected (attestation).
- Capabilities directory mounted RO with verified contracts.
- No host FS mounts beyond explicit RO capability store.
- All egress flows through proxy with enforced ACLs/DLP.
- MicroVM seccomp + minimal kernel reduce attack surface.
- Causal Chain anchored periodically for tamper detection.

---

## 10) Example: RTFS MicroVM Profile Macro

```clojure
(def profile:microvm-networked
  (fn [agent-id allowed-domains]
    (-> (profile:minimal agent-id)
        (assoc :orchestrator {:isolation {:mode :microvm
                                          :fs {:ephemeral true}}
                              :dlp {:enabled true :policy :strict}}
               :network {:enabled true
                         :egress {:via :proxy
                                  :allow-domains allowed-domains
                                  :mtls true}}
               :microvm {:kernel {:image "kernels/vmlinuz-min" :cmdline "console=none"}
                         :rootfs {:image "images/agent-rootfs.img" :ro true}
                         :resources {:vcpus 1 :mem_mb 256}
                         :devices {:nic {:enabled true :proxy_ns "egress-proxy-ns"}
                                   :vsock {:enabled true :cid 3}}
                         :attestation {:enabled true}}))))
```

---

## 11) Acceptance Criteria

- Given an agent.config with :microvm mode, the supervisor can launch a Firecracker VM with RO rootfs, vsock, and proxy-bridged NIC.
- The Orchestrator enforces per-step profiles derived from effects/resources, communicated to the proxy/DLP.
- No direct internet from the MicroVM; all traffic is via proxy with domain/method pinning and TLS pinning.
- Capabilities store mounted RO; attempts to write or escape FS rejected.
- Causal Chain events streamed via vsock; on outage, buffered locally and flushed on reconnect.
- Full start→execute→stop cycle within seconds; memory and CPU overhead within profile’s bounds.

---

## 12) Rationale: Why MicroVM vs Container/WASM-only

- MicroVM provides stronger isolation boundary than namespaces, while remaining lighter than full VMs.
- WASM remains the preferred per-step sandbox inside the guest; the MicroVM isolates the guest process and its OS from the host.
- Split of concerns:
  - WASM: fine-grained, per-call sandbox of capability executables.
  - MicroVM: coarse boundary to protect host from agent as a whole.

---
