"""
CCOS Python SDK for sandboxed capability execution.

Provides a minimal interface allowing Python code running inside the CCOS
bubblewrap sandbox to interact with the CCOS capability system via a
two-way stdout/stdin protocol intercepted by the host runtime.

PROTOCOL
--------
Python code cannot make HTTP calls inside the sandbox (network is disabled by
default and no bearer token is available). Instead, this SDK emits structured
markers on stdout that the CCOS sandbox runner intercepts and dispatches:

    CCOS_CALL::<json>        ← Python writes to stdout
    CCOS_RESULT::<json>      ← host writes to Python's stdin

Where each <json> is a UTF-8 JSON object.

CCOS_CALL shape:
    { "cap": "<capability_id>", "inputs": { ... } }

CCOS_RESULT shape:
    { "success": true,  "value": <result_json> }   # on success
    { "success": false, "error": "<message>"    }   # on failure

The host intercepts each CCOS_CALL:: line, executes the local capability
synchronously, then writes one CCOS_RESULT:: line to stdin. Python blocks on
`sys.stdin.readline()` until the result arrives, making the call appear
synchronous from Python's perspective.

The host runner mounts this file at /workspace/input/ccos_sdk.py and sets
PYTHONPATH=/workspace/input so scripts can `import ccos_sdk`.

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
from typing import Any


_CALL_PREFIX = "CCOS_CALL::"
_RESULT_PREFIX = "CCOS_RESULT::"


def _call(cap: str, inputs: dict) -> Any:  # noqa: ANN401
    """Emit a CCOS_CALL:: marker on stdout, block until CCOS_RESULT:: arrives on stdin.

    Returns the deserialized result value, or raises RuntimeError on failure.
    """
    marker = json.dumps({"cap": cap, "inputs": inputs}, ensure_ascii=False)
    # Write to stderr-less stdout (PYTHONUNBUFFERED=1 is set by the host).
    sys.stdout.write(f"{_CALL_PREFIX}{marker}\n")
    sys.stdout.flush()

    # Block waiting for the host to write the result back.
    result_line = sys.stdin.readline()
    if not result_line:
        raise RuntimeError(f"CCOS host closed stdin before returning result for cap '{cap}'")

    result_line = result_line.rstrip("\n")
    if not result_line.startswith(_RESULT_PREFIX):
        raise RuntimeError(
            f"Unexpected response from CCOS host (cap '{cap}'): {result_line!r}"
        )

    result = json.loads(result_line[len(_RESULT_PREFIX):])
    if result.get("success"):
        return result.get("value")
    raise RuntimeError(f"Capability '{cap}' failed: {result.get('error', 'unknown error')}")


# ---------------------------------------------------------------------------
# Memory namespace
# ---------------------------------------------------------------------------

class _Memory:
    """Interface to ccos.memory.get / ccos.memory.store."""

    def get(self, key: str, default: Any = None) -> Any:  # noqa: ANN401
        """Retrieve a value from Working Memory.

        Returns the stored value, or `default` if the key does not exist.
        Blocks until the host dispatches the capability and returns the result.
        """
        result = _call("ccos.memory.get", {"key": key, "default": default})
        # result is the serialised MemoryGetOutput:
        # { "value": <any>, "found": bool, "expired": bool }
        if isinstance(result, dict):
            if result.get("found"):
                return result.get("value", default)
            return default
        return default

    def store(self, key: str, value: Any) -> None:  # noqa: ANN401
        """Store a value in Working Memory under the given key.

        Blocks until the host confirms the write.
        """
        _call("ccos.memory.store", {"key": key, "value": value})


# ---------------------------------------------------------------------------
# IO / logging namespace
# ---------------------------------------------------------------------------

class _IO:
    """Utility I/O helpers."""

    def log(self, message: str) -> None:
        """Emit a log message via ccos.io.log (best-effort, non-blocking result)."""
        try:
            _call("ccos.io.log", {"message": str(message)})
        except Exception:  # noqa: BLE001
            # Logging must never crash user code.
            pass


# ---------------------------------------------------------------------------
# Top-level CCOS object
# ---------------------------------------------------------------------------

class _CCOS:
    memory: _Memory = _Memory()
    io: _IO = _IO()

    def call(self, capability_id: str, inputs: dict) -> Any:  # noqa: ANN401
        """Generic capability call — dispatches any local CCOS capability."""
        return _call(capability_id, inputs)


# Singleton exported as `ccos`
ccos = _CCOS()

# Module-level aliases so `import ccos_sdk; ccos_sdk.memory.get(...)` works
# (the prompts and docs use this flat style rather than `ccos_sdk.ccos.memory`)
memory: _Memory = ccos.memory
io: _IO = ccos.io


def call(capability_id: str, inputs: dict) -> Any:  # noqa: ANN401
    """Module-level generic capability call — delegates to ccos.call()."""
    return ccos.call(capability_id, inputs)


__all__ = ["ccos", "memory", "io", "call"]
