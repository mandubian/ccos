// RTFS REPL - Interactive Development Environment
// Standalone REPL binary for RTFS development and exploration

use std::env;
use std::process;

// Import from the rtfs_compiler crate
use rtfs_compiler::{RtfsRepl, RuntimeStrategy};

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Parse command line arguments
    let mut runtime_strategy = RuntimeStrategy::Ast;
    let mut show_help = false;
    
    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                show_help = true;
                break;
            }
            "--runtime=ast" => runtime_strategy = RuntimeStrategy::Ast,
            "--runtime=ir" => runtime_strategy = RuntimeStrategy::Ir,
            "--runtime=fallback" => runtime_strategy = RuntimeStrategy::IrWithFallback,
            "--version" | "-V" => {
                println!("rtfs-repl 0.1.0");
                println!("RTFS Interactive Development Environment");
                return;
            }
            _ if arg.starts_with("--") => {
                eprintln!("❌ Unknown option: {}", arg);
                eprintln!("Use --help for usage information");
                process::exit(1);
            }
            _ => {
                eprintln!("❌ Unexpected argument: {}", arg);
                eprintln!("Use --help for usage information");
                process::exit(1);
            }
        }
    }
    
    if show_help {
        print_help();
        return;
    }
    
    // Create and run REPL
    let mut repl = RtfsRepl::with_runtime_strategy(runtime_strategy);
    
    match repl.run() {
        Ok(_) => {},
        Err(e) => {
            eprintln!("❌ REPL error: {}", e);
            process::exit(1);
        }
    }
}

fn print_help() {
    println!("RTFS REPL - Interactive Development Environment");
    println!();
    println!("USAGE:");
    println!("    rtfs-repl [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help               Show this help message");
    println!("    -V, --version            Show version information");
    println!("    --runtime=<STRATEGY>     Set runtime strategy");
    println!();
    println!("RUNTIME STRATEGIES:");
    println!("    ast                      Use AST-based runtime (default)");
    println!("    ir                       Use IR-based runtime");
    println!("    fallback                 Use IR with AST fallback");
    println!();
    println!("EXAMPLES:");
    println!("    rtfs-repl                        # Start REPL with default settings");
    println!("    rtfs-repl --runtime=ir           # Start REPL with IR runtime");
    println!("    rtfs-repl --runtime=fallback     # Start REPL with IR+AST fallback");
    println!();
    println!("INTERACTIVE COMMANDS:");
    println!("    :help                    Show interactive help");
    println!("    :quit                    Exit REPL");
    println!("    :history                 Show command history");
    println!("    :context                 Show current context");
    println!("    :ast                     Toggle AST display");
    println!("    :ir                      Toggle IR display");
    println!("    :opt                     Toggle optimization display");
    println!("    :test                    Run built-in test suite");
    println!("    :bench                   Run performance benchmarks");
    println!();
    println!("RUNTIME SWITCHING:");
    println!("    :runtime-ast             Switch to AST runtime");
    println!("    :runtime-ir              Switch to IR runtime");
    println!("    :runtime-fallback        Switch to IR with AST fallback");
    println!();
    println!("EXAMPLE EXPRESSIONS:");
    println!("    (+ 1 2 3)                       # Basic arithmetic");
    println!("    (let [x 10] (+ x 5))            # Let binding");
    println!("    (if true \"yes\" \"no\")             # Conditional");
    println!("    (vector 1 2 3)                  # Vector creation");
    println!("    (defn square [x] (* x x))       # Function definition");
    println!("    (square 5)                      # Function call");
    println!();
    println!("For more information, visit: https://github.com/mandubian/rtfs-ai");
}
