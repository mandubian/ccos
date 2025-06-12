// Simple test runner for enhanced integration tests
mod ast; 
pub mod parser; 
pub mod runtime; 
mod ir; 
mod ir_converter; 
mod ir_optimizer; 
mod integration_tests; 

fn main() {
    println!("🚀 RTFS Enhanced Integration Test Demo");
    println!("=====================================");
    
    // Run just the basic comprehensive tests
    integration_tests::run_comprehensive_integration_tests();
}
