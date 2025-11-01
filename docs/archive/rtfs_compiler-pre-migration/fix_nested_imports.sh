#!/bin/bash

# Script to fix nested import path issues in integration tests

echo "Fixing nested import path issues..."

# Find all test files and fix nested import issues
find tests/ -name "*.rs" -exec grep -l "runtime::capabilities\|runtime::host\|runtime::capability_marketplace" {} \; | while read file; do
    echo "Fixing nested imports in $file..."
    
    # Fix nested import structures
    sed -i 's|runtime::capabilities::|ccos::capabilities::|g' "$file"
    sed -i 's|runtime::host::|ccos::host::|g' "$file"
    sed -i 's|runtime::capability_marketplace::|ccos::capability_marketplace::|g' "$file"
done

echo "Done fixing nested imports!"
