#!/usr/bin/env python3
"""Normalize researcher input query in the last user message."""

from __future__ import annotations

import json
import re
import sys
from typing import Any


def normalize_text(value: str) -> str:
    # Trim and collapse repeated whitespace into single spaces.
    return re.sub(r"\s+", " ", value).strip()


def normalize_content(content: Any) -> Any:
    if isinstance(content, str):
        stripped = content.strip()
        try:
            parsed = json.loads(stripped)
        except Exception:
            return normalize_text(content)
        if isinstance(parsed, dict) and isinstance(parsed.get("query"), str):
            parsed["query"] = normalize_text(parsed["query"])
            return json.dumps(parsed)
        return stripped
    if isinstance(content, dict) and isinstance(content.get("query"), str):
        content["query"] = normalize_text(content["query"])
    return content


def main() -> int:
    request = json.load(sys.stdin)
    messages = request.get("messages")
    if isinstance(messages, list) and messages:
        last = messages[-1]
        if isinstance(last, dict) and "content" in last:
            last["content"] = normalize_content(last.get("content"))
    json.dump(request, sys.stdout)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
