#!/bin/bash

# Comprehensive script to fix remaining test file errors

echo "Fixing remaining test file errors..."

# 1. Fix remaining Evaluator::new calls with 4 arguments (remove delegation_engine)
echo "1. Fixing Evaluator::new calls with 4 arguments..."
find tests/ -name "*.rs" -exec sed -i 's|Evaluator::new(\([^,]*\), \([^,]*\), \([^,]*\), \([^)]*\))|Evaluator::new(\1, \3, \4)|g' {} \;

# 2. Fix remaining IrRuntime::new_compat calls
echo "2. Fixing IrRuntime::new_compat calls..."
find tests/ -name "*.rs" -exec sed -i 's|IrRuntime::new_compat(\([^)]*\))|IrRuntime::new(host, security_context)|g' {} \;

# 3. Fix remaining import issues
echo "3. Fixing remaining import issues..."
find tests/ -name "*.rs" -exec sed -i 's|use rtfs_compiler::runtime::capability_marketplace::|use rtfs_compiler::ccos::capability_marketplace::|g' {} \;
find tests/ -name "*.rs" -exec sed -i 's|use rtfs_compiler::runtime::host::|use rtfs_compiler::ccos::host::|g' {} \;
find tests/ -name "*.rs" -exec sed -i 's|use rtfs_compiler::runtime::ccos_environment::|use rtfs_compiler::ccos::environment::|g' {} \;

# 4. Fix remaining ExecutionOutcome comparison issues
echo "4. Fixing remaining ExecutionOutcome comparison issues..."
find tests/ -name "*.rs" -exec sed -i 's|assert_eq!(result, Value::\([^)]*\));|match result { rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(Value::\1) => {}, _ => panic!("Expected Complete result") };|g' {} \;

# 5. Remove unused delegation_engine variables
echo "5. Removing unused delegation_engine variables..."
find tests/ -name "*.rs" -exec sed -i '/let.*delegation_engine.*=/d' {} \;

# 6. Fix remaining method calls that expect different signatures
echo "6. Fixing method signature issues..."
find tests/ -name "*.rs" -exec sed -i 's|\.evaluate_with_env(&expr, &mut env)\.expect|\.evaluate(&expr)\.expect|g' {} \;

echo "Remaining test errors fixes completed!"
