// CCOS Integration Tests Module
// This module contains tests for CCOS component integration

// Include all CCOS integration test files
mod arbiter_plan_generation_integration;
mod capability_integration_tests;
mod capability_marketplace_tests;
mod capability_system;
mod capability_type_validation_tests;
mod capability_type_validation_tests_fixed;
mod capability_type_validation_tests_original;
mod ccos_context_exposure_tests;
mod ch_working_memory_integration;
mod execution_context_tests;
mod firecracker_enhanced_tests;
mod http_capability_tests;
mod l4_cache_ir_integration;
mod test_helpers; // Re-exports from shared
mod intent_graph_dependency_tests;
mod intent_lifecycle_audit_tests;
mod l4_cache_integration;
mod microvm_central_auth_tests;
mod microvm_performance_tests;
mod microvm_policy_enforcement_tests;
mod microvm_provider_lifecycle_tests;
mod microvm_security_tests;
mod orchestrator_checkpoint_tests;
mod orchestrator_intent_status_tests;
mod rtfs_bridge_tests;
mod runtime_type_integration_tests;
mod test_http_capabilities;
mod test_microvm_http_plan;
mod test_weather_mcp_integration;
mod working_memory_integration;
