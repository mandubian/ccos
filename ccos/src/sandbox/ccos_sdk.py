"""
CCOS Python SDK for sandboxed capability execution.

Provides a minimal interface allowing Python code running inside the CCOS
bubblewrap sandbox to interact with the CCOS capability system via stdout
markers intercepted by the host runtime.

PROTOCOL
--------
Python code cannot make HTTP calls inside the sandbox (network is disabled by
default, and no bearer token is available). Instead, this SDK emits structured
markers on stdout that the CCOS sandbox runner intercepts and dispatches:

    CCOS_CALL::<json>

Where <json> is a UTF-8 JSON object with the shape:
    {
        "cap": "<capability_id>",
        "inputs": { ... }
    }

The runner intercepts the line, executes the capability locally (in-process),
and injects the result into the next iteration as:

    CCOS_RESULT::<json>

Where <json> is the raw RTFS Value serialized as JSON.

TODO: The bubblewrap.rs process runner must be updated to:
  1. Scan stdout lines for the CCOS_CALL:: prefix and strip them from the
     visible output passed back to the LLM.
  2. Execute the named local capability with the decoded inputs.
  3. Write the result line (CCOS_RESULT::...) to the process's stdin.
  4. Mount this file at /ccos/ccos_sdk.py and set PYTHONPATH=/ccos so Python
     scripts can `import ccos_sdk` without any install step.

USAGE EXAMPLE
-------------
    import ccos_sdk

    state = ccos_sdk.memory.get("fibonacci_state", default=[0, 1])
    a, b = state[-2], state[-1]
    next_val = a + b
    state.append(next_val)
    ccos_sdk.memory.store("fibonacci_state", state)
    print(f"Next Fibonacci: {next_val}")
"""

from __future__ import annotations

import json
import sys
from typing import Any, Optional


def _emit(cap: str, inputs: dict) -> None:
    """Emit a CCOS_CALL:: marker on stdout and flush immediately."""
    marker = json.dumps({"cap": cap, "inputs": inputs}, ensure_ascii=False)
    print(f"CCOS_CALL::{marker}", flush=True)


# ---------------------------------------------------------------------------
# Memory namespace
# ---------------------------------------------------------------------------

class _Memory:
    """Interface to ccos.memory.get / ccos.memory.store."""

    def get(self, key: str, default: Any = None) -> None:  # noqa: ANN401
        """Emit a request to retrieve a value from Working Memory.

        NOTE: In the current implementation the result is NOT returned to
        Python — the SDK emits the CCOS_CALL:: marker and the host runner
        must inject the result via CCOS_RESULT:: (not yet implemented).
        Until that interceptor is in place, use the LLM-as-orchestrator
        pattern: let the LLM agent call ccos.memory.get, embed the result
        as a literal in the Python script, then call ccos.memory.store with
        the output.
        """
        _emit("ccos.memory.get", {"key": key, "default": default})

    def store(self, key: str, value: Any) -> None:  # noqa: ANN401
        """Emit a request to store a value in Working Memory."""
        _emit("ccos.memory.store", {"key": key, "value": value})


# ---------------------------------------------------------------------------
# IO / logging namespace
# ---------------------------------------------------------------------------

class _IO:
    """Utility I/O helpers."""

    def log(self, message: str) -> None:
        """Emit a structured log message via ccos.io.log."""
        _emit("ccos.io.log", {"message": str(message)})


# ---------------------------------------------------------------------------
# Top-level CCOS object
# ---------------------------------------------------------------------------

class _CCOS:
    memory: _Memory = _Memory()
    io: _IO = _IO()

    def call(self, capability_id: str, inputs: dict) -> None:
        """Generic capability call — emits CCOS_CALL:: for any local cap."""
        _emit(capability_id, inputs)


# Singleton exported as `ccos`
ccos = _CCOS()

__all__ = ["ccos"]
