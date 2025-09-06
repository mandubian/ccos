#!/usr/bin/env bash

# Test script to verify execution result visualization in TUI
# This script will create a simple test case and run it

echo "Testing execution result visualization..."

# Create a simple RTFS plan for testing
cat > test_execution.rtfs << 'EOF'
(do
  (step "Test execution" (call :stdlib.core:println {"message" "Hello from RTFS!"}))
  (step "Return result" "Execution completed successfully")
)
EOF

echo "Created test plan: test_execution.rtfs"

# Run the RTFS compiler to test the plan
echo "Running RTFS compiler on test plan..."
cd /home/mandubian/workspaces/mandubian/ccos/rtfs_compiler
RTFS_CODE='(do (step "Test execution" (call :stdlib.core:println {"message" "Hello from RTFS!"})) (step "Return result" "Execution completed successfully"))'
cargo run --bin rtfs_compiler -- --input string --string "$RTFS_CODE" --execute --show-timing

echo "Test completed. Execution results should be visible in the TUI demo."
