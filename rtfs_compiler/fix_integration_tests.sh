#!/bin/bash

# Script to fix common import path issues in integration tests

echo "Fixing integration test import paths..."

# Find all test files that need fixing
find tests/ -name "*.rs" -exec grep -l "runtime::capabilities\|runtime::host\|runtime::capability_marketplace\|ccos::delegation::StaticDelegationEngine" {} \; | while read file; do
    echo "Fixing $file..."
    
    # Fix StaticDelegationEngine import
    sed -i 's|use rtfs_compiler::ccos::delegation::StaticDelegationEngine;|use rtfs_compiler::runtime::delegation::StaticDelegationEngine;|g' "$file"
    
    # Fix capabilities import
    sed -i 's|use rtfs_compiler::runtime::capabilities::|use rtfs_compiler::ccos::capabilities::|g' "$file"
    
    # Fix host import
    sed -i 's|use rtfs_compiler::runtime::host::|use rtfs_compiler::ccos::host::|g' "$file"
    
    # Fix capability_marketplace import
    sed -i 's|use rtfs_compiler::runtime::capability_marketplace::|use rtfs_compiler::ccos::capability_marketplace::|g' "$file"
    
    # Fix StaticDelegationEngine::new calls
    sed -i 's|StaticDelegationEngine::new(HashMap::new())|StaticDelegationEngine::new_empty()|g' "$file"
    sed -i 's|StaticDelegationEngine::new(std::collections::HashMap::new())|StaticDelegationEngine::new_empty()|g' "$file"
    
    # Fix runtime path references in code
    sed -i 's|rtfs_compiler::runtime::capabilities::|rtfs_compiler::ccos::capabilities::|g' "$file"
    sed -i 's|rtfs_compiler::runtime::host::|rtfs_compiler::ccos::host::|g' "$file"
    sed -i 's|rtfs_compiler::runtime::capability_marketplace::|rtfs_compiler::ccos::capability_marketplace::|g' "$file"
done

echo "Done fixing import paths!"
