#!/usr/bin/env python3
"""
Fibonacci worker script for Autonoetic background execution.

Uses the autonoetic_sdk to persist state via the content-addressable
storage system. SDK is always available in the sandbox environment.
"""
import json
import sys

import autonoetic_sdk as sdk

_sdk = sdk.init()


def compute_next_fibonacci(state: dict) -> dict:
    """Compute the next Fibonacci number and update state."""
    previous = state.get("previous", 1)
    current = state.get("current", 1)
    index = state.get("index", 2)
    sequence = state.get("sequence", [1, 1])

    next_value = previous + current

    return {
        "previous": current,
        "current": next_value,
        "index": index + 1,
        "sequence": sequence + [next_value],
    }


def load_state() -> dict:
    """Load Fibonacci state from content store."""
    try:
        result = _sdk.files.read("fib_state.json")
        return json.loads(result["content"])
    except Exception:
        # First run - initialize with first two Fibonacci numbers
        return {
            "previous": 1,
            "current": 1,
            "index": 2,
            "sequence": [1, 1],
        }


def save_state(state: dict) -> dict:
    """Save Fibonacci state to content store."""
    state_json = json.dumps(state, indent=2)
    result = _sdk.files.write("fib_state.json", state_json)
    handle = result["handle"]
    
    # Persist so it survives session cleanup
    _sdk.files.persist(handle)
    
    return {
        "handle": handle,
        "name": "fib_state.json",
    }


def main():
    # Read input from stdin
    try:
        params = json.loads(sys.stdin.read().strip()) if sys.stdin.readable() else {}
    except json.JSONDecodeError:
        params = {}

    # Load state, compute, save
    state = load_state()
    new_state = compute_next_fibonacci(state)
    save_result = save_state(new_state)

    # Output result
    result = {
        "ok": True,
        "sequence_index": new_state["index"],
        "current_value": new_state["current"],
        "previous_value": new_state["previous"],
        "all_values": new_state["sequence"][-10:],
        "state_handle": save_result["handle"],
    }

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
