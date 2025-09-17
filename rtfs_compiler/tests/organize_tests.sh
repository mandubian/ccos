#!/bin/bash

# RTFS-CCOS Test Organization Script
# This script organizes test files into appropriate directories based on their content

echo "Organizing RTFS-CCOS test files..."

# RTFS-only tests (pure RTFS functionality, no CCOS dependencies)
RTFS_ONLY_TESTS=(
    "parser.rs"
    "type_system_tests.rs"
    "ast_coverage.rs"
    "debug_parse_failures.rs"
    "enhanced_error_reporting.rs"
    "get_shorthand.rs"
    "test_comment_preprocessing.rs"
    "test_qualified_symbols.rs"
    "test_type_annotation_whitespace.rs"
    "set_form_tests.rs"
    "test_implemented_functions.rs"
    "test_missing_stdlib_functions.rs"
    "test_simple_recursion.rs"
    "test_simple_recursion_new.rs"
    "test_recursive_patterns.rs"
    "ir_language_coverage.rs"
    "ir_optimization.rs"
    "ir_step_params_tests.rs"
    "ir_step_params_additional_tests.rs"
    "l4_cache_ir_integration.rs"
    "realistic_model_tests.rs"
    "secure_stdlib_comprehensive_tests.rs"
    "simple_secure_stdlib_test.rs"
    "stdlib_e2e_tests.rs"
    "test_helpers.rs"
)

# CCOS integration tests (tests that use CCOS components)
CCOS_INTEGRATION_TESTS=(
    "capability_integration_tests.rs"
    "capability_system.rs"
    "capability_type_validation_tests.rs"
    "capability_type_validation_tests_fixed.rs"
    "capability_type_validation_tests_original.rs"
    "ccos_context_exposure_tests.rs"
    "intent_graph_dependency_tests.rs"
    "intent_lifecycle_audit_tests.rs"
    "arbiter_plan_generation_integration.rs"
    "ch_working_memory_integration.rs"
    "orchestrator_checkpoint_tests.rs"
    "orchestrator_intent_status_tests.rs"
    "rtfs_bridge_tests.rs"
    "runtime_type_integration_tests.rs"
    "working_memory_integration.rs"
    "execution_context_tests.rs"
    "microvm_central_auth_tests.rs"
    "microvm_performance_tests.rs"
    "microvm_policy_enforcement_tests.rs"
    "microvm_provider_lifecycle_tests.rs"
    "microvm_security_tests.rs"
    "firecracker_enhanced_tests.rs"
    "l4_cache_integration.rs"
    "test_http_capabilities.rs"
    "test_microvm_http_plan.rs"
    "test_weather_mcp_integration.rs"
    "http_capability_tests"
)

# Shared tests (tests that might use both RTFS and CCOS)
SHARED_TESTS=(
    "basic_validation.rs"
    "readme_scenario_test.rs"
    "e2e_features.rs"
    "integration_tests.rs"
    "rtfs_config_integration_tests.rs"
    "skip_compile_time_verified_tests.rs"
    "step_params_additional.rs"
    "step_params_integration.rs"
    "step_parallel_merge_tests.rs"
    "set_form_integration.rs"
    "test_execution_context_fix.rs"
    "checkpoint_resume_tests.rs"
    "test_issue_43_completion.rs"
    "rtfs_plan_generation.rs"
    "resources"
    "rtfs_files"
    "stdlib"
    "orchestration_primitives_test.rtfs"
)

# Move RTFS-only tests
echo "Moving RTFS-only tests..."
for test in "${RTFS_ONLY_TESTS[@]}"; do
    if [ -f "$test" ]; then
        echo "  Moving $test to rtfs-only/"
        mv "$test" rtfs-only/
    elif [ -d "$test" ]; then
        echo "  Moving directory $test to rtfs-only/"
        mv "$test" rtfs-only/
    else
        echo "  Warning: $test not found"
    fi
done

# Move CCOS integration tests
echo "Moving CCOS integration tests..."
for test in "${CCOS_INTEGRATION_TESTS[@]}"; do
    if [ -f "$test" ]; then
        echo "  Moving $test to ccos-integration/"
        mv "$test" ccos-integration/
    elif [ -d "$test" ]; then
        echo "  Moving directory $test to ccos-integration/"
        mv "$test" ccos-integration/
    else
        echo "  Warning: $test not found"
    fi
done

# Move shared tests
echo "Moving shared tests..."
for test in "${SHARED_TESTS[@]}"; do
    if [ -f "$test" ]; then
        echo "  Moving $test to shared/"
        mv "$test" shared/
    elif [ -d "$test" ]; then
        echo "  Moving directory $test to shared/"
        mv "$test" shared/
    else
        echo "  Warning: $test not found"
    fi
done

echo "Test organization complete!"
echo ""
echo "Directory structure:"
echo "  rtfs-only/        - Pure RTFS tests (no CCOS dependencies)"
echo "  ccos-integration/ - CCOS component integration tests"
echo "  shared/           - Tests that use both RTFS and CCOS"
echo ""
echo "Remaining files in root:"
ls -la *.rs 2>/dev/null || echo "  (no .rs files remaining)"
ls -la *.rtfs 2>/dev/null || echo "  (no .rtfs files remaining)"
