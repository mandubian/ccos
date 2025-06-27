// RTFS Production Compiler Binary
// Command-line RTFS compiler with optimization levels and performance reporting

use clap::{Parser, ValueEnum};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

// Import the RTFS compiler modules
// Note: We need to reference the parent crate since this is a binary
extern crate rtfs_compiler;
use rtfs_compiler::{
    parser::parse_expression,
    runtime::{Runtime, RuntimeStrategy},
    runtime::module_runtime::ModuleRegistry,
    ir_converter::IrConverter,
    ir::enhanced_optimizer::{EnhancedOptimizationPipeline, OptimizationLevel},
    agent::discovery_traits::NoOpAgentDiscovery,
};

#[derive(Parser)]
#[command(name = "rtfs-compiler")]
#[command(about = "RTFS Production Compiler with Advanced Optimization")]
#[command(version = "0.1.0")]
struct Args {
    /// Input RTFS source file (can be provided as positional argument or with --input flag)
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,

    /// Input RTFS source file (alternative to positional argument)
    #[arg(short = 'i', long = "input", value_name = "FILE", conflicts_with = "input")]
    input_flag: Option<PathBuf>,

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

impl From<OptLevel> for OptimizationLevel {
    fn from(level: OptLevel) -> Self {
        match level {
            OptLevel::None => OptimizationLevel::None,
            OptLevel::Basic => OptimizationLevel::Basic,
            OptLevel::Aggressive => OptimizationLevel::Aggressive,
        }
    }
}

#[derive(Clone, ValueEnum, Debug)]
enum RuntimeType {
    Ast,
    Ir,
    Fallback,
}

impl From<RuntimeType> for RuntimeStrategy {
    fn from(runtime_type: RuntimeType) -> Self {
        match runtime_type {
            RuntimeType::Ast => RuntimeStrategy::Ast,
            RuntimeType::Ir => RuntimeStrategy::Ir,
            RuntimeType::Fallback => RuntimeStrategy::IrWithFallback,
        }
    }
}

fn main() {
    let args = Args::parse();
    
    // Determine the input file path (either from positional arg or --input flag)
    let input_path = args.input.or(args.input_flag).unwrap_or_else(|| {
        eprintln!("❌ Error: Input file is required. Provide it as a positional argument or use --input flag.");
        eprintln!("Usage: rtfs-compiler <FILE> [OPTIONS]");
        eprintln!("   or: rtfs-compiler --input <FILE> [OPTIONS]");
        std::process::exit(1);
    });
    
    if args.verbose {
        println!("🚀 RTFS Production Compiler v0.1.0");
        println!("📁 Input: {}", input_path.display());
        println!("⚡ Optimization Level: {:?}", args.opt_level);
        println!("🏃 Runtime Strategy: {:?}", args.runtime);
        println!();
    }

    // Read input file
    let source_code = match fs::read_to_string(&input_path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Error reading input file: {}", e);
            std::process::exit(1);
        }
    };

    // Track total compilation time
    let total_start = Instant::now();

    // Phase 1: Parsing
    let parse_start = Instant::now();
    let ast = match parse_expression(&source_code) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("❌ Parse error: {:?}", e);
            std::process::exit(1);
        }
    };
    let parse_time = parse_start.elapsed();

    if args.verbose {
        println!("✅ Parsing completed in {:?}", parse_time);
    }

    // Phase 2: IR Conversion
    let ir_start = Instant::now();
    let mut ir_converter = IrConverter::new();
    let ir_node = match ir_converter.convert_expression(ast) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("❌ IR conversion error: {:?}", e);
            std::process::exit(1);
        }
    };
    let ir_time = ir_start.elapsed();

    if args.verbose {
        println!("✅ IR conversion completed in {:?}", ir_time);
    }    // Phase 3: Optimization
    let opt_start = Instant::now();
    let opt_level_for_optimizer = args.opt_level.clone();
    let mut optimizer = EnhancedOptimizationPipeline::with_optimization_level(opt_level_for_optimizer.into());
    let optimized_ir = optimizer.optimize(ir_node);
    let opt_time = opt_start.elapsed();

    if args.verbose {
        println!("✅ Optimization completed in {:?}", opt_time);
    }

    let total_time = total_start.elapsed();

    // Phase 4: Execution (if requested)
    let execution_result = if args.execute {
        let exec_start = Instant::now();
        
        // Create runtime with agent discovery
        let agent_discovery = Box::new(NoOpAgentDiscovery);
        let module_registry = ModuleRegistry::new();
        let mut runtime = Runtime::with_strategy_and_agent_discovery(
            args.runtime.into(),
            agent_discovery,
            &module_registry
        );
        
        let result = match runtime.evaluate_ir(&optimized_ir) {
            Ok(value) => {
                let exec_time = exec_start.elapsed();
                if args.verbose {
                    println!("✅ Execution completed in {:?}", exec_time);
                }
                Some((value, exec_time))
            }
            Err(e) => {
                eprintln!("❌ Runtime error: {:?}", e);
                std::process::exit(1);
            }
        };
        
        result
    } else {
        None
    };

    // Output Results
    if args.show_timing {
        println!("📊 COMPILATION TIMING:");
        println!("  Parsing:      {:>8.2?}", parse_time);
        println!("  IR Conversion: {:>8.2?}", ir_time);
        println!("  Optimization:  {:>8.2?}", opt_time);
        println!("  Total:         {:>8.2?}", total_time);
        
        if let Some((_, exec_time)) = &execution_result {
            println!("  Execution:     {:>8.2?}", exec_time);
        }
        println!();
    }

    if args.show_stats {
        let stats = optimizer.stats();
        println!("📈 OPTIMIZATION STATISTICS:");
        println!("  Control Flow Optimizations: {}", stats.control_flow_optimizations);
        println!("  Functions Inlined:          {}", stats.functions_inlined);
        println!("  Dead Code Blocks Eliminated: {}", stats.dead_code_blocks_eliminated);
        println!("  Optimization Time:          {}ms", stats.optimization_time_ms);
        println!();
    }    if args.optimization_report {
        println!("📋 OPTIMIZATION REPORT:");
        println!("  Input File: {}", input_path.display());
        println!("  Optimization Level: {:?}", args.opt_level);
        println!("  Total Compilation Time: {:?}", total_time);
        println!("  Optimization Impact: {:.1}% of total time", 
                 (opt_time.as_nanos() as f64 / total_time.as_nanos() as f64) * 100.0);
        
        if let Some((_, exec_time)) = &execution_result {
            println!("  Execution Performance: {:?}", exec_time);
            println!("  Compile vs Execute Ratio: {:.2}:1", 
                     total_time.as_nanos() as f64 / exec_time.as_nanos() as f64);
        }
        println!();
    }

    // Show execution result if requested
    if let Some((result, _)) = execution_result {
        println!("🎯 EXECUTION RESULT:");
        println!("{:?}", result);
    } else if args.verbose {
        println!("✅ Compilation successful! Use --execute to run the compiled code.");
    }

    // Save output if specified
    if let Some(output_path) = args.output {
        let output_content = format!("{:#?}", optimized_ir);
        if let Err(e) = fs::write(&output_path, output_content) {
            eprintln!("❌ Error writing output file: {}", e);
            std::process::exit(1);
        }
        
        if args.verbose {
            println!("💾 Output saved to: {}", output_path.display());
        }
    }
}
