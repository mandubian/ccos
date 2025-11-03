// RTFS Interactive REPL with RTFS 2.0 Support
// Interactive read-eval-print loop with RTFS 2.0 object support and validation

use clap::Parser;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::time::Instant;

// Import the RTFS compiler modules
extern crate rtfs;
use rtfs::{
    ast::TopLevel, // Add TopLevel for RTFS 2.0 objects
    bytecode::BytecodeBackend,
    input_handling::{read_input_content, validate_input_args, InputConfig, InputSource},
    ir::converter::IrConverter, // Fix import path
    ir::enhanced_optimizer::{EnhancedOptimizationPipeline, OptimizationLevel},
    ir::type_checker,
    parser::parse_with_enhanced_errors, // Changed from parse_expression to parse for full programs
    runtime::module_runtime::ModuleRegistry,
    runtime::{Runtime, RuntimeStrategy},
    validator::SchemaValidator, // Add schema validation
};

// REPL state for interactive features
#[derive(Debug, Clone)]
struct ReplState {
    last_input: String,
    last_result: Option<String>,
    last_ir: Option<String>,
    show_types: bool,
    show_ir: bool,
    show_timing: bool,
}

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
    
    if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&module_registry) {
        eprintln!("Warning: Failed to load standard library: {:?}", e);
    }
    
    let module_registry = std::sync::Arc::new(module_registry);
    let mut runtime_strategy = rtfs::runtime::ir_runtime::IrStrategy::new(module_registry);

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

            // Initialize state for non-interactive mode
            let mut state = ReplState {
                last_input: String::new(),
                last_result: None,
                last_ir: None,
                show_types: false,
                show_ir: false,
                show_timing: false,
            };
            
            // Process the input
            process_rtfs_input_with_state(
                &input_content.content,
                &mut runtime,
                &mut ir_converter,
                &mut optimizer,
                &mut state,
            );
        }
    }
}

fn run_interactive_repl(
    runtime: &mut Runtime,
    ir_converter: &mut IrConverter,
    optimizer: &mut EnhancedOptimizationPipeline,
) {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║  🚀 RTFS Interactive REPL v2.0                      ║");
    println!("║  Type-Safe • Interactive • Developer-Friendly       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
    println!("💡 Quick Tips:");
    println!("  • Type expressions and see results instantly");
    println!("  • Use :commands for special features (type :help for list)");
    println!("  • Multi-line: paste code, press Enter twice");
    println!("  • Type checking is ON by default for safety");
    println!();

    // Initialize REPL state
    let mut state = ReplState {
        last_input: String::new(),
        last_result: None,
        last_ir: None,
        show_types: false,
        show_ir: false,
        show_timing: false,
    };

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
                    // Interactive :commands for REPL features
                    if line.starts_with(':') {
                        handle_repl_command(line, &mut state, runtime, ir_converter, optimizer);
                        continue;
                    }
                    
                    // Legacy commands (for backward compatibility)
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
                        process_rtfs_input_with_state(&input, runtime, ir_converter, optimizer, &mut state);
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

                        process_rtfs_input_with_state(&input, runtime, ir_converter, optimizer, &mut state);
                    }
                } else {
                    // Single line input
                    if !line.is_empty() {
                        process_rtfs_input_with_state(line, runtime, ir_converter, optimizer, &mut state);
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

fn process_rtfs_input_with_state(
    input: &str,
    runtime: &mut Runtime,
    ir_converter: &mut IrConverter,
    optimizer: &mut EnhancedOptimizationPipeline,
    state: &mut ReplState,
) {
    // Save input to state
    state.last_input = input.to_string();
    
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
                                // Type check (always on for safety)
                                let type_check_result = type_checker::type_check_ir(&ir_node);
                                
                                // Auto-show type if enabled
                                if state.show_types {
                                    if let Some(ir_type) = ir_node.ir_type() {
                                        println!("🔍 Type: {}", format_type_friendly(ir_type));
                                    }
                                }
                                
                                // Auto-show IR if enabled
                                if state.show_ir {
                                    println!("🔧 IR nodes: {}", count_ir_nodes(&ir_node));
                                }
                                
                                // Optimize
                                let optimized_ir = optimizer.optimize(ir_node);

                                // Execute
                                match runtime.run(expr) {
                                    Ok(value) => {
                                        let elapsed = start_time.elapsed();
                                        
                                        // Nice result display
                                        println!("╭─────────────────────────────────────╮");
                                        print!("│ ✅ Result: ");
                                        state.last_result = Some(format!("{:?}", value));
                                        println!("{:?}", value);
                                        
                                        if state.show_timing {
                                            println!("│ ⏱️  Time: {:?}", elapsed);
                                        }
                                        
                                        if let Err(e) = type_check_result {
                                            println!("│ ⚠️  Type warning: {}", e);
                                        }
                                        
                                        println!("╰─────────────────────────────────────╯");
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

// =============================================================================
// INTERACTIVE REPL COMMANDS
// =============================================================================

/// Handle interactive :commands for user-friendly features
fn handle_repl_command(
    cmd: &str,
    state: &mut ReplState,
    runtime: &mut Runtime,
    ir_converter: &mut IrConverter,
    optimizer: &mut EnhancedOptimizationPipeline,
) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let command = parts[0];
    
    match command {
        ":help" | ":h" => show_help(),
        
        ":type" | ":t" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                show_type_info(&state.last_input, ir_converter);
            }
        }
        
        ":ast" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                show_ast_friendly(&state.last_input);
            }
        }
        
        ":ir" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                show_ir_friendly(&state.last_input, ir_converter, optimizer);
            }
        }
        
        ":explain" | ":e" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                explain_code(&state.last_input, ir_converter);
            }
        }
        
        ":security" | ":sec" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                show_security_info(&state.last_input);
            }
        }
        
        ":info" | ":i" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                show_comprehensive_info(&state.last_input, ir_converter, optimizer);
            }
        }
        
        ":set" => {
            if parts.len() < 3 {
                println!("📝 Current Settings:");
                println!("  show_types: {}", state.show_types);
                println!("  show_ir: {}", state.show_ir);
                println!("  show_timing: {}", state.show_timing);
                println!("\nℹ️  Usage: :set <option> <on|off>");
                println!("  Options: types, ir, timing");
            } else {
                handle_set_command(parts[1], parts[2], state);
            }
        }
        
        ":format" | ":fmt" => {
            if state.last_input.is_empty() {
                println!("ℹ️  No previous expression. Try evaluating something first!");
            } else {
                format_code(&state.last_input);
            }
        }
        
        _ => {
            println!("❓ Unknown command: {}", command);
            println!("💡 Type :help to see all available commands");
        }
    }
}

fn handle_set_command(option: &str, value: &str, state: &mut ReplState) {
    let enabled = matches!(value, "on" | "true" | "1" | "yes");
    
    match option {
        "types" | "type" => {
            state.show_types = enabled;
            println!("✅ Type display: {}", if enabled { "ON" } else { "OFF" });
        }
        "ir" => {
            state.show_ir = enabled;
            println!("✅ IR display: {}", if enabled { "ON" } else { "OFF" });
        }
        "timing" | "time" => {
            state.show_timing = enabled;
            println!("✅ Timing display: {}", if enabled { "ON" } else { "OFF" });
        }
        _ => {
            println!("❓ Unknown option: {}", option);
            println!("💡 Available: types, ir, timing");
        }
    }
}

fn show_help() {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║  📚 RTFS REPL Interactive Commands                  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    
    println!("\n🎯 Quick Commands:");
    println!("  :help, :h              Show this help");
    println!("  :type, :t              Show type of last expression");
    println!("  :ast                   Show Abstract Syntax Tree (visual)");
    println!("  :ir                    Show Intermediate Representation");
    println!("  :explain, :e           Explain what the code does");
    println!("  :security, :sec        Security analysis of code");
    println!("  :info, :i              Comprehensive info (type + IR + timing)");
    println!("  :format, :fmt          Format/prettify last expression");
    
    println!("\n⚙️  Settings:");
    println!("  :set types on/off      Auto-show types after each eval");
    println!("  :set ir on/off         Auto-show IR after each eval");
    println!("  :set timing on/off     Auto-show timing after each eval");
    println!("  :set                   Show current settings");
    
    println!("\n🛠️  Utilities:");
    println!("  help                   Show extended help");
    println!("  quit, exit             Exit the REPL");
    println!("  clear                  Clear the screen");
    println!("  reset                  Reset multi-line buffer");
    
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
    println!("  (+ 1 2 3)");
    println!("  (let [x 5] (* x x))");
    println!("  [1 2.5 3]");
    println!();
    
    println!("💡 Pro Tip: Use :info to see types, IR, and security info all at once!");
    println!();
}

// =============================================================================
// USER-FRIENDLY DISPLAY FUNCTIONS
// =============================================================================

/// Show type information in a user-friendly way
fn show_type_info(input: &str, ir_converter: &mut IrConverter) {
    println!("\n┌─ 🔍 TYPE INFORMATION ─────────────────────────────────┐");
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            for (i, item) in items.iter().enumerate() {
                if let TopLevel::Expression(expr) = item {
                    match ir_converter.convert_expression(expr.clone()) {
                        Ok(ir_node) => {
                            if let Some(ir_type) = ir_node.ir_type() {
                                println!("│ Expression {}: {}", i + 1, format_type_friendly(ir_type));
                                
                                // Add helpful explanation
                                match ir_type {
                                    rtfs::ir::core::IrType::Union(types) if types.len() == 2 
                                        && types.contains(&rtfs::ir::core::IrType::Int)
                                        && types.contains(&rtfs::ir::core::IrType::Float) => {
                                        println!("│ 📘 This is a Number (can be Int or Float)");
                                    }
                                    rtfs::ir::core::IrType::Vector(elem_type) => {
                                        println!("│ 📘 This is a Vector containing: {}", format_type_friendly(elem_type));
                                    }
                                    rtfs::ir::core::IrType::Any => {
                                        println!("│ ⚠️  Type is Any (dynamic - be careful!)");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(e) => println!("│ ❌ Could not infer type: {:?}", e),
                    }
                }
            }
        }
        Err(e) => println!("│ ❌ Parse error: {}", e),
    }
    
    println!("└────────────────────────────────────────────────────────┘\n");
}

/// Show AST in a user-friendly, visual way
fn show_ast_friendly(input: &str) {
    println!("\n┌─ 🌳 SYNTAX TREE (How RTFS Sees Your Code) ───────────┐");
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            for (i, item) in items.iter().enumerate() {
                println!("│");
                println!("│ Expression {}:", i + 1);
                print_ast_tree(item, "│   ", true);
            }
        }
        Err(e) => println!("│ ❌ Parse error: {}", e),
    }
    
    println!("└────────────────────────────────────────────────────────┘\n");
}

fn print_ast_tree(item: &TopLevel, prefix: &str, is_last: bool) {
    use rtfs::ast::Expression;
    
    match item {
        TopLevel::Expression(expr) => {
            match expr {
                Expression::Literal(lit) => {
                    println!("{}└─ 💎 {:?}", prefix, lit);
                }
                Expression::Symbol(s) => {
                    println!("{}└─ 🏷️  {}", prefix, s.0);
                }
                Expression::FunctionCall { callee, arguments } => {
                    println!("{}└─ ⚙️  Function Call", prefix);
                    if let Expression::Symbol(func) = callee.as_ref() {
                        println!("{}   ├─ Function: {}", prefix, func.0);
                    }
                    println!("{}   └─ {} argument(s)", prefix, arguments.len());
                }
                Expression::Vector(items) => {
                    println!("{}└─ 📦 Vector [{} items]", prefix, items.len());
                    for (i, item_expr) in items.iter().enumerate() {
                        let is_last_item = i == items.len() - 1;
                        let new_prefix = format!("{}   {}", prefix, if is_last_item { " " } else { "│" });
                        print_expression_tree(item_expr, &new_prefix, is_last_item);
                    }
                }
                _ => {
                    println!("{}└─ {:?}", prefix, std::mem::discriminant(expr));
                }
            }
        }
        _ => {
            println!("{}└─ RTFS 2.0 Object: {:?}", prefix, std::mem::discriminant(item));
        }
    }
}

fn print_expression_tree(expr: &rtfs::ast::Expression, prefix: &str, is_last: bool) {
    use rtfs::ast::Expression;
    
    let symbol = if is_last { "└─" } else { "├─" };
    
    match expr {
        Expression::Literal(lit) => println!("{}{} 💎 {:?}", prefix, symbol, lit),
        Expression::Symbol(s) => println!("{}{} 🏷️  {}", prefix, symbol, s.0),
        Expression::Vector(items) => println!("{}{} 📦 [{} items]", prefix, symbol, items.len()),
        _ => println!("{}{} {:?}", prefix, symbol, std::mem::discriminant(expr)),
    }
}

/// Show IR in a friendly way with explanations
fn show_ir_friendly(input: &str, ir_converter: &mut IrConverter, optimizer: &mut EnhancedOptimizationPipeline) {
    println!("\n┌─ 🔧 INTERMEDIATE REPRESENTATION (Optimized) ─────────┐");
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            for (i, item) in items.iter().enumerate() {
                if let TopLevel::Expression(expr) = item {
                    match ir_converter.convert_expression(expr.clone()) {
                        Ok(ir_node) => {
                            let optimized = optimizer.optimize(ir_node);
                            println!("│");
                            println!("│ Expression {}: {:#?}", i + 1, optimized);
                        }
                        Err(e) => println!("│ ❌ IR conversion error: {:?}", e),
                    }
                }
            }
        }
        Err(e) => println!("│ ❌ Parse error: {}", e),
    }
    
    println!("└────────────────────────────────────────────────────────┘\n");
}

/// Explain what the code does in plain language
fn explain_code(input: &str, ir_converter: &mut IrConverter) {
    println!("\n┌─ 💭 CODE EXPLANATION ────────────────────────────────┐");
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            for (i, item) in items.iter().enumerate() {
                if let TopLevel::Expression(expr) = item {
                    println!("│");
                    println!("│ Expression {}:", i + 1);
                    explain_expression(expr, "│   ");
                    
                    // Show inferred type
                    if let Ok(ir_node) = ir_converter.convert_expression(expr.clone()) {
                        if let Some(ir_type) = ir_node.ir_type() {
                            println!("│");
                            println!("│ 📘 Type: {}", format_type_friendly(ir_type));
                        }
                    }
                }
            }
        }
        Err(e) => println!("│ ❌ Parse error: {}", e),
    }
    
    println!("└────────────────────────────────────────────────────────┘\n");
}

fn explain_expression(expr: &rtfs::ast::Expression, prefix: &str) {
    use rtfs::ast::Expression;
    
    match expr {
        Expression::Literal(lit) => {
            println!("{}This is a literal value: {:?}", prefix, lit);
        }
        Expression::Symbol(s) => {
            println!("{}This references the variable or function: {}", prefix, s.0);
        }
        Expression::FunctionCall { callee, arguments } => {
            if let Expression::Symbol(func) = callee.as_ref() {
                match func.0.as_str() {
                    "+" => println!("{}This adds {} numbers together", prefix, arguments.len()),
                    "-" => println!("{}This subtracts numbers", prefix),
                    "*" => println!("{}This multiplies {} numbers", prefix, arguments.len()),
                    "/" => println!("{}This divides numbers", prefix),
                    _ => println!("{}This calls the function: {}", prefix, func.0),
                }
                
                if !arguments.is_empty() {
                    println!("{}With {} argument(s)", prefix, arguments.len());
                }
            }
        }
        Expression::Vector(items) => {
            println!("{}This is a vector with {} elements", prefix, items.len());
            if !items.is_empty() {
                println!("{}Elements: {:?}...", prefix, items.first());
            }
        }
        _ => {
            println!("{}This is a {} expression", prefix, get_expr_type_name(expr));
        }
    }
}

fn get_expr_type_name(expr: &rtfs::ast::Expression) -> &'static str {
    use rtfs::ast::Expression;
    match expr {
        Expression::If(_) => "conditional (if)",
        Expression::Let(_) => "let binding",
        Expression::Do(_) => "do block",
        Expression::Fn(_) => "function definition",
        Expression::Match(_) => "pattern match",
        Expression::Map(_) => "map/dictionary",
        Expression::List(_) => "list",
        _ => "complex",
    }
}

/// Show security analysis in a user-friendly way
fn show_security_info(input: &str) {
    println!("\n┌─ 🔒 SECURITY ANALYSIS ───────────────────────────────┐");
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            let mut has_file_ops = false;
            let mut has_network_ops = false;
            let mut capabilities = Vec::new();
            
            for item in &items {
                if let TopLevel::Expression(expr) = item {
                    scan_for_capabilities(expr, &mut capabilities, &mut has_file_ops, &mut has_network_ops);
                }
            }
            
            if capabilities.is_empty() {
                println!("│ ✅ Safe: No external operations detected");
                println!("│ 📘 This code is pure (no side effects)");
            } else {
                println!("│ ⚠️  External Operations Detected:");
                for cap in &capabilities {
                    println!("│   • {}", cap);
                }
                
                println!("│");
                println!("│ 🔐 Security Level:");
                if has_network_ops {
                    println!("│   Sandboxed (requires MicroVM)");
                    println!("│   📘 Network operations need strict isolation");
                } else if has_file_ops {
                    println!("│   Controlled (file access monitored)");
                } else {
                    println!("│   Basic (minimal risk)");
                }
            }
        }
        Err(e) => println!("│ ❌ Parse error: {}", e),
    }
    
    println!("└────────────────────────────────────────────────────────┘\n");
}

fn scan_for_capabilities(
    expr: &rtfs::ast::Expression,
    capabilities: &mut Vec<String>,
    has_file_ops: &mut bool,
    has_network_ops: &mut bool,
) {
    use rtfs::ast::Expression;
    
    match expr {
        Expression::FunctionCall { callee, arguments } => {
            if let Expression::Symbol(func) = callee.as_ref() {
                match func.0.as_str() {
                    "read-file" | "write-file" | "delete-file" => {
                        capabilities.push(format!("File I/O: {}", func.0));
                        *has_file_ops = true;
                    }
                    "http-fetch" | "fetch" => {
                        capabilities.push(format!("Network: {}", func.0));
                        *has_network_ops = true;
                    }
                    _ => {}
                }
            }
            for arg in arguments {
                scan_for_capabilities(arg, capabilities, has_file_ops, has_network_ops);
            }
        }
        Expression::Vector(items) | Expression::List(items) => {
            for item in items {
                scan_for_capabilities(item, capabilities, has_file_ops, has_network_ops);
            }
        }
        _ => {}
    }
}

/// Show comprehensive information (type + IR + security + timing)
fn show_comprehensive_info(input: &str, ir_converter: &mut IrConverter, optimizer: &mut EnhancedOptimizationPipeline) {
    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║  📊 COMPREHENSIVE CODE ANALYSIS                     ║");
    println!("╚══════════════════════════════════════════════════════╝");
    
    let start = Instant::now();
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            for (i, item) in items.iter().enumerate() {
                if let TopLevel::Expression(expr) = item {
                    println!("\n🔸 Expression {}:", i + 1);
                    println!("  Input: {}", truncate_string(input, 60));
                    
                    // Type information
                    match ir_converter.convert_expression(expr.clone()) {
                        Ok(ir_node) => {
                            if let Some(ir_type) = ir_node.ir_type() {
                                println!("\n  🔍 Type: {}", format_type_friendly(ir_type));
                                
                                // Type check
                                match type_checker::type_check_ir(&ir_node) {
                                    Ok(_) => println!("  ✅ Type Check: PASS"),
                                    Err(e) => println!("  ❌ Type Check: FAIL - {}", e),
                                }
                            }
                            
                            // Complexity
                            let optimized = optimizer.optimize(ir_node.clone());
                            println!("\n  📈 Complexity:");
                            println!("    IR nodes: {} → {} (after optimization)", count_ir_nodes(&ir_node), count_ir_nodes(&optimized));
                        }
                        Err(e) => println!("  ❌ IR error: {:?}", e),
                    }
                    
                    // Security quick check
                    let mut caps = Vec::new();
                    let mut file_ops = false;
                    let mut net_ops = false;
                    scan_for_capabilities(expr, &mut caps, &mut file_ops, &mut net_ops);
                    
                    println!("\n  🔒 Security:");
                    if caps.is_empty() {
                        println!("    ✅ Pure (no external operations)");
                    } else {
                        println!("    ⚠️  {} external operation(s)", caps.len());
                        for cap in &caps {
                            println!("      • {}", cap);
                        }
                    }
                }
            }
            
            println!("\n  ⏱️  Analysis Time: {:?}", start.elapsed());
        }
        Err(e) => println!("❌ Parse error: {}", e),
    }
    
    println!();
}

/// Format code in a user-friendly way
fn format_code(input: &str) {
    println!("\n┌─ ✨ FORMATTED CODE ──────────────────────────────────┐");
    
    match parse_with_enhanced_errors(input, None) {
        Ok(items) => {
            for item in &items {
                println!("│ {}", format_toplevel_friendly(item));
            }
        }
        Err(e) => println!("│ ❌ Parse error: {}", e),
    }
    
    println!("└────────────────────────────────────────────────────────┘\n");
}

// Helper functions for user-friendly formatting

fn format_type_friendly(ir_type: &rtfs::ir::core::IrType) -> String {
    use rtfs::ir::core::IrType;
    
    match ir_type {
        IrType::Int => "Integer".to_string(),
        IrType::Float => "Float".to_string(),
        IrType::String => "String".to_string(),
        IrType::Bool => "Boolean".to_string(),
        IrType::Any => "Any (dynamic)".to_string(),
        IrType::Union(types) if types.len() == 2 
            && types.contains(&IrType::Int)
            && types.contains(&IrType::Float) => {
            "Number (Int or Float)".to_string()
        }
        IrType::Union(types) => {
            let type_strs: Vec<String> = types.iter().map(|t| format_type_friendly(t)).collect();
            format!("One of: {}", type_strs.join(", "))
        }
        IrType::Vector(elem_type) => {
            format!("Vector of {}", format_type_friendly(elem_type))
        }
        IrType::Function { param_types, return_type, .. } => {
            let params: Vec<String> = param_types.iter().map(|t| format_type_friendly(t)).collect();
            format!("Function ({}) → {}", params.join(", "), format_type_friendly(return_type))
        }
        _ => format!("{:?}", ir_type),
    }
}

fn format_toplevel_friendly(item: &TopLevel) -> String {
    match item {
        TopLevel::Expression(expr) => format_expression_friendly(expr, 0),
        TopLevel::Intent(i) => format!("Intent: {}", i.name),
        TopLevel::Plan(p) => format!("Plan: {}", p.name),
        TopLevel::Action(a) => format!("Action: {}", a.name),
        TopLevel::Capability(c) => format!("Capability: {}", c.name),
        TopLevel::Resource(r) => format!("Resource: {}", r.name),
        TopLevel::Module(m) => format!("Module: {}", m.name),
    }
}

fn format_expression_friendly(expr: &rtfs::ast::Expression, indent: usize) -> String {
    use rtfs::ast::Expression;
    let ind = "  ".repeat(indent);
    
    match expr {
        Expression::Literal(lit) => format!("{:?}", lit),
        Expression::Symbol(s) => s.0.clone(),
        Expression::FunctionCall { callee, arguments } => {
            if let Expression::Symbol(func) = callee.as_ref() {
                let args_str: Vec<String> = arguments.iter().map(|a| format_expression_friendly(a, 0)).collect();
                format!("({} {})", func.0, args_str.join(" "))
            } else {
                format!("(call ...)")
            }
        }
        Expression::Vector(items) => {
            let items_str: Vec<String> = items.iter().map(|i| format_expression_friendly(i, 0)).collect();
            format!("[{}]", items_str.join(" "))
        }
        _ => format!("{:?}", std::mem::discriminant(expr)),
    }
}

fn count_ir_nodes(node: &rtfs::ir::core::IrNode) -> usize {
    use rtfs::ir::core::IrNode;
    
    match node {
        IrNode::Apply { function, arguments, .. } => {
            1 + count_ir_nodes(function) + arguments.iter().map(count_ir_nodes).sum::<usize>()
        }
        IrNode::Vector { elements, .. } => {
            1 + elements.iter().map(count_ir_nodes).sum::<usize>()
        }
        _ => 1,
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[0..max_len])
    }
}
