#!/usr/bin/env bash

# Test script to demonstrate RTFS syntax formatting for execution results

echo "=== RTFS Syntax Execution Result Formatting Demo ==="
echo ""

echo "Before (old format):"
echo "✅ Result: \"Hello World\""
echo "❌ Result: Execution failed"
echo ""

echo "After (new RTFS syntax):"
echo "✅ (result \"Hello World\")"
echo "❌ (error \"Execution failed\")"
echo ""

echo "Examples of different value types:"
echo "✅ (result 42)              # Numbers"
echo "✅ (result true)            # Booleans"
echo "✅ (result \"text\")          # Strings"
echo "✅ (result nil)             # Null values"
echo "❌ (error \"Custom error\")   # Error messages"
echo ""

echo "The TUI demo now displays execution results in this clean RTFS syntax format!"
echo "This makes the results more consistent with RTFS language conventions."
