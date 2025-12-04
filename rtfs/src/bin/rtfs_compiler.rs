// RTFS Production Compiler Binary
// Command-line RTFS compiler with RTFS 2.0 support, optimization levels and performance reporting

use clap::{Parser, ValueEnum};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

// Import the RTFS compiler modules
// Note: We need to reference the parent crate since this is a binary
extern crate rtfs;
use rtfs::{
    ast::TopLevel,
    bytecode::BytecodeBackend,
    input_handling::{read_input_content, validate_input_args, InputConfig, InputSource},
    ir::converter::IrConverter,
    ir::enhanced_optimizer::{EnhancedOptimizationPipeline, OptimizationLevel},
    ir::type_checker,
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

    /// Dump parsed AST (Abstract Syntax Tree)
    #[arg(long)]
    dump_ast: bool,

    /// Dump IR (Intermediate Representation) before optimization
    #[arg(long)]
    dump_ir: bool,

    /// Dump optimized IR after optimization passes
    #[arg(long)]
    dump_ir_optimized: bool,

    /// Format/prettify RTFS code and output to stdout or file
    #[arg(long)]
    format: bool,

    /// Show inferred types for expressions
    #[arg(long)]
    show_types: bool,

    /// Compile to WebAssembly bytecode
    #[arg(long)]
    compile_wasm: bool,

    /// WASM output file (required if --compile-wasm is used)
    #[arg(long, requires = "compile_wasm")]
    wasm_output: Option<PathBuf>,

    /// Security audit: analyze code for security issues and capability requirements
    #[arg(long)]
    security_audit: bool,

    /// Enable IR type checking (validates type consistency before execution)
    #[arg(long, default_value_t = true)]
    type_check: bool,

    /// Disable IR type checking (skip type validation)
    #[arg(long, conflicts_with = "type_check")]
    no_type_check: bool,
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
                let module_registry = ModuleRegistry::new();
                // Load standard library
                if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&module_registry) {
                    eprintln!("Warning: Failed to load standard library: {:?}", e);
                }
                // Use pure host for standalone RTFS compilation (no CCOS dependencies)
                let host = rtfs::runtime::pure_host::create_pure_host();
                let evaluator = rtfs::runtime::Evaluator::new(
                    std::sync::Arc::new(module_registry),
                    rtfs::runtime::security::RuntimeContext::full(),
                    host,
                    rtfs::compiler::expander::MacroExpander::default(),
                );
                Box::new(rtfs::runtime::TreeWalkingStrategy::new(evaluator))
            }
            RuntimeType::Ir => {
                let module_registry = ModuleRegistry::new();
                // Load standard library
                if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&module_registry) {
                    eprintln!("Warning: Failed to load standard library: {:?}", e);
                }
                Box::new(rtfs::runtime::ir_runtime::IrStrategy::new(Arc::new(
                    module_registry,
                )))
            }
            RuntimeType::Fallback => {
                let module_registry = ModuleRegistry::new();
                // Load standard library
                if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&module_registry) {
                    eprintln!("Warning: Failed to load standard library: {:?}", e);
                }
                Box::new(rtfs::runtime::IrWithFallbackStrategy::new(Arc::new(
                    module_registry,
                )))
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
        InputSource::Pipe => InputConfig::from_pipe(args.verbose),
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
    let mut parsed_items = match parse_with_enhanced_errors(
        &input_content.content,
        Some(&input_content.source_name),
    ) {
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

    // ========================================
    // NEW FEATURES: AST dump, IR dump, format, types, security audit
    // ========================================

    // Feature 1: Dump AST
    if args.dump_ast {
        println!("\n📋 ABSTRACT SYNTAX TREE (AST):");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        for (i, item) in parsed_items.iter().enumerate() {
            println!("\n[{}] {:#?}", i + 1, item);
        }
        println!("\n✅ AST dump complete");
    }

    // Feature 2: Format/Prettify
    if args.format {
        println!("\n✨ FORMATTED RTFS CODE:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        for item in &parsed_items {
            println!("{}", format_toplevel(item));
        }
        println!("\n✅ Format complete");

        // If format is the only operation, exit early
        if !args.execute
            && !args.dump_ir
            && !args.dump_ir_optimized
            && !args.show_types
            && !args.compile_wasm
            && !args.security_audit
        {
            return;
        }
    }

    // Feature 3: Show Types
    if args.show_types {
        println!("\n🔍 TYPE INFERENCE:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        for (i, item) in parsed_items.iter().enumerate() {
            let inferred_type = infer_type(item);
            println!(
                "[{}] {} :: {}",
                i + 1,
                describe_toplevel(item),
                inferred_type
            );
        }
        println!("\n✅ Type inference complete");
    }

    // Feature 4: Security Audit
    if args.security_audit {
        println!("\n🔒 SECURITY AUDIT:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        let audit_result = perform_security_audit(&parsed_items);
        print_security_audit(&audit_result);
        println!("\n✅ Security audit complete");
    }

    // Phase 2: Process top-level items
    // Expand macros across top-level items early so later phases (IR
    // conversion / execution) don't need to repeatedly reconstruct a
    // persistent expander. This centralizes expansion and ensures
    // defmacro declarations are available for subsequent top-level items.
    // Expand top-levels and capture the MacroExpander used so we can reuse
    // the same macro registry during later runtime strategy injection.
    let mut macro_expander = rtfs::compiler::expander::MacroExpander::default();
    match rtfs::compiler::expander::expand_top_levels(&parsed_items) {
        Ok((expanded, expander)) => {
            if args.verbose {
                println!("📄 Expanded top-level AST (macros applied)");
            }
            parsed_items = expanded;
            macro_expander = expander;
        }
        Err(e) => {
            eprintln!(
                "Warning: macro expansion failed for top-level program: {}",
                e
            );
        }
    }

    let mut all_results = Vec::new();
    // Persistent MacroExpander used across processing phases so top-level
    // defmacro declarations are registered and available for subsequent
    // expressions during dumping and execution. (kept for backward compatibility)
    let mut total_ir_time = std::time::Duration::ZERO;
    let mut total_opt_time = std::time::Duration::ZERO;

    if args.execute {
        // If IR dumping is requested, convert to IR first even when executing
        if args.dump_ir || args.dump_ir_optimized {
            // Use a persistent MacroExpander across the dump pass so top-level
            // defmacro declarations are recorded and subsequent expressions are
            // expanded before IR conversion.
            for (i, item) in parsed_items.iter().enumerate() {
                if let TopLevel::Expression(expr) = item {
                    // Convert to IR for dumping
                    let ir_start = Instant::now();
                    let module_registry = ModuleRegistry::new();
                    if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&module_registry) {
                        eprintln!(
                            "Warning: Failed to load standard library for IR conversion: {:?}",
                            e
                        );
                    }

                    let mut ir_converter = IrConverter::with_module_registry(&module_registry);
                    // Expand macros for this expression before converting to IR.
                    let expanded_expr = match macro_expander.expand(expr, 0) {
                        Ok(e) => e,
                        Err(e) => {
                            eprintln!(
                                "Warning: macro expansion error for expression {}: {}",
                                i + 1,
                                e
                            );
                            expr.clone()
                        }
                    };
                    if args.verbose {
                        println!(
                            "📄 Expanded AST for expression {}: {:#?}",
                            i + 1,
                            expanded_expr
                        );
                    }
                    match ir_converter.convert_expression(expanded_expr) {
                        Ok(ir_node) => {
                            let ir_time = ir_start.elapsed();
                            total_ir_time += ir_time;

                            // Dump IR before optimization
                            if args.dump_ir {
                                println!("\n📊 IR (Before Optimization) for Expression {}:", i + 1);
                                println!("{:#?}", ir_node);
                            }

                            // Optimize and dump if requested
                            if args.dump_ir_optimized {
                                let opt_start = Instant::now();
                                let opt_level = args.opt_level.clone();
                                let mut optimizer =
                                    EnhancedOptimizationPipeline::with_optimization_level(
                                        opt_level.into(),
                                    );
                                let optimized_ir = optimizer.optimize(ir_node);
                                let opt_time = opt_start.elapsed();
                                total_opt_time += opt_time;

                                println!("\n📊 IR (After Optimization) for Expression {}:", i + 1);
                                println!("{:#?}", optimized_ir);
                            }
                        }
                        Err(e) => {
                            eprintln!("❌ IR conversion error for expression {}: {:?}", i + 1, e);
                        }
                    }
                }
            }
        }

        // Execute all expressions together to preserve state
        let exec_start = Instant::now();

        // Create a shared runtime strategy for all expressions to preserve state
        let mut runtime_strategy: Box<dyn RuntimeStrategy> = args.runtime.clone().into();
        // Inject the persistent MacroExpander so runtime strategies share the same
        // macro registry as the compiler expansion pass. Clone since MacroExpander
        // implements Clone and strategies take ownership.
        runtime_strategy.set_macro_expander(macro_expander.clone());
        let mut runtime = Runtime::new(runtime_strategy);

        // For AST runtime, we can use eval_toplevel to preserve state
        if let RuntimeType::Ast = args.runtime {
            // Create an evaluator that can handle multiple top-level items
            let mut module_registry = ModuleRegistry::new();
            // Load standard library
            if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&mut module_registry) {
                eprintln!("Warning: Failed to load standard library: {:?}", e);
            }
            // Use pure host for standalone RTFS compilation
            let host = rtfs::runtime::pure_host::create_pure_host();
            let mut evaluator = rtfs::runtime::Evaluator::new(
                std::sync::Arc::new(module_registry),
                rtfs::runtime::security::RuntimeContext::full(),
                host.clone(),
                macro_expander.clone(),
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
                            all_results.push(rtfs::runtime::ExecutionOutcome::Complete(value));
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
                            if let Err(e) = rtfs::runtime::stdlib::load_stdlib(&mut module_registry)
                            {
                                eprintln!("Warning: Failed to load standard library for IR conversion: {:?}", e);
                            }

                            let mut ir_converter =
                                IrConverter::with_module_registry(&module_registry);
                            // Expand macros here as well so IR conversion doesn't see macro nodes.
                            let expanded_expr = match macro_expander.expand(expr, 0) {
                                Ok(e) => e,
                                Err(e) => {
                                    eprintln!(
                                        "Warning: macro expansion error for expression {}: {}",
                                        i + 1,
                                        e
                                    );
                                    expr.clone()
                                }
                            };
                            if args.verbose {
                                println!(
                                    "📄 Expanded AST for expression {}: {:#?}",
                                    i + 1,
                                    expanded_expr
                                );
                            }
                            let ir_node = match ir_converter.convert_expression(expanded_expr) {
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

                            // Feature: Dump IR (before optimization)
                            if args.dump_ir {
                                println!("\n📊 IR (Before Optimization) for Expression {}:", i + 1);
                                println!("{:#?}", ir_node);
                            }

                            if args.verbose {
                                println!("✅ IR conversion completed in {:?}", ir_time);
                            }

                            // Feature: Type check IR (enabled by default)
                            let should_type_check = args.type_check && !args.no_type_check;
                            if should_type_check {
                                let type_check_start = Instant::now();
                                if let Err(e) = type_checker::type_check_ir(&ir_node) {
                                    eprintln!("❌ Type error in expression {}: {}", i + 1, e);
                                    eprintln!("   Use --no-type-check to skip type validation (not recommended)");
                                    std::process::exit(1);
                                }
                                let type_check_time = type_check_start.elapsed();
                                if args.verbose {
                                    println!("✅ Type checking completed in {:?}", type_check_time);
                                }
                            }

                            // Optimize IR
                            let opt_start = Instant::now();
                            let opt_level_for_optimizer = args.opt_level.clone();
                            let mut optimizer =
                                EnhancedOptimizationPipeline::with_optimization_level(
                                    opt_level_for_optimizer.into(),
                                );
                            let optimized_ir = optimizer.optimize(ir_node);
                            let opt_time = opt_start.elapsed();
                            total_opt_time += opt_time;

                            // Feature: Dump optimized IR
                            if args.dump_ir_optimized {
                                println!("\n📊 IR (After Optimization) for Expression {}:", i + 1);
                                println!("{:#?}", optimized_ir);
                            }

                            // Feature: Compile to WASM
                            if args.compile_wasm {
                                let wasm_backend = rtfs::bytecode::WasmBackend;
                                let bytecode = wasm_backend.compile_module(&optimized_ir);

                                if let Some(ref wasm_path) = args.wasm_output {
                                    if let Err(e) = fs::write(wasm_path, &bytecode) {
                                        eprintln!("❌ Error writing WASM output: {}", e);
                                        std::process::exit(1);
                                    }
                                    println!(
                                        "✅ WASM bytecode written to: {}",
                                        wasm_path.display()
                                    );
                                    println!("   Size: {} bytes", bytecode.len());
                                } else {
                                    println!("📦 WASM Bytecode (Expression {}):", i + 1);
                                    println!("   Size: {} bytes", bytecode.len());
                                    println!("   Target: {}", wasm_backend.target_id());
                                }
                            }

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
    } else {
        // Always show compilation success message when not executing
        println!("✅ Compilation successful!");
        if !args.verbose {
            println!(
                "💡 Tip: Use --execute to run the compiled code, or --verbose for more details."
            );
        }
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

// ============================================================================
// HELPER FUNCTIONS FOR NEW FEATURES
// ============================================================================

/// Format a top-level item as prettified RTFS code
fn format_toplevel(item: &TopLevel) -> String {
    match item {
        TopLevel::Expression(expr) => format_expression(expr, 0),
        TopLevel::Intent(intent) => format!("(intent {} ...)", intent.name),
        TopLevel::Plan(plan) => format!("(plan {} ...)", plan.name),
        TopLevel::Action(action) => format!("(action {} ...)", action.name),
        TopLevel::Capability(cap) => format!("(capability {} ...)", cap.name),
        TopLevel::Resource(res) => format!("(resource {} ...)", res.name),
        TopLevel::Module(module) => format!("(module {} ...)", module.name),
    }
}

/// Format an expression with indentation
fn format_expression(expr: &rtfs::ast::Expression, indent: usize) -> String {
    use rtfs::ast::Expression;
    let indent_str = "  ".repeat(indent);

    match expr {
        Expression::Literal(lit) => format!("{}{:?}", indent_str, lit),
        Expression::Symbol(s) => format!("{}{}", indent_str, s.0),
        Expression::List(items) => {
            if items.is_empty() {
                format!("{}()", indent_str)
            } else if items.len() == 1 {
                format!("({})", format_expression(&items[0], 0).trim())
            } else {
                let mut result = format!("{}(", indent_str);
                for (i, item) in items.iter().enumerate() {
                    if i == 0 {
                        result.push_str(&format_expression(item, 0).trim());
                    } else {
                        result.push('\n');
                        result.push_str(&format_expression(item, indent + 1));
                    }
                }
                result.push(')');
                result
            }
        }
        Expression::Vector(items) => {
            format!(
                "{}[{}]",
                indent_str,
                items
                    .iter()
                    .map(|e| format_expression(e, 0).trim().to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        Expression::Map(pairs) => {
            use rtfs::ast::MapKey;
            let mut result = format!("{}{{", indent_str);
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    result.push_str(",\n");
                }
                let key_str = match k {
                    MapKey::Keyword(k) => format!(":{}", k.0),
                    MapKey::String(s) => format!("\"{}\"", s),
                    MapKey::Integer(n) => n.to_string(),
                };
                result.push_str(&format!(
                    "\n{}  {} {}",
                    indent_str,
                    key_str,
                    format_expression(v, 0).trim()
                ));
            }
            result.push_str(&format!("\n{}}}", indent_str));
            result
        }
        _ => format!("{}{:?}", indent_str, expr),
    }
}

/// Infer the type of a top-level item
fn infer_type(item: &TopLevel) -> String {
    match item {
        TopLevel::Expression(expr) => infer_expression_type(expr),
        TopLevel::Intent(_) => "Intent".to_string(),
        TopLevel::Plan(_) => "Plan".to_string(),
        TopLevel::Action(_) => "Action".to_string(),
        TopLevel::Capability(_) => "Capability".to_string(),
        TopLevel::Resource(_) => "Resource".to_string(),
        TopLevel::Module(_) => "Module".to_string(),
    }
}

/// Infer the type of an expression
fn infer_expression_type(expr: &rtfs::ast::Expression) -> String {
    use rtfs::ast::{Expression, Literal};

    match expr {
        Expression::Literal(lit) => match lit {
            Literal::Integer(_) => "Integer",
            Literal::Float(_) => "Float",
            Literal::String(_) => "String",
            Literal::Boolean(_) => "Boolean",
            Literal::Nil => "Nil",
            Literal::Keyword(_) => "Keyword",
            Literal::Symbol(_) => "Symbol",
            Literal::Timestamp(_) => "Timestamp",
            Literal::Uuid(_) => "UUID",
            Literal::ResourceHandle(_) => "ResourceHandle",
        }
        .to_string(),
        Expression::Symbol(s) => format!("Symbol({})", s.0),
        Expression::List(items) => {
            if items.is_empty() {
                "List<Any>".to_string()
            } else if let Expression::Symbol(s) = &items[0] {
                // Try to infer return type based on function name
                match s.0.as_str() {
                    "+" | "-" | "*" | "/" | "mod" => "Number".to_string(),
                    "=" | "<" | ">" | "<=" | ">=" | "and" | "or" | "not" => "Boolean".to_string(),
                    "str" | "concat" => "String".to_string(),
                    "list" => "List<Any>".to_string(),
                    "map" => "Map<Any, Any>".to_string(),
                    _ => "Any".to_string(),
                }
            } else {
                "Any".to_string()
            }
        }
        Expression::Vector(_) => "Vector<Any>".to_string(),
        Expression::Map(_) => "Map<Any, Any>".to_string(),
        _ => "Any".to_string(),
    }
}

/// Get a short description of a top-level item
fn describe_toplevel(item: &TopLevel) -> String {
    match item {
        TopLevel::Expression(expr) => describe_expression(expr),
        TopLevel::Intent(intent) => format!("Intent({})", intent.name),
        TopLevel::Plan(plan) => format!("Plan({})", plan.name),
        TopLevel::Action(action) => format!("Action({})", action.name),
        TopLevel::Capability(cap) => format!("Capability({})", cap.name),
        TopLevel::Resource(res) => format!("Resource({})", res.name),
        TopLevel::Module(module) => format!("Module({})", module.name),
    }
}

/// Get a short description of an expression
fn describe_expression(expr: &rtfs::ast::Expression) -> String {
    use rtfs::ast::Expression;

    match expr {
        Expression::Literal(lit) => format!("{:?}", lit),
        Expression::Symbol(s) => s.0.clone(),
        Expression::List(items) if !items.is_empty() => {
            if let Expression::Symbol(s) = &items[0] {
                format!("({}...)", s.0)
            } else {
                "(...)".to_string()
            }
        }
        Expression::List(_) => "()".to_string(),
        Expression::Vector(_) => "[...]".to_string(),
        Expression::Map(_) => "{...}".to_string(),
        _ => format!("{:?}", expr),
    }
}

/// Security audit result
#[derive(Debug)]
struct SecurityAudit {
    required_capabilities: Vec<String>,
    file_operations: Vec<String>,
    network_operations: Vec<String>,
    system_operations: Vec<String>,
    isolation_level: String,
    microvm_required: bool,
    security_issues: Vec<SecurityIssue>,
    recommended_memory_limit: u64,
    recommended_time_limit: u64,
}

#[derive(Debug)]
struct SecurityIssue {
    severity: &'static str,
    message: String,
    location: String,
}

/// Perform security audit on parsed items
fn perform_security_audit(items: &[TopLevel]) -> SecurityAudit {
    let mut capabilities = Vec::new();
    let mut file_ops = Vec::new();
    let mut network_ops = Vec::new();
    let mut system_ops = Vec::new();
    let mut issues = Vec::new();
    let mut needs_sandboxed = false;

    for (i, item) in items.iter().enumerate() {
        if let TopLevel::Expression(expr) = item {
            audit_expression(
                expr,
                &format!("item {}", i + 1),
                &mut capabilities,
                &mut file_ops,
                &mut network_ops,
                &mut system_ops,
                &mut issues,
                &mut needs_sandboxed,
            );
        }
    }

    // Remove duplicates
    capabilities.sort();
    capabilities.dedup();
    file_ops.sort();
    file_ops.dedup();
    network_ops.sort();
    network_ops.dedup();
    system_ops.sort();
    system_ops.dedup();

    // Determine resource limits based on operations (before moving the Vecs)
    let network_count = network_ops.len();
    let file_count = file_ops.len();

    let memory_limit = if network_count == 0 && file_count < 3 {
        16 * 1024 * 1024 // 16MB for simple operations
    } else {
        64 * 1024 * 1024 // 64MB for complex operations
    };

    let time_limit = if network_count == 0 {
        1000 // 1s for local operations
    } else {
        5000 // 5s for network operations
    };

    let microvm_required = network_count > 0 || file_count > 0;

    SecurityAudit {
        required_capabilities: capabilities,
        file_operations: file_ops,
        network_operations: network_ops,
        system_operations: system_ops,
        isolation_level: if needs_sandboxed {
            "Sandboxed"
        } else {
            "Controlled"
        }
        .to_string(),
        microvm_required,
        security_issues: issues,
        recommended_memory_limit: memory_limit,
        recommended_time_limit: time_limit,
    }
}

/// Recursively audit an expression for security concerns
fn audit_expression(
    expr: &rtfs::ast::Expression,
    location: &str,
    capabilities: &mut Vec<String>,
    file_ops: &mut Vec<String>,
    network_ops: &mut Vec<String>,
    system_ops: &mut Vec<String>,
    issues: &mut Vec<SecurityIssue>,
    needs_sandboxed: &mut bool,
) {
    use rtfs::ast::Expression;

    match expr {
        // Handle FunctionCall variant (modern AST structure)
        Expression::FunctionCall { callee, arguments } => {
            if let Expression::Symbol(func) = callee.as_ref() {
                let arg_count = arguments.len();
                match func.0.as_str() {
                    // File I/O operations
                    "read-file" | "ccos.io.read-file" => {
                        capabilities.push("ccos.io.read-file".to_string());
                        file_ops.push(format!("{}: read-file", location));
                        if arg_count < 1 {
                            issues.push(SecurityIssue {
                                severity: "HIGH",
                                message: "File read without path argument".to_string(),
                                location: location.to_string(),
                            });
                        }
                    }
                    "write-file" | "ccos.io.write-file" => {
                        capabilities.push("ccos.io.write-file".to_string());
                        file_ops.push(format!("{}: write-file", location));
                        *needs_sandboxed = true;
                    }
                    "delete-file" | "ccos.io.delete-file" => {
                        capabilities.push("ccos.io.delete-file".to_string());
                        file_ops.push(format!("{}: delete-file", location));
                        *needs_sandboxed = true;
                        issues.push(SecurityIssue {
                            severity: "HIGH",
                            message: "File deletion operation detected".to_string(),
                            location: location.to_string(),
                        });
                    }

                    // Network operations
                    "http-fetch" | "ccos.network.http-fetch" | "fetch" => {
                        capabilities.push("ccos.network.http-fetch".to_string());
                        network_ops.push(format!("{}: http-fetch", location));
                        *needs_sandboxed = true;
                        if arg_count < 1 {
                            issues.push(SecurityIssue {
                                severity: "HIGH",
                                message: "Network call without URL argument".to_string(),
                                location: location.to_string(),
                            });
                        }
                    }

                    // System operations
                    "get-env" | "ccos.system.get-env" => {
                        capabilities.push("ccos.system.get-env".to_string());
                        system_ops.push(format!("{}: get-env", location));
                    }
                    "execute" | "system" | "shell" => {
                        capabilities.push("external_program".to_string());
                        system_ops.push(format!("{}: execute", location));
                        *needs_sandboxed = true;
                        issues.push(SecurityIssue {
                            severity: "CRITICAL",
                            message: "External program execution detected".to_string(),
                            location: location.to_string(),
                        });
                    }

                    _ => {}
                }
            }

            // Recursively audit arguments
            audit_expression(
                callee.as_ref(),
                &format!("{}[callee]", location),
                capabilities,
                file_ops,
                network_ops,
                system_ops,
                issues,
                needs_sandboxed,
            );
            for (i, arg) in arguments.iter().enumerate() {
                audit_expression(
                    arg,
                    &format!("{}[arg:{}]", location, i),
                    capabilities,
                    file_ops,
                    network_ops,
                    system_ops,
                    issues,
                    needs_sandboxed,
                );
            }
        }

        // Handle List variant (legacy AST structure)
        Expression::List(items) if !items.is_empty() => {
            if let Expression::Symbol(func) = &items[0] {
                match func.0.as_str() {
                    // File I/O operations
                    "read-file" | "ccos.io.read-file" => {
                        capabilities.push("ccos.io.read-file".to_string());
                        file_ops.push(format!("{}: read-file", location));
                        if items.len() < 2 {
                            issues.push(SecurityIssue {
                                severity: "HIGH",
                                message: "File read without path argument".to_string(),
                                location: location.to_string(),
                            });
                        }
                    }
                    "write-file" | "ccos.io.write-file" => {
                        capabilities.push("ccos.io.write-file".to_string());
                        file_ops.push(format!("{}: write-file", location));
                        *needs_sandboxed = true;
                    }
                    "delete-file" | "ccos.io.delete-file" => {
                        capabilities.push("ccos.io.delete-file".to_string());
                        file_ops.push(format!("{}: delete-file", location));
                        *needs_sandboxed = true;
                        issues.push(SecurityIssue {
                            severity: "HIGH",
                            message: "File deletion operation detected".to_string(),
                            location: location.to_string(),
                        });
                    }

                    // Network operations
                    "http-fetch" | "ccos.network.http-fetch" | "fetch" => {
                        capabilities.push("ccos.network.http-fetch".to_string());
                        network_ops.push(format!("{}: http-fetch", location));
                        *needs_sandboxed = true;
                        if items.len() < 2 {
                            issues.push(SecurityIssue {
                                severity: "HIGH",
                                message: "Network call without URL argument".to_string(),
                                location: location.to_string(),
                            });
                        }
                    }

                    // System operations
                    "get-env" | "ccos.system.get-env" => {
                        capabilities.push("ccos.system.get-env".to_string());
                        system_ops.push(format!("{}: get-env", location));
                    }
                    "execute" | "system" | "shell" => {
                        capabilities.push("external_program".to_string());
                        system_ops.push(format!("{}: execute", location));
                        *needs_sandboxed = true;
                        issues.push(SecurityIssue {
                            severity: "CRITICAL",
                            message: "External program execution detected".to_string(),
                            location: location.to_string(),
                        });
                    }

                    _ => {}
                }
            }

            // Recursively audit nested expressions
            for (i, item) in items.iter().enumerate() {
                audit_expression(
                    item,
                    &format!("{}[{}]", location, i),
                    capabilities,
                    file_ops,
                    network_ops,
                    system_ops,
                    issues,
                    needs_sandboxed,
                );
            }
        }
        Expression::Vector(items) | Expression::List(items) => {
            for (i, item) in items.iter().enumerate() {
                audit_expression(
                    item,
                    &format!("{}[{}]", location, i),
                    capabilities,
                    file_ops,
                    network_ops,
                    system_ops,
                    issues,
                    needs_sandboxed,
                );
            }
        }
        Expression::Map(pairs) => {
            for (i, (_k, v)) in pairs.iter().enumerate() {
                audit_expression(
                    v,
                    &format!("{}.val[{}]", location, i),
                    capabilities,
                    file_ops,
                    network_ops,
                    system_ops,
                    issues,
                    needs_sandboxed,
                );
            }
        }
        _ => {}
    }
}

/// Print security audit results
fn print_security_audit(audit: &SecurityAudit) {
    println!("Security Level: {}", audit.isolation_level);

    if !audit.required_capabilities.is_empty() {
        println!("\n🎯 Required Capabilities:");
        for cap in &audit.required_capabilities {
            println!("  - {}", cap);
        }
    }

    if !audit.file_operations.is_empty()
        || !audit.network_operations.is_empty()
        || !audit.system_operations.is_empty()
    {
        println!("\n⚠️  Effects Produced:");
        if !audit.file_operations.is_empty() {
            println!("  File I/O: {} operation(s)", audit.file_operations.len());
            for op in &audit.file_operations {
                println!("    - {}", op);
            }
        }
        if !audit.network_operations.is_empty() {
            println!("  Network: {} operation(s)", audit.network_operations.len());
            for op in &audit.network_operations {
                println!("    - {}", op);
            }
        }
        if !audit.system_operations.is_empty() {
            println!("  System: {} operation(s)", audit.system_operations.len());
            for op in &audit.system_operations {
                println!("    - {}", op);
            }
        }
    }

    println!("\n🔐 Recommended Security Settings:");
    println!("  Isolation Level: {}", audit.isolation_level);
    println!(
        "  MicroVM Required: {}",
        if audit.microvm_required { "Yes" } else { "No" }
    );
    println!(
        "  Memory Limit: {} MB",
        audit.recommended_memory_limit / (1024 * 1024)
    );
    println!(
        "  Execution Time Limit: {} ms",
        audit.recommended_time_limit
    );

    if !audit.security_issues.is_empty() {
        println!(
            "\n⛔ Security Issues Found: {}",
            audit.security_issues.len()
        );
        for issue in &audit.security_issues {
            println!(
                "  [{}] {}: {}",
                issue.severity, issue.location, issue.message
            );
        }
    } else {
        println!("\n✅ No security issues detected");
    }
}
