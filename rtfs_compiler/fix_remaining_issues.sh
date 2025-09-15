#!/bin/bash

# Script to fix remaining import path issues in integration tests

echo "Fixing remaining import path issues..."

# Find all test files and fix remaining issues
find tests/ -name "*.rs" -exec grep -l "ccos::delegation::StaticDelegationEngine" {} \; | while read file; do
    echo "Fixing remaining delegation issues in $file..."
    
    # Fix remaining StaticDelegationEngine references
    sed -i 's|rtfs_compiler::ccos::delegation::StaticDelegationEngine|rtfs_compiler::runtime::delegation::StaticDelegationEngine|g' "$file"
done

# Also fix any remaining runtime path references
find tests/ -name "*.rs" -exec grep -l "runtime::capabilities\|runtime::host\|runtime::capability_marketplace" {} \; | while read file; do
    echo "Fixing remaining runtime path issues in $file..."
    
    # Fix any remaining runtime path references in code
    sed -i 's|rtfs_compiler::runtime::capabilities::|rtfs_compiler::ccos::capabilities::|g' "$file"
    sed -i 's|rtfs_compiler::runtime::host::|rtfs_compiler::ccos::host::|g' "$file"
    sed -i 's|rtfs_compiler::runtime::capability_marketplace::|rtfs_compiler::ccos::capability_marketplace::|g' "$file"
done

echo "Done fixing remaining issues!"
