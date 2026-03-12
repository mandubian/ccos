#!/usr/bin/env python3
"""Format coder output into the declared returns schema."""

from __future__ import annotations

import json
import sys
from typing import Any, Dict


def ensure_array(value: Any) -> list[Any]:
    if isinstance(value, list):
        return value
    if value is None:
        return []
    return [value]


def ensure_object(value: Any) -> Dict[str, Any]:
    if isinstance(value, dict):
        return value
    return {}


def format_payload(text: str) -> Dict[str, Any]:
    try:
        parsed = json.loads(text)
    except Exception:
        parsed = {"changes": [text], "verification": {}, "risks": []}

    if not isinstance(parsed, dict):
        parsed = {"changes": [parsed], "verification": {}, "risks": []}

    parsed["changes"] = ensure_array(parsed.get("changes"))
    parsed["verification"] = ensure_object(parsed.get("verification"))
    parsed["risks"] = ensure_array(parsed.get("risks"))
    return parsed


def main() -> int:
    response = json.load(sys.stdin)
    text = response.get("text")
    if isinstance(text, str):
        response["text"] = json.dumps(format_payload(text))
    json.dump(response, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
