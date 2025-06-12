// Simple test runner for integration tests only
// This allows us to see the complete integration test results

mod ast;
pub mod parser;
pub mod runtime;
mod ir;
mod ir_converter;
mod ir_optimizer;
mod integration_tests;

fn main() {
    println!("🚀 RTFS INTEGRATION TEST RUNNER");
    println!("Demonstrating the complete RTFS Source → AST → IR → Optimized IR pipeline");
    println!();
    
    // Run the comprehensive integration tests
    integration_tests::run_comprehensive_integration_tests();
    
    println!();
    
    // Demonstrate the complete pipeline with a complex example
    integration_tests::demonstrate_complete_pipeline();
    
    println!();
    
    // Show performance benchmarks
    integration_tests::benchmark_pipeline_performance();
}
