# Tiered Memory Probe Example

This example is a behavioral probe for Tier 1 vs Tier 2 memory usage.

It asks a generic builder agent to install a recurring worker from a user request that implies two persistence layers:

- short deterministic checkpoint continuity for each scheduled tick
- reusable knowledge for future workers and sessions

The prompt does not explicitly say "use the SDK" or "call memory.remember". The worker must infer the right architecture from the constraints.

## What This Probe Checks

1. A child worker is installed and scheduled by the builder (`agent.install`).
2. Tier 1 checkpoint files are produced under `state/`.
3. Generated worker code references memory APIs (`autonoetic_sdk` or `memory.remember/recall/search`).
4. Runtime causal traces include memory-category events from worker execution.

If the worker only uses local files and never touches long-term memory primitives, the probe fails.

## Run

From `autonoetic/`:

```bash
bash examples/tiered_memory_probe/run.sh
```

Optional args:

```bash
bash examples/tiered_memory_probe/run.sh /tmp/autonoetic-tiered-memory-probe builder_memory_probe
```

## Expected Outcome

A successful run prints `PROBE_RESULT: PASS` and exits `0`.

A failed run prints `PROBE_RESULT: FAIL` and exits non-zero, with diagnostics including:

- generated child agent ID
- whether Tier 1 state files were produced
- whether SDK/memory markers were found in worker code
- whether memory events were logged in child causal trace
