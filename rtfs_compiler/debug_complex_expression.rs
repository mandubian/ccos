use rtfs_compiler::*;

fn main() {
    let content = "(* (+ (* 5 4) (/ 20 2)) (- (+ 3 7) (* 2 2)))";
    
    // Parse the expression
    let parsed = parser::parse_expression(content).unwrap();
    println!("Parsed AST: {:#?}", parsed);
    
    // Convert to IR
    let mut converter = ir_converter::IrConverter::new();
    let ir_node = converter.convert_expression(parsed).unwrap();
    println!("IR Node: {:#?}", ir_node);
    
    // Try to execute with IR runtime
    let agent_discovery = Box::new(agent::discovery_traits::NoOpAgentDiscovery);
    let mut runtime = runtime::Runtime::with_strategy_and_agent_discovery(
        runtime::RuntimeStrategy::Ir,
        agent_discovery
    );
    
    match runtime.evaluate_ir(&ir_node) {
        Ok(value) => println!("Result: {:?}", value),
        Err(e) => println!("Error: {:?}", e),
    }
}
