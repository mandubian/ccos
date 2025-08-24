# wt/microvm-security — bootstrap

Planned scope (from issues):
- MicroVM Control Plane and Security Hardening (#71)
- MicroVM Step-Level Policy Enforcement (#72)
- Orchestrator: derive per-step MicroVM profile (network ACL, FS policy, determinism flags) (#60)

Owner: TBD

Initial tasks:
- [ ] Add `group:microvm-security` label to issues #71, #72, #60 and assign owners.
- [ ] Add a minimal README describing how to run MicroVM-local tests and the expected safety model.
- [ ] Implement per-step profile data shape in the orchestrator (small RFC + types).
- [ ] Add unit tests for step-level policy enforcement (happy path + denied access).

Notes:
This worktree is intended for early design and implementation of MicroVM-level controls. Keep changes small and add tests for every new enforcement rule.
