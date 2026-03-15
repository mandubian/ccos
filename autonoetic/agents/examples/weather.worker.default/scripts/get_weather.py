#!/usr/bin/env python3
"""Simple weather data retrieval script for deterministic agent execution.

This script runs without LLM, making it:
- Fast (~100ms)
- Cheap (no token usage)
- Deterministic (same input → same output)

Usage:
    Input: JSON with "location" field
    Output: JSON with weather data
"""

import json
import sys

# Simulated weather data (deterministic)
WEATHER_DB = {
    "tokyo": {"temp_c": 22, "condition": "partly cloudy", "humidity": 65},
    "london": {"temp_c": 15, "condition": "overcast", "humidity": 78},
    "new york": {"temp_c": 18, "condition": "sunny", "humidity": 55},
    "paris": {"temp_c": 17, "condition": "light rain", "humidity": 72},
    "sydney": {"temp_c": 24, "condition": "clear", "humidity": 60},
    "berlin": {"temp_c": 14, "condition": "cloudy", "humidity": 68},
    "default": {"temp_c": 20, "condition": "unknown", "humidity": 50},
}

def get_weather(location: str) -> dict:
    """Get weather for a location (simulated, deterministic)."""
    location_lower = location.lower().strip()
    data = WEATHER_DB.get(location_lower, WEATHER_DB["default"])
    return {
        "location": location,
        "temperature_c": data["temp_c"],
        "condition": data["condition"],
        "humidity_percent": data["humidity"],
        "source": "simulated",
    }

def main():
    try:
        # Read input from stdin (gateway provides JSON)
        input_data = json.load(sys.stdin)
        location = input_data.get("location", "unknown")
        
        result = get_weather(location)
        
        # Output JSON to stdout (gateway captures this)
        json.dump(result, sys.stdout)
        print()  # Newline after JSON
        
    except json.JSONDecodeError as e:
        json.dump({"error": f"Invalid JSON input: {e}"}, sys.stdout)
        print()
        sys.exit(1)
    except Exception as e:
        json.dump({"error": str(e)}, sys.stdout)
        print()
        sys.exit(1)

if __name__ == "__main__":
    main()
