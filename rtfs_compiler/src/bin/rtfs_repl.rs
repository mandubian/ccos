// RTFS Interactive REPL with RTFS 2.0 Support
// Interactive read-eval-print loop with RTFS 2.0 object support and validation

use clap::Parser;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::time::Instant;

// Import the RTFS compiler modules
extern crate rtfs_compiler;
use rtfs_compiler::{
    ast::TopLevel, // Add TopLevel for RTFS 2.0 objects
    input_handling::{read_input_content, validate_input_args, InputConfig, InputSource},
    ir::converter::IrConverter, // Fix import path
    ir::enhanced_optimizer::{EnhancedOptimizationPipeline, OptimizationLevel},
    parser::parse_with_enhanced_errors, // Changed from parse_expression to parse for full programs
    runtime::module_runtime::ModuleRegistry,
    runtime::{Runtime, RuntimeStrategy},
    validator::SchemaValidator, // Add schema validation
};

#[derive(Parser)]
#[command(name = "rtfs-repl")]
#[command(about = "RTFS Interactive REPL with multi-source input support")]
struct Args {
    /// Input source type
    #[arg(short, long, value_enum, default_value_t = InputSource::Interactive)]
    input: InputSource,

    /// Input string (when using --input string)
    #[arg(short, long)]
    string: Option<String>,

    /// Input file path (when using --input file)
    #[arg(short, long)]
    file: Option<String>,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();

    if args.verbose {
        println!("🚀 RTFS Interactive REPL with RTFS 2.0 Support");
        println!("Input source: {:?}", args.input);
    }

    // Initialize runtime components
    let module_registry = ModuleRegistry::new();
    let mut runtime_strategy = rtfs_compiler::runtime::ir_runtime::IrStrategy::new(module_registry);

    // Enable persistent environment for REPL usage
    if let Err(e) = runtime_strategy.enable_persistent_env() {
        eprintln!("Failed to enable persistent environment: {:?}", e);
        std::process::exit(1);
    }

    let runtime_strategy: Box<dyn RuntimeStrategy> = Box::new(runtime_strategy);
    let mut runtime = Runtime::new(runtime_strategy);

    let mut ir_converter = IrConverter::new();
    let mut optimizer =
        EnhancedOptimizationPipeline::with_optimization_level(OptimizationLevel::Basic);

    match args.input {
        InputSource::Interactive => {
            run_interactive_repl(&mut runtime, &mut ir_converter, &mut optimizer);
        }
        InputSource::String | InputSource::File | InputSource::Pipe => {
            // Convert args to PathBuf for file path
            let file_path = args.file.map(std::path::PathBuf::from);

            // Validate input arguments
            if let Err(e) = validate_input_args(args.input.clone(), &file_path, &args.string) {
                eprintln!("{}", e);
                std::process::exit(1);
            }

            // Create input configuration
            let input_config = match args.input {
                InputSource::File => {
                    let path = file_path.expect("File path should be validated");
                    InputConfig::from_file(path, args.verbose)
                }
                InputSource::String => {
                    let content = args.string.expect("String content should be validated");
                    InputConfig::from_string(content, args.verbose)
                }
                InputSource::Pipe => InputConfig::from_pipe(args.verbose),
                InputSource::Interactive => unreachable!(),
            };

            // Read input content
            let input_content = match read_input_content(&input_config) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };

            // Process the input
            process_rtfs_input(
                &input_content.content,
                &mut runtime,
                &mut ir_converter,
                &mut optimizer,
            );
        }
    }
}

fn run_interactive_repl(
    runtime: &mut Runtime,
    ir_converter: &mut IrConverter,
    optimizer: &mut EnhancedOptimizationPipeline,
) {
    println!("🚀 RTFS Interactive REPL with RTFS 2.0 Support");
    println!("Type 'help' for commands, 'quit' to exit");
    println!(
        "Supports RTFS 2.0 objects: intents, plans, actions, capabilities, resources, modules"
    );
    println!("💡 Multi-line support: Paste multi-line RTFS code directly!");
    println!("💡 Non-interactive modes:");
    println!("  • rtfs-repl --input string --string \"(intent ...)\"");
    println!("  • rtfs-repl --input file --file input.rtfs");
    println!("  • echo \"(intent ...)\" | rtfs-repl --input pipe");
    println!();

    // Initialize rustyline editor
    let mut rl = Editor::<(), _>::new().expect("Failed to create line editor");

    // Multi-line input buffer
    let mut multi_line_buffer = String::new();
    let mut in_multi_line = false;

    loop {
        let prompt = if in_multi_line { "  " } else { "rtfs> " };

        match rl.readline(prompt) {
            Ok(line) => {
                let line = line.trim();

                // Add to history if not empty
                if !line.is_empty() {
                    let _ = rl.add_history_entry(line);
                }

                // Handle special commands (only when not in multi-line mode)
                if !in_multi_line {
                    match line {
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
                        "reset" => {
                            multi_line_buffer.clear();
                            in_multi_line = false;
                            println!("🔄 Reset multi-line buffer");
                            continue;
                        }
                        _ => {}
                    }
                }

                // Check for multi-line indicators
                if line.is_empty() && in_multi_line {
                    // Empty line in multi-line mode - process the buffer
                    let input = multi_line_buffer.clone();
                    multi_line_buffer.clear();
                    in_multi_line = false;

                    if !input.trim().is_empty() {
                        process_rtfs_input(&input, runtime, ir_converter, optimizer);
                    }
                    continue;
                }

                // Check if this looks like the start of a multi-line construct
                if !in_multi_line
                    && (line.contains('{') || line.contains('(') || line.contains('['))
                {
                    // Count opening/closing brackets to see if we need more lines
                    let open_count = line
                        .chars()
                        .filter(|&c| c == '{' || c == '(' || c == '[')
                        .count();
                    let close_count = line
                        .chars()
                        .filter(|&c| c == '}' || c == ')' || c == ']')
                        .count();

                    if open_count > close_count {
                        in_multi_line = true;
                        multi_line_buffer = line.to_string();
                        continue;
                    }
                }

                if in_multi_line {
                    // Add to multi-line buffer
                    if !multi_line_buffer.is_empty() {
                        multi_line_buffer.push('\n');
                    }
                    multi_line_buffer.push_str(line);

                    // Check if we have balanced brackets
                    let open_count = multi_line_buffer
                        .chars()
                        .filter(|&c| c == '{' || c == '(' || c == '[')
                        .count();
                    let close_count = multi_line_buffer
                        .chars()
                        .filter(|&c| c == '}' || c == ')' || c == ']')
                        .count();

                    if open_count == close_count && open_count > 0 {
                        // Balanced brackets - process the buffer
                        let input = multi_line_buffer.clone();
                        multi_line_buffer.clear();
                        in_multi_line = false;

                        process_rtfs_input(&input, runtime, ir_converter, optimizer);
                    }
                } else {
                    // Single line input
                    if !line.is_empty() {
                        process_rtfs_input(line, runtime, ir_converter, optimizer);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C
                if in_multi_line {
                    println!("🔄 Cancelled multi-line input");
                    multi_line_buffer.clear();
                    in_multi_line = false;
                } else {
                    println!("👋 Goodbye!");
                    break;
                }
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D
                println!("👋 Goodbye!");
                break;
            }
            Err(err) => {
                println!("❌ Error reading input: {}", err);
                break;
            }
        }
    }
}

fn process_rtfs_input(
    input: &str,
    runtime: &mut Runtime,
    ir_converter: &mut IrConverter,
    optimizer: &mut EnhancedOptimizationPipeline,
) {
    let start_time = Instant::now();

    match parse_with_enhanced_errors(input, None) {
        Ok(parsed_items) => {
            if parsed_items.is_empty() {
                println!("⚠️  No content to process");
                return;
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

fn show_help() {
    println!("\n📚 RTFS REPL Commands:");
    println!("  help                    - Show this help");
    println!("  quit, exit              - Exit the REPL");
    println!("  clear                   - Clear the screen");
    println!("  reset                   - Reset multi-line buffer");
    println!("\n💡 Multi-line Support:");
    println!("  • Paste multi-line RTFS code directly");
    println!("  • Press Enter twice to execute multi-line input");
    println!("  • Press Ctrl-C to cancel multi-line input");
    println!("  • Brackets are automatically balanced");
    println!("\n🚀 Non-interactive Usage:");
    println!("  • rtfs-repl --input string --string \"(intent ...)\"");
    println!("  • rtfs-repl --input file --file input.rtfs");
    println!("  • echo \"(intent ...)\" | rtfs-repl --input pipe");
    println!("  • rtfs-repl --input file --file input.rtfs --verbose");
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
