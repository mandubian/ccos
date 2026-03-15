#!/usr/bin/env python3
"""Compare base/target I/O schemas and emit mapping requirements.

Reads JSON from stdin:
{
  "base_accepts": {...} | null,
  "base_returns": {...} | null,
  "target_accepts": {...} | null,
  "target_returns": {...} | null
}
"""

from __future__ import annotations

import json
import sys
from typing import Any, Dict, List, Tuple


def required_fields(schema: Dict[str, Any]) -> List[str]:
    req = schema.get("required")
    if isinstance(req, list):
        return [str(x) for x in req]
    return []


def schema_type(schema: Dict[str, Any]) -> str:
    t = schema.get("type")
    if isinstance(t, str):
        return t
    return "unknown"


def infer_mappings(base_required: List[str], target_required: List[str]) -> List[Dict[str, str]]:
    """Infer deterministic field mappings target->base.

    Strategy:
    1) keep same-name fields paired first,
    2) pair remaining target/base fields by required-list order.
    """
    mappings: List[Dict[str, str]] = []
    base_set = set(base_required)
    target_set = set(target_required)
    shared = [name for name in target_required if name in base_set]
    for name in shared:
        mappings.append({"from": name, "to": name})

    rem_base = [name for name in base_required if name not in shared]
    rem_target = [name for name in target_required if name not in shared]
    for src, dst in zip(rem_target, rem_base):
        mappings.append({"from": src, "to": dst})
    return mappings


def compare(
    base: Dict[str, Any] | None, target: Dict[str, Any] | None, label: str
) -> Tuple[bool, bool, List[str], List[Dict[str, str]]]:
    notes: List[str] = []
    if base is None or target is None:
        notes.append(f"{label}: missing schema on one side, mapping required")
        return (False, True, notes, [])

    base_t = schema_type(base)
    target_t = schema_type(target)
    if base_t != target_t:
        notes.append(f"{label}: type mismatch {base_t} -> {target_t}")
        return (False, True, notes, [])

    base_required = required_fields(base)
    target_required = required_fields(target)
    base_set = set(base_required)
    target_set = set(target_required)
    missing = sorted(target_set - base_set)
    extra = sorted(base_set - target_set)
    if missing:
        notes.append(f"{label}: target requires additional fields {missing}; base extras {extra}")
        mappings: List[Dict[str, str]] = []
        if base_t == "object" and base_required and target_required:
            mappings = infer_mappings(base_required, target_required)
            if mappings:
                notes.append(f"{label}: inferred mappings {mappings}")
        return (False, True, notes, mappings)

    notes.append(f"{label}: compatible")
    return (True, False, notes, infer_mappings(base_required, target_required))


def main() -> int:
    payload = json.load(sys.stdin)

    base_accepts = payload.get("base_accepts")
    base_returns = payload.get("base_returns")
    target_accepts = payload.get("target_accepts")
    target_returns = payload.get("target_returns")

    accepts_ok, need_in_map, accepts_notes, input_mappings = compare(
        base_accepts, target_accepts, "accepts"
    )
    returns_ok, need_out_map, returns_notes, output_mappings = compare(
        base_returns, target_returns, "returns"
    )

    result = {
        "accepts_compatible": accepts_ok,
        "returns_compatible": returns_ok,
        "requires_input_mapping": need_in_map,
        "requires_output_mapping": need_out_map,
        "input_mappings": input_mappings,
        "output_mappings": output_mappings,
        "notes": accepts_notes + returns_notes,
    }
    json.dump(result, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
