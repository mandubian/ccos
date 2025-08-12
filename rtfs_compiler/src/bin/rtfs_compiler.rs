// RTFS Production Compiler Binary
// Command-line RTFS compiler with RTFS 2.0 support, optimization levels and performance reporting

use clap::{Parser, ValueEnum};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use std::sync::Arc;
use std::cell::RefCell;
use rtfs_compiler::runtime::host::RuntimeHost;

// Import the RTFS compiler modules
// Note: We need to reference the parent crate since this is a binary
extern crate rtfs_compiler;
use rtfs_compiler::{
    ast::TopLevel,
    input_handling::{InputConfig, InputSource, read_input_content, validate_input_args},
    ir::converter::IrConverter,
    ir::enhanced_optimizer::{EnhancedOptimizationPipeline, OptimizationLevel},
    parser::parse_with_enhanced_errors,
    runtime::module_runtime::ModuleRegistry,
    runtime::{Runtime, RuntimeStrategy},
    validator::SchemaValidator,
};

#[derive(Parser)]
#[command(name = "rtfs-compiler")]
#[command(about = "RTFS Production Compiler with RTFS 2.0 Support and Advanced Optimization")]
#[command(version = "0.1.0")]
struct Args {
    /// Input source type
    #[arg(short = 'i', long, value_enum, default_value_t = InputSource::File)]
    input: InputSource,

    /// Input RTFS source file (when using --input file)
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    file: Option<PathBuf>,

    /// Input string (when using --input string)
    #[arg(short = 's', long = "string")]
    string: Option<String>,

    /// Input RTFS source file (positional argument, alternative to --file)
    #[arg(value_name = "FILE", conflicts_with = "file")]
    input_file: Option<PathBuf>,

    /// Output file (optional, defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Optimization level
    #[arg(long, default_value = "aggressive")]
    opt_level: OptLevel,

    /// Runtime strategy for execution
    #[arg(long, default_value = "ir")]
    runtime: RuntimeType,

    /// Show optimization statistics
    #[arg(long)]
    show_stats: bool,

    /// Generate optimization report
    #[arg(long)]
    optimization_report: bool,

    /// Show compilation timing information
    #[arg(long)]
    show_timing: bool,

    /// Execute the compiled code (instead of just compiling)
    #[arg(long)]
    execute: bool,

    /// Validate RTFS 2.0 objects against schemas
    #[arg(long, default_value = "true")]
    validate: bool,

    /// Skip validation (for debugging)
    #[arg(long)]
    skip_validation: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Clone, ValueEnum, Debug)]
enum OptLevel {
    None,
    Basic,
    Aggressive,
}

#[derive(Clone, ValueEnum)]
enum RuntimeType {
    Ast,
    Ir,
    Fallback,
}

impl From<OptLevel> for OptimizationLevel {
    fn from(level: OptLevel) -> Self {
        match level {
            OptLevel::None => OptimizationLevel::None,
            OptLevel::Basic => OptimizationLevel::Basic,
            OptLevel::Aggressive => OptimizationLevel::Aggressive,
        }
    }
}

impl From<RuntimeType> for Box<dyn RuntimeStrategy> {
    fn from(runtime_type: RuntimeType) -> Self {
        match runtime_type {
            RuntimeType::Ast => {
                let mut module_registry = ModuleRegistry::new();
                // Load standard library
                if let Err(e) = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry) {
                    eprintln!("Warning: Failed to load standard library: {:?}", e);
                }
                let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
                let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
                
                let causal_chain = Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
                let host = std::sync::Arc::new(RuntimeHost::new(
                    causal_chain,
                    capability_marketplace,
                    rtfs_compiler::runtime::security::RuntimeContext::full(),
                ));
                let evaluator =
                    rtfs_compiler::runtime::Evaluator::new(
                        std::rc::Rc::new(module_registry),
                        std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())),
                        rtfs_compiler::runtime::security::RuntimeContext::full(),
                        host,
                    );
                Box::new(rtfs_compiler::runtime::TreeWalkingStrategy::new(evaluator))
            }
            RuntimeType::Ir => {
                let mut module_registry = ModuleRegistry::new();
                // Load standard library
                if let Err(e) = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry) {
                    eprintln!("Warning: Failed to load standard library: {:?}", e);
                }
                Box::new(rtfs_compiler::runtime::ir_runtime::IrStrategy::new(
                    module_registry,
                ))
            }
            RuntimeType::Fallback => {
                let mut module_registry = ModuleRegistry::new();
                // Load standard library
                if let Err(e) = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry) {
                    eprintln!("Warning: Failed to load standard library: {:?}", e);
                }
                Box::new(rtfs_compiler::runtime::IrWithFallbackStrategy::new(
                    module_registry,
                ))
            }
        }
    }
}

fn main() {
    let args = Args::parse();

    // Validate input arguments
    let file_path = args.file.or(args.input_file);
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
        InputSource::Pipe => {
            InputConfig::from_pipe(args.verbose)
        }
        InputSource::Interactive => {
            eprintln!("❌ Error: Interactive mode is not supported in rtfs-compiler. Use rtfs-repl instead.");
            std::process::exit(1);
        }
    };

    // Read input content
    let input_content = match read_input_content(&input_config) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    if args.verbose {
        println!("📖 Reading from: {}", input_content.source_name);
        println!("📊 Source size: {} bytes", input_content.content.len());
    }

    let total_start = Instant::now();

    // Phase 1: Parsing
    let parse_start = Instant::now();
    let parsed_items = match parse_with_enhanced_errors(&input_content.content, Some(&input_content.source_name)) {
        Ok(items) => items,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let parse_time = parse_start.elapsed();

    if args.verbose {
        println!("✅ Parsing completed in {:?}", parse_time);
        println!("📊 Parsed {} top-level items", parsed_items.len());
    }

    // Phase 1.5: RTFS 2.0 Schema Validation
    let validation_start = Instant::now();
    let should_validate = args.validate && !args.skip_validation;

    if should_validate {
        let mut validation_errors = Vec::new();

        for (i, item) in parsed_items.iter().enumerate() {
            match SchemaValidator::validate_object(item) {
                Ok(_) => {
                    if args.verbose {
                        println!(
                            "✅ Validated item {}: {:?}",
                            i + 1,
                            std::mem::discriminant(item)
                        );
                    }
                }
                Err(e) => {
                    validation_errors.push((i + 1, e));
                }
            }
        }

        if !validation_errors.is_empty() {
            eprintln!("❌ RTFS 2.0 Schema Validation Errors:");
            for (item_num, error) in validation_errors {
                eprintln!("  Item {}: {:?}", item_num, error);
            }
            std::process::exit(1);
        }

        let validation_time = validation_start.elapsed();
        if args.verbose {
            println!("✅ Schema validation completed in {:?}", validation_time);
        }
    } else if args.verbose {
        println!("⚠️  Schema validation skipped");
    }

    // Phase 2: Process top-level items
    let mut all_results = Vec::new();
    let mut total_ir_time = std::time::Duration::ZERO;
    let mut total_opt_time = std::time::Duration::ZERO;

    if args.execute {
        // Execute all expressions together to preserve state
        let exec_start = Instant::now();

        // Create a shared runtime strategy for all expressions to preserve state
        let runtime_strategy: Box<dyn RuntimeStrategy> = args.runtime.clone().into();
        let mut runtime = Runtime::new(runtime_strategy);

        // For AST runtime, we can use eval_toplevel to preserve state
        if let RuntimeType::Ast = args.runtime {
            // Create an evaluator that can handle multiple top-level items
            let mut module_registry = ModuleRegistry::new();
            // Load standard library
            if let Err(e) = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry) {
                eprintln!("Warning: Failed to load standard library: {:?}", e);
            }
                let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capability_registry::CapabilityRegistry::new()));
                let causal_chain = Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()));
                let capability_marketplace = Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
                let host = std::rc::Rc::new(RuntimeHost::new(
                    causal_chain,
                    capability_marketplace,
                    rtfs_compiler::runtime::security::RuntimeContext::full(),
                ));
                let mut evaluator =
                    rtfs_compiler::runtime::Evaluator::new(
                        std::rc::Rc::new(module_registry),
                        std::sync::Arc::new(rtfs_compiler::ccos::delegation::StaticDelegationEngine::new(std::collections::HashMap::new())),
                        rtfs_compiler::runtime::security::RuntimeContext::full(),
                        host.clone(),
                    );

            // TODO: Call host.prepare_execution() when method is implemented

            match evaluator.eval_toplevel(&parsed_items) {
                Ok(value) => {
                    // TODO: Call host.cleanup_execution() when method is implemented
                    let exec_time = exec_start.elapsed();
                    if args.verbose {
                        println!("✅ Execution completed in {:?}", exec_time);
                        println!("📊 Result: {:?}", value);
                    }
                    all_results.push(value);
                }
                Err(e) => {
                    // TODO: Call host.cleanup_execution() when method is implemented
                    eprintln!("❌ Runtime error: {:?}", e);
                    std::process::exit(1);
                }
            }
        } else {
            // For other runtimes, execute each expression individually
            for (i, item) in parsed_items.iter().enumerate() {
                if args.verbose {
                    println!(
                        "\n🔄 Processing item {}: {:?}",
                        i + 1,
                        std::mem::discriminant(item)
                    );
                }

                match item {
                    TopLevel::Expression(expr) => match runtime.run(expr) {
                        Ok(value) => {
                            if args.verbose {
                                println!("📊 Result: {:?}", value);
                            }
                            all_results.push(value);
                        }
                        Err(e) => {
                            eprintln!("❌ Runtime error for expression {}: {:?}", i + 1, e);
                            std::process::exit(1);
                        }
                    },
                    TopLevel::Intent(_)
                    | TopLevel::Plan(_)
                    | TopLevel::Action(_)
                    | TopLevel::Capability(_)
                    | TopLevel::Resource(_)
                    | TopLevel::Module(_) => {
                        if args.verbose {
                            println!("📋 RTFS 2.0 object (no execution needed)");
                        }
                    }
                }
            }
        }
    } else {
        // Process items based on runtime choice (even when not executing)
        for (i, item) in parsed_items.iter().enumerate() {
            if args.verbose {
                println!(
                    "\n🔄 Processing item {}: {:?}",
                    i + 1,
                    std::mem::discriminant(item)
                );
            }

            match item {
                TopLevel::Expression(expr) => {
                    match args.runtime {
                        RuntimeType::Ast => {
                            // For AST runtime, just validate the expression without IR conversion
                            if args.verbose {
                                println!("📋 AST validation completed (no IR conversion needed)");
                            }
                        }
                        RuntimeType::Ir | RuntimeType::Fallback => {
                            // Convert expression to IR for IR-based runtimes
                            let ir_start = Instant::now();
                            
                            // Create module registry and load standard library for IR conversion
                            let mut module_registry = ModuleRegistry::new();
                            if let Err(e) = rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry) {
                                eprintln!("Warning: Failed to load standard library for IR conversion: {:?}", e);
                            }
                            
                            let mut ir_converter = IrConverter::with_module_registry(&module_registry);
                            let ir_node = match ir_converter.convert_expression(expr.clone()) {
                                Ok(ir) => ir,
                                Err(e) => {
                                    eprintln!(
                                        "❌ IR conversion error for expression {}: {:?}",
                                        i + 1,
                                        e
                                    );
                                    std::process::exit(1);
                                }
                            };
                            let ir_time = ir_start.elapsed();
                            total_ir_time += ir_time;

                            if args.verbose {
                                println!("✅ IR conversion completed in {:?}", ir_time);
                            }

                            // Optimize IR
                            let opt_start = Instant::now();
                            let opt_level_for_optimizer = args.opt_level.clone();
                            let mut optimizer =
                                EnhancedOptimizationPipeline::with_optimization_level(
                                    opt_level_for_optimizer.into(),
                                );
                            let _optimized_ir = optimizer.optimize(ir_node);
                            let opt_time = opt_start.elapsed();
                            total_opt_time += opt_time;

                            if args.verbose {
                                println!("✅ Optimization completed in {:?}", opt_time);
                            }
                        }
                    }
                }
                TopLevel::Intent(_)
                | TopLevel::Plan(_)
                | TopLevel::Action(_)
                | TopLevel::Capability(_)
                | TopLevel::Resource(_)
                | TopLevel::Module(_) => {
                    if args.verbose {
                        println!("📋 RTFS 2.0 object (no execution needed)");
                    }
                }
            }
        }
    }

    let total_time = total_start.elapsed();

    // Output Results
    if args.show_timing {
        println!("📊 COMPILATION TIMING:");
        println!("  Parsing:      {:>8.2?}", parse_time);
        if should_validate {
            let validation_time = validation_start.elapsed();
            println!("  Validation:   {:>8.2?}", validation_time);
        }
        println!("  IR Conversion: {:>8.2?}", total_ir_time);
        println!("  Optimization:  {:>8.2?}", total_opt_time);
        println!("  Total:         {:>8.2?}", total_time);

        if !all_results.is_empty() {
            println!("  Execution:     {:>8.2?}", total_time);
        }
        println!();
    }

    if args.show_stats || args.optimization_report {
        println!("📈 OPTIMIZATION STATISTICS:");
        println!("  Optimization Level: {:?}", args.opt_level);

        if !all_results.is_empty() {
            println!("  Execution Performance: {:?}", total_time);
            println!(
                "  Compile vs Execute Ratio: {:.2}:1",
                total_time.as_nanos() as f64 / total_time.as_nanos() as f64
            );
        }
        println!();
    }

    // Show execution result if requested
    if !all_results.is_empty() {
        println!("🎯 EXECUTION RESULT:");
        for (i, result) in all_results.iter().enumerate() {
            println!("📊 Result {}: {:?}", i + 1, result);
        }
    } else if args.verbose {
        println!("✅ Compilation successful! Use --execute to run the compiled code.");
    }

    // Save output if specified
    if let Some(output_path) = args.output {
        let output_content = format!("{:#?}", parsed_items);
        if let Err(e) = fs::write(&output_path, output_content) {
            eprintln!("❌ Error writing output file: {}", e);
            std::process::exit(1);
        }
        if args.verbose {
            println!("💾 Output saved to: {}", output_path.display());
        }
    }
}
