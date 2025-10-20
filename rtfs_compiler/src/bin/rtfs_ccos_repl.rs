//! Enhanced RTFS REPL with CCOS Integration
//!
//! Provides an interactive REPL environment with:
//! - Multiple security levels
//! - Configurable capability access
//! - File execution support
//! - Command-line configuration

use clap::{Arg, Command};
use rtfs_compiler::ccos::environment::{
    CCOSBuilder, CCOSEnvironment, CapabilityCategory, SecurityLevel,
};
use rtfs_compiler::runtime::{values::Value, ExecutionOutcome};
use rustyline::Editor;
use std::path::Path;

const REPL_HISTORY_FILE: &str = ".rtfs_repl_history";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("rtfs-ccos-repl")
        .version("1.0")
        .author("RTFS Team")
        .about("Interactive RTFS REPL with CCOS capabilities")
        .arg(
            Arg::new("file")
                .help("RTFS file to execute")
                .value_name("FILE")
                .index(1),
        )
        .arg(
            Arg::new("expr")
                .long("expr")
                .short('e')
                .help("RTFS expression to execute (after optional --file)")
                .value_name("EXPR"),
        )
        .arg(
            Arg::new("security")
                .long("security")
                .short('s')
                .help("Security level")
                .value_parser(["minimal", "standard", "paranoid", "custom"])
                .default_value("standard"),
        )
        .arg(
            Arg::new("enable")
                .long("enable")
                .help("Enable capability categories")
                .value_parser([
                    "system", "fileio", "network", "agent", "ai", "data", "logging",
                ])
                .num_args(1..)
                .action(clap::ArgAction::Append),
        )
        .arg(
            Arg::new("disable")
                .long("disable")
                .help("Disable capability categories")
                .value_parser([
                    "system", "fileio", "network", "agent", "ai", "data", "logging",
                ])
                .num_args(1..)
                .action(clap::ArgAction::Append),
        )
        .arg(
            Arg::new("timeout")
                .long("timeout")
                .help("Maximum execution time in milliseconds")
                .value_parser(clap::value_parser!(u64))
                .default_value("30000"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .short('v')
                .help("Enable verbose output")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("allow")
                .long("allow")
                .help("Allow specific capabilities")
                .num_args(1..)
                .action(clap::ArgAction::Append),
        )
        .arg(
            Arg::new("deny")
                .long("deny")
                .help("Deny specific capabilities")
                .num_args(1..)
                .action(clap::ArgAction::Append),
        )
        .arg(
            Arg::new("http-real")
                .long("http-real")
                .help("Use the real HTTP provider instead of the mock")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("http-allow")
                .long("http-allow")
                .help("Allow outbound HTTP hostnames (repeatable)")
                .num_args(1..)
                .action(clap::ArgAction::Append),
        )
        .arg(
            Arg::new("microvm-provider")
                .long("microvm-provider")
                .help("Select MicroVM provider (e.g. mock, process)")
                .value_name("PROVIDER"),
        )
        .get_matches();

    // Parse security level
    let security_level = match matches.get_one::<String>("security").unwrap().as_str() {
        "minimal" => SecurityLevel::Minimal,
        "standard" => SecurityLevel::Standard,
        "paranoid" => SecurityLevel::Paranoid,
        "custom" => SecurityLevel::Custom,
        _ => SecurityLevel::Standard,
    };

    // Build CCOS environment
    let mut builder = CCOSBuilder::new()
        .security_level(security_level)
        .max_execution_time(*matches.get_one::<u64>("timeout").unwrap())
        .verbose(matches.get_flag("verbose"));

    // Handle enabled categories
    if let Some(enabled) = matches.get_many::<String>("enable") {
        for category in enabled {
            let cat = parse_capability_category(category);
            if let Some(cat) = cat {
                builder = builder.enable_category(cat);
            }
        }
    }

    // Handle disabled categories
    if let Some(disabled) = matches.get_many::<String>("disable") {
        for category in disabled {
            let cat = parse_capability_category(category);
            if let Some(cat) = cat {
                builder = builder.disable_category(cat);
            }
        }
    }

    // Handle allowed capabilities
    if let Some(allowed) = matches.get_many::<String>("allow") {
        for capability in allowed {
            builder = builder.allow_capability(capability);
        }
    }

    // Handle denied capabilities
    if let Some(denied) = matches.get_many::<String>("deny") {
        for capability in denied {
            builder = builder.deny_capability(capability);
        }
    }

    let http_real = matches.get_flag("http-real");
    if http_real {
        builder = builder.http_mocking(false);
        builder = builder.enable_category(CapabilityCategory::Network);
    }

    if let Some(hosts) = matches.get_many::<String>("http-allow") {
        let host_list: Vec<String> = hosts.map(|h| h.to_string()).collect();
        if !host_list.is_empty() {
            builder = builder.http_allow_hosts(host_list);
        }
    }

    if let Some(provider) = matches.get_one::<String>("microvm-provider") {
        builder = builder.microvm_provider(provider.clone());
    } else if http_real {
        builder = builder.microvm_provider("process");
    }

    // Create environment
    let env = builder.build()?;

    if env.config().verbose {
        println!("🚀 RTFS CCOS Environment initialized");
        println!("🔒 Security Level: {:?}", env.config().security_level);
        println!(
            "📦 Available Capabilities: {}",
            env.list_capabilities().len()
        );
    }

    if http_real
        && !env
            .config()
            .enabled_categories
            .contains(&CapabilityCategory::Network)
    {
        eprintln!(
            "⚠️  HTTP provider enabled but network capability disabled; enable it with --enable network"
        );
    }

    // If file argument provided, execute file first
    if let Some(file_path) = matches.get_one::<String>("file") {
        execute_file(&env, file_path)?;
    }

    // If expression argument provided, execute expression and exit
    if let Some(expr) = matches.get_one::<String>("expr") {
        return execute_expr(&env, expr);
    }

    // Otherwise start interactive REPL
    start_repl(env)
}

fn parse_capability_category(s: &str) -> Option<CapabilityCategory> {
    match s.to_lowercase().as_str() {
        "system" => Some(CapabilityCategory::System),
        "fileio" => Some(CapabilityCategory::FileIO),
        "network" => Some(CapabilityCategory::Network),
        "agent" => Some(CapabilityCategory::Agent),
        "ai" => Some(CapabilityCategory::AI),
        "data" => Some(CapabilityCategory::Data),
        "logging" => Some(CapabilityCategory::Logging),
        _ => None,
    }
}

fn execute_file(env: &CCOSEnvironment, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(file_path).exists() {
        eprintln!("❌ File not found: {}", file_path);
        std::process::exit(1);
    }

    match env.execute_file(file_path) {
        Ok(outcome) => {
            if env.config().verbose {
                println!("✅ Execution completed");
            }
            print_outcome(env, outcome);
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Execution error: {:?}", e);
            std::process::exit(1);
        }
    }
}

fn execute_expr(env: &CCOSEnvironment, expr: &str) -> Result<(), Box<dyn std::error::Error>> {
    match env.execute_code(expr) {
        Ok(outcome) => {
            if env.config().verbose {
                println!("✅ Expression executed");
            }
            print_outcome(env, outcome);
            Ok(())
        }
        Err(e) => {
            eprintln!("❌ Execution error: {:?}", e);
            std::process::exit(1);
        }
    }
}

fn print_outcome(env: &CCOSEnvironment, outcome: ExecutionOutcome) {
    match outcome {
        ExecutionOutcome::Complete(value) => match value {
            Value::Nil => {} // don't print nil
            other => {
                if env.config().verbose {
                    println!("📊 Result: {:?}", other);
                } else {
                    println!("{:?}", other);
                }
            }
        },
        ExecutionOutcome::RequiresHost(hc) => {
            eprintln!("❌ Execution requires host call: {:?}", hc);
        }
        _ => {
            println!("ℹ️  Outcome: {:?}", outcome);
        }
    }
}

fn start_repl(env: CCOSEnvironment) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔮 RTFS CCOS REPL v1.0");
    println!("Type 'help' for commands, 'quit' to exit");
    println!();

    let mut rl = Editor::<(), rustyline::history::DefaultHistory>::new()?;

    // Load history if it exists
    if Path::new(REPL_HISTORY_FILE).exists() {
        let _ = rl.load_history(REPL_HISTORY_FILE);
    }

    loop {
        let prompt = format!(
            "rtfs[{}]> ",
            format!("{:?}", env.config().security_level).to_lowercase()
        );

        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim();

                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line)?;

                match line {
                    "quit" | "exit" | ":q" => {
                        println!("👋 Goodbye!");
                        break;
                    }
                    "help" | ":h" => {
                        print_help();
                    }
                    "stats" | ":stats" => {
                        print_stats(&env);
                    }
                    "caps" | ":caps" => {
                        print_capabilities(&env);
                    }
                    "config" | ":config" => {
                        interactive_config(&env, &mut rl)?;
                    }
                    "clear" | ":clear" => {
                        print!("\x1B[2J\x1B[1;1H"); // Clear screen
                    }
                    line if line.starts_with(":load ") => {
                        let file_path = &line[6..].trim();
                        match env.execute_file(file_path) {
                            Ok(outcome) => print_outcome(&env, outcome),
                            Err(e) => eprintln!("❌ Error: {:?}", e),
                        }
                    }
                    _ => {
                        // Execute RTFS code
                        match env.execute_code(line) {
                            Ok(outcome) => print_outcome(&env, outcome),
                            Err(e) => eprintln!("❌ Error: {:?}", e),
                        }
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(err) => {
                eprintln!("❌ Error: {:?}", err);
                break;
            }
        }
    }

    // Save history
    let _ = rl.save_history(REPL_HISTORY_FILE);

    Ok(())
}

fn print_help() {
    println!("📚 RTFS CCOS REPL Commands:");
    println!("  help, :h         - Show this help");
    println!("  stats, :stats    - Show environment statistics");
    println!("  caps, :caps      - List available capabilities");
    println!("  config, :config  - Interactive configuration menu");
    println!("  clear, :clear    - Clear screen");
    println!("  :load <file>     - Load and execute RTFS file");
    println!("  quit, exit, :q   - Exit REPL");
    println!();
    println!("💡 RTFS Syntax Examples:");
    println!("  (+ 1 2 3)                           ; Basic arithmetic");
    println!("  (let [x 42] (* x 2))                 ; Variable binding");
    println!("  (call \"ccos.io.log\" \"Hello CCOS!\")  ; Capability call");
    println!("  (if (> 5 3) \"yes\" \"no\")             ; Conditional");
    println!();
}

fn print_stats(env: &CCOSEnvironment) {
    println!("📊 Environment Statistics:");
    let stats = env.get_stats();
    for (key, value) in stats {
        println!("  {}: {:?}", key, value);
    }
    println!();
}

fn print_capabilities(env: &CCOSEnvironment) {
    println!("🔧 Available Capabilities:");
    let capabilities = env.list_capabilities();
    if capabilities.is_empty() {
        println!("  (none available)");
    } else {
        for (i, cap) in capabilities.iter().enumerate() {
            println!("  {}: {}", i + 1, cap);
        }
    }
    println!("  Total: {} capabilities", capabilities.len());
    println!();
}

fn interactive_config(
    env: &CCOSEnvironment,
    rl: &mut Editor<(), rustyline::history::DefaultHistory>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        print_config_menu();

        let input = rl.readline("config> ")?;
        let choice = input.trim();

        match choice {
            "1" => security_level_menu(env, rl)?,
            "2" => capabilities_menu(env, rl)?,
            "3" => show_current_config(env),
            "4" | "back" | "b" => break,
            "help" | "h" => print_config_help(),
            "" => continue,
            _ => println!("❌ Invalid choice. Type 'help' for options."),
        }
    }
    Ok(())
}

fn print_config_menu() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                            🔧 CCOS Configuration                         ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║                                                                          ║");
    println!("║   ┌─────────────────────────────────────────────────────────────────┐   ║");
    println!("║   │  🔒  1. Security Level      │  🛡️  2. Capabilities           │   ║");
    println!("║   │                              │                                 │   ║");
    println!("║   │  📊  3. Current Config      │  🔙  4. Back to REPL           │   ║");
    println!("║   └─────────────────────────────────────────────────────────────────┘   ║");
    println!("║                                                                          ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!();
}

fn print_config_help() {
    println!("🆘 Configuration Help:");
    println!("  1, security     - Change security level (minimal/standard/paranoid)");
    println!("  2, caps         - Enable/disable capability categories");
    println!("  3, config       - Show current configuration");
    println!("  4, back, b      - Return to main REPL");
    println!("  help, h         - Show this help");
    println!();
}

fn security_level_menu(
    env: &CCOSEnvironment,
    rl: &mut Editor<(), rustyline::history::DefaultHistory>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                          🔒 Security Level Settings                      ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║                                                                          ║");
    println!(
        "║   Current: {:52} ║",
        format!("{:?}", env.config().security_level)
    );
    println!("║                                                                          ║");
    println!("║   🟢  1. Minimal   - Basic security, most capabilities allowed          ║");
    println!("║   🟡  2. Standard  - Balanced security and functionality                ║");
    println!("║   🔴  3. Paranoid  - Maximum security, restricted capabilities          ║");
    println!("║                                                                          ║");
    println!("║   🔙  4. Back to config menu                                            ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!();

    let input = rl.readline("security> ")?;
    match input.trim() {
        "1" | "minimal" => {
            println!("⚠️  Note: Security level changes require REPL restart to take full effect.");
            println!("✅ Would set security level to: Minimal");
        }
        "2" | "standard" => {
            println!("⚠️  Note: Security level changes require REPL restart to take full effect.");
            println!("✅ Would set security level to: Standard");
        }
        "3" | "paranoid" => {
            println!("⚠️  Note: Security level changes require REPL restart to take full effect.");
            println!("✅ Would set security level to: Paranoid");
        }
        "4" | "back" | "b" => {}
        _ => println!("❌ Invalid choice."),
    }
    Ok(())
}

fn capabilities_menu(
    env: &CCOSEnvironment,
    rl: &mut Editor<(), rustyline::history::DefaultHistory>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        println!();
        println!("╔══════════════════════════════════════════════════════════════════════════╗");
        println!("║                        🛡️  Capability Categories                        ║");
        println!("╠══════════════════════════════════════════════════════════════════════════╣");

        let categories = [
            ("System", "🖥️", "Environment, time, process operations"),
            ("FileIO", "📁", "File reading, writing, directory access"),
            ("Network", "🌐", "HTTP requests, network communication"),
            ("Agent", "🤖", "Inter-agent communication, discovery"),
            ("AI", "🧠", "LLM inference, AI model operations"),
            ("Data", "📊", "JSON parsing, data manipulation"),
            ("Logging", "📝", "Output logging, debugging info"),
        ];

        for (i, (name, icon, desc)) in categories.iter().enumerate() {
            let status = if env
                .list_capabilities()
                .iter()
                .any(|cap| cap.starts_with(&format!("ccos.{}", name.to_lowercase())))
            {
                "🟢 ON "
            } else {
                "🔴 OFF"
            };
            println!(
                "║  {}  {}. {} {:20} - {:25} ║",
                status,
                i + 1,
                icon,
                name,
                desc
            );
        }

        println!("║                                                                          ║");
        println!("║  📋  8. List all capabilities    🔙  9. Back to config               ║");
        println!("╚══════════════════════════════════════════════════════════════════════════╝");
        println!();
        println!("💡 Type number to toggle category, or 'on'/'off' + category name");

        let input = rl.readline("capabilities> ")?;
        let choice = input.trim().to_lowercase();

        match choice.as_str() {
            "1" => toggle_category_message("System"),
            "2" => toggle_category_message("FileIO"),
            "3" => toggle_category_message("Network"),
            "4" => toggle_category_message("Agent"),
            "5" => toggle_category_message("AI"),
            "6" => toggle_category_message("Data"),
            "7" => toggle_category_message("Logging"),
            "8" | "list" => print_capabilities(env),
            "9" | "back" | "b" => break,
            input if input.starts_with("on ") => {
                let category = &input[3..];
                println!("⚠️  Note: Capability changes require REPL restart to take full effect.");
                println!("✅ Would enable category: {}", category);
            }
            input if input.starts_with("off ") => {
                let category = &input[4..];
                println!("⚠️  Note: Capability changes require REPL restart to take full effect.");
                println!("✅ Would disable category: {}", category);
            }
            "" => continue,
            _ => println!("❌ Invalid choice. Try a number 1-9 or 'on/off <category>'"),
        }
    }
    Ok(())
}

fn toggle_category_message(category: &str) {
    println!("⚠️  Note: Capability changes require REPL restart to take full effect.");
    println!("✅ Would toggle category: {}", category);
}

fn show_current_config(env: &CCOSEnvironment) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════╗");
    println!("║                           📊 Current Configuration                       ║");
    println!("╠══════════════════════════════════════════════════════════════════════════╣");
    println!("║                                                                          ║");
    println!(
        "║  🔒 Security Level: {:50} ║",
        format!("{:?}", env.config().security_level)
    );
    println!("║  ⏱️  Execution Timeout: {:46} ║", "30000ms");
    println!(
        "║  🔧 Available Capabilities: {:42} ║",
        env.list_capabilities().len()
    );
    println!("║                                                                          ║");

    // Group capabilities by category
    let mut categories: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for cap in env.list_capabilities() {
        if let Some(category_end) = cap[5..].find('.') {
            // Skip "ccos." and find next dot
            let category = cap[5..5 + category_end].to_string();
            categories
                .entry(category)
                .or_insert_with(Vec::new)
                .push(cap);
        }
    }

    for (category, caps) in categories {
        println!(
            "║  📦 {}: {:58} ║",
            category,
            format!("{} capabilities", caps.len())
        );
    }

    println!("║                                                                          ║");
    println!("╚══════════════════════════════════════════════════════════════════════════╝");
    println!();
}
