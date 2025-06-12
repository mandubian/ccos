// Test runner for integration tests only
mod ast; 
pub mod parser; 
pub mod runtime; 
mod ir; 
mod ir_converter; 
mod ir_optimizer; 
mod integration_tests; 

use integration_tests::*;

fn main() {
    println!("🌟 ENHANCED RTFS INTEGRATION TEST SUITE DEMO");
    println!("=============================================\n");
    
    println!("Running basic comprehensive tests...");
    run_comprehensive_integration_tests();
    
    println!("\n" + &"=".repeat(80));
    println!("Running advanced integration tests...");
    run_advanced_integration_tests();
    
    println!("\n" + &"=".repeat(80));
    println!("Running error case tests...");
    run_error_case_tests();
    
    println!("\n✅ Enhanced integration test suite demonstration completed!");
}
