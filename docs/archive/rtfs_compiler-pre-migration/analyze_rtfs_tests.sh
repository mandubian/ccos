#!/bin/bash

echo "Analyzing RTFS-only tests for CCOS dependencies..."

cd tests/rtfs-only

# Check each test file for CCOS imports
for file in *.rs; do
    echo "=== $file ==="
    
    # Check for CCOS imports
    if grep -q "ccos::" "$file"; then
        echo "  ❌ Has CCOS imports - should be moved to shared/"
        grep "ccos::" "$file" | head -3
    fi
    
    # Check for CapabilityMarketplace usage
    if grep -q "CapabilityMarketplace\|StaticDelegationEngine\|RuntimeHost\|CausalChain" "$file"; then
        echo "  ❌ Uses CCOS components - should be moved to shared/"
        grep -E "CapabilityMarketplace|StaticDelegationEngine|RuntimeHost|CausalChain" "$file" | head -2
    fi
    
    # Check for ExecutionOutcome usage (indicates CCOS integration)
    if grep -q "ExecutionOutcome" "$file"; then
        echo "  ❌ Uses ExecutionOutcome - should be moved to shared/"
        grep "ExecutionOutcome" "$file" | head -2
    fi
    
    # Check if it's actually pure RTFS
    if ! grep -q "ccos::\|CapabilityMarketplace\|StaticDelegationEngine\|RuntimeHost\|CausalChain\|ExecutionOutcome" "$file"; then
        echo "  ✅ Pure RTFS test"
    fi
    
    echo ""
done

echo "Analysis complete!"
