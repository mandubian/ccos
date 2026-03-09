# Reference Agent Bundles

This directory groups reference agent bundles by role family.

- `lead/`: front-door and orchestration agents.
- `specialists/`: role-specific hands such as researcher, coder, auditor, and evaluator.
- `evolution/`: builders and steward agents that create or adapt other agents.

Current bundles:

- `lead/planner.default/`: default lead planner for ambiguous ingress.
- `specialists/researcher.default/`: evidence and source synthesis specialist.
- `specialists/architect.default/`: architecture and interface design specialist.
- `specialists/coder.default/`: implementation specialist.
- `specialists/debugger.default/`: root-cause and failure diagnosis specialist.
- `specialists/evaluator.default/`: validation specialist.
- `specialists/auditor.default/`: governance and risk specialist.
- `evolution/specialized_builder.default/`: installs durable specialist agents.
- `evolution/evolution-steward.default/`: governs promotion and upgrade decisions.
- `evolution/memory-curator.default/`: distills durable memory from execution outcomes.

Notes:

- These are reference bundles committed with the project.
- Runtime-loaded agents still come from the configured `agents_dir` in `config.yaml`.
- Copy or sync a bundle from here into your runtime `agents_dir` to activate it.
