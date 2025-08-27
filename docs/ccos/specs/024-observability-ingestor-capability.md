````markdown
# CCOS Specification 024: Observability Ingestor Capability

**Spec ID:** SEP-024
**Capability ID:** `observability.ingestor:v1.ingest`
**Status:** Proposed
**Version:** 1.0
**Date:** 2025-08-26
**Related:**
- [SEP-003: Causal Chain](./003-causal-chain.md)
- [SEP-013: Working Memory](./013-working-memory.md)
- [SEP-023: Observability Foundation](./023-observability-foundation.md)

A local capability that ingests distilled Working Memory entries from:
- single record: ["single", <record>]
- batch of records: ["batch", [<record>...]]
- full replay from the Causal Chain: ["replay"]

## Input schema (informal)

- record is a map with fields:
  - action_id (string, optional; generated if missing)
  - kind (string, default "CapabilityCall")
  - provider (string, optional)
  - timestamp_s (int seconds, default now)
  - summary (string)
  - content (string)
  - plan_id, intent_id, step_id (string, optional)
  - attestation_hash, content_hash (string, optional)

## Output

- map with keys:
  - mode: "single" | "batch" | "replay"
  - ingested: number of entries appended
  - scanned_actions (replay only): count scanned from the chain

## Enablement

- WM ingestor must be enabled (env `CCOS_ENABLE_WM_INGESTOR=1` or `CCOSConfig.enable_wm_ingestor=true`).

## Notes

- Idempotent via content hash; duplicate content overwrites same WM id.
- Lightweight and safe: ignores ingestion errors per-record in batch.

## Examples

```
; single
(call :observability.ingestor:v1.ingest { :args ["single" { :action_id "demo-1" :summary "hello" :content "payload" :timestamp_s 123 }] })

; batch
(call :observability.ingestor:v1.ingest { :args ["batch" [
  { :action_id "a" :summary "s-a" :content "c-a" }
  { :action_id "b" :summary "s-b" :content "c-b" }
]] })

; replay
(call :observability.ingestor:v1.ingest { :args ["replay"] })
```

## Verification

- After a single ingest call, inspect function/capability metrics to ensure counters incremented:
  - `host.get_capability_metrics("observability.ingestor:v1.ingest")`
  - `host.get_function_metrics("observability.ingestor:v1.ingest")`
- Recent structured logs should include entries for the call:
  - `host.get_recent_logs(32)` contains `action_appended` / `action_result_recorded` with the capability id in `function_name`.

Quickstart guide: See docs/ccos/guides/observability-quickstart.md for a minimal end-to-end usage sample from Rust.
````