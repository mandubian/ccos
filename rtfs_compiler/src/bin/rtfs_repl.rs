// RTFS Interactive REPL with RTFS 2.0 Support
// Interactive read-eval-print loop with RTFS 2.0 object support and validation

use std::io::{self, Write};
use std::time::Instant;

// Import the RTFS compiler modules
extern crate rtfs_compiler;
use rtfs_compiler::{
    agent::discovery_traits::NoOpAgentDiscovery,
    ast::TopLevel,              // Add TopLevel for RTFS 2.0 objects
    ir::converter::IrConverter, // Fix import path
    ir::enhanced_optimizer::{EnhancedOptimizationPipeline, OptimizationLevel},
    parser::parse_with_enhanced_errors, // Changed from parse_expression to parse for full programs
    runtime::module_runtime::ModuleRegistry,
    runtime::{Runtime, RuntimeStrategy},
    validator::SchemaValidator, // Add schema validation
};

fn main() {
    println!("🚀 RTFS Interactive REPL with RTFS 2.0 Support");
    println!("Type 'help' for commands, 'quit' to exit");
    println!(
        "Supports RTFS 2.0 objects: intents, plans, actions, capabilities, resources, modules"
    );
    println!();

    // Initialize runtime components
    let module_registry = ModuleRegistry::new();
    let runtime_strategy: Box<dyn RuntimeStrategy> = Box::new(
        rtfs_compiler::runtime::ir_runtime::IrStrategy::new(module_registry),
    );
    let mut runtime = Runtime::new(runtime_strategy);

    let mut ir_converter = IrConverter::new();
    let mut optimizer =
        EnhancedOptimizationPipeline::with_optimization_level(OptimizationLevel::Basic);

    loop {
        print!("rtfs> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            println!("Error reading input");
            continue;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Handle special commands
        match input {
            "quit" | "exit" => {
                println!("👋 Goodbye!");
                break;
            }
            "help" => {
                show_help();
                continue;
            }
            "clear" => {
                print!("\x1B[2J\x1B[1;1H"); // Clear screen
                continue;
            }
            _ => {}
        }

        // Process RTFS input
        let start_time = Instant::now();

        match parse_with_enhanced_errors(input, None) {
            Ok(parsed_items) => {
                if parsed_items.is_empty() {
                    println!("⚠️  No content to process");
                    continue;
                }

                println!("📊 Parsed {} top-level items", parsed_items.len());

                // Process each top-level item
                for (i, item) in parsed_items.iter().enumerate() {
                    println!(
                        "\n🔄 Processing item {}: {:?}",
                        i + 1,
                        std::mem::discriminant(item)
                    );

                    match item {
                        TopLevel::Expression(expr) => {
                            // Convert to IR
                            match ir_converter.convert_expression(expr.clone()) {
                                Ok(ir_node) => {
                                    // Optimize
                                    let optimized_ir = optimizer.optimize(ir_node);

                                    // Execute
                                    match runtime.run(expr) {
                                        Ok(value) => {
                                            let elapsed = start_time.elapsed();
                                            println!("✅ Result: {:?} (took {:?})", value, elapsed);
                                        }
                                        Err(e) => {
                                            println!("❌ Runtime error: {:?}", e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("❌ IR conversion error: {:?}", e);
                                }
                            }
                        }
                        TopLevel::Intent(intent) => {
                            // Validate intent
                            match SchemaValidator::validate_object(item) {
                                Ok(_) => {
                                    println!("✅ Valid intent: {:?}", intent.name);
                                    println!("   Properties: {:?}", intent.properties);
                                }
                                Err(e) => {
                                    println!("❌ Invalid intent: {:?}", e);
                                }
                            }
                        }
                        TopLevel::Plan(plan) => {
                            // Validate plan
                            match SchemaValidator::validate_object(item) {
                                Ok(_) => {
                                    println!("✅ Valid plan: {:?}", plan.name);
                                    println!("   Properties: {:?}", plan.properties);
                                }
                                Err(e) => {
                                    println!("❌ Invalid plan: {:?}", e);
                                }
                            }
                        }
                        TopLevel::Action(action) => {
                            // Validate action
                            match SchemaValidator::validate_object(item) {
                                Ok(_) => {
                                    println!("✅ Valid action: {:?}", action.name);
                                    println!("   Properties: {:?}", action.properties);
                                }
                                Err(e) => {
                                    println!("❌ Invalid action: {:?}", e);
                                }
                            }
                        }
                        TopLevel::Capability(capability) => {
                            // Validate capability
                            match SchemaValidator::validate_object(item) {
                                Ok(_) => {
                                    println!("✅ Valid capability: {:?}", capability.name);
                                    println!("   Properties: {:?}", capability.properties);
                                }
                                Err(e) => {
                                    println!("❌ Invalid capability: {:?}", e);
                                }
                            }
                        }
                        TopLevel::Resource(resource) => {
                            // Validate resource
                            match SchemaValidator::validate_object(item) {
                                Ok(_) => {
                                    println!("✅ Valid resource: {:?}", resource.name);
                                    println!("   Properties: {:?}", resource.properties);
                                }
                                Err(e) => {
                                    println!("❌ Invalid resource: {:?}", e);
                                }
                            }
                        }
                        TopLevel::Module(module) => {
                            // Validate module
                            match SchemaValidator::validate_object(item) {
                                Ok(_) => {
                                    println!("✅ Valid module: {:?}", module.name);
                                    println!("   Exports: {:?}", module.exports);
                                }
                                Err(e) => {
                                    println!("❌ Invalid module: {:?}", e);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                println!("{}", e);
            }
        }
    }
}

fn show_help() {
    println!("\n📚 RTFS REPL Commands:");
    println!("  help                    - Show this help");
    println!("  quit, exit              - Exit the REPL");
    println!("  clear                   - Clear the screen");
    println!("\n📝 RTFS 2.0 Object Examples:");
    println!("  intent my-intent {{");
    println!("    name: \"my-intent\"");
    println!("    description: \"My intent description\"");
    println!("  }}");
    println!("\n  plan my-plan {{");
    println!("    name: \"my-plan\"");
    println!("    description: \"My plan description\"");
    println!("    steps: [\"step1\", \"step2\"]");
    println!("  }}");
    println!("\n  action my-action {{");
    println!("    name: \"my-action\"");
    println!("    description: \"My action description\"");
    println!("  }}");
    println!("\n  capability my-capability {{");
    println!("    name: \"my-capability\"");
    println!("    description: \"My capability description\"");
    println!("  }}");
    println!("\n  resource my-resource {{");
    println!("    name: \"my-resource\"");
    println!("    resource_type: \"file\"");
    println!("  }}");
    println!("\n  module my-module {{");
    println!("    name: \"my-module\"");
    println!("    version: \"1.0.0\"");
    println!("  }}");
    println!("\n🔢 Expression Examples:");
    println!("  1 + 2 * 3");
    println!("  let x = 5 in x * x");
    println!("  map (fn x => x * 2) [1, 2, 3]");
    println!();
}
