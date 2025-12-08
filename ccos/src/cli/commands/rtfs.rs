use crate::capabilities::registry::CapabilityRegistry;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::utils::value_conversion::rtfs_value_to_json;
use clap::Subcommand;
use rtfs::ast::Symbol;
use rtfs::parser::parse_expression;
use rtfs::runtime::environment::Environment;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::runtime::{Runtime, TreeWalkingStrategy};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime as TokioRuntime};

#[derive(Subcommand)]
pub enum RtfsCommand {
    /// Evaluate an RTFS expression
    Eval {
        /// RTFS expression to evaluate
        expr: String,
    },

    /// Run an RTFS file
    Run {
        /// Path to RTFS file
        file: PathBuf,
    },

    /// Start interactive REPL
    Repl,
}

pub async fn execute(ctx: &mut CliContext, command: RtfsCommand) -> RuntimeResult<()> {
    let _formatter = OutputFormatter::new(ctx.output_format);

    match command {
        RtfsCommand::Eval { expr } => {
            run_rtfs_expr_in_process(&expr).await?;
        }
        RtfsCommand::Run { file } => {
            let content = std::fs::read_to_string(&file).map_err(|e| {
                RuntimeError::Generic(format!(
                    "Failed to read RTFS file {}: {}",
                    file.display(),
                    e
                ))
            })?;
            run_rtfs_expr_in_process(&content).await?;
        }
        RtfsCommand::Repl => {
            ctx.status("Starting RTFS REPL...");
            let mut cmd = Command::new("cargo");
            cmd.arg("run").arg("--bin").arg("rtfs-ccos-repl");

            let status = cmd
                .status()
                .map_err(|e| RuntimeError::Generic(format!("Failed to start REPL: {}", e)))?;

            if !status.success() {
                return Err(RuntimeError::Generic("REPL exited with error".to_string()));
            }
        }
    }

    Ok(())
}

async fn run_rtfs_expr_in_process(expr: &str) -> RuntimeResult<()> {
    // Parse the RTFS expression
    let parsed = parse_expression(expr)
        .map_err(|e| RuntimeError::Generic(format!("RTFS parse error: {:?}", e)))?;

    // Build or reuse an async context (tokio handle)
    let async_ctx = AsyncContext::new().map_err(|e| RuntimeError::Generic(e))?;

    // Build marketplace and load capabilities (approved + generated)
    let registry = Arc::new(tokio::sync::RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
    async_ctx.block_on(load_caps_into_marketplace(&marketplace))?;

    // Build RTFS module registry + stdlib
    let module_registry = Arc::new(ModuleRegistry::new());
    let _ = rtfs::runtime::stdlib::load_stdlib(&module_registry);
    let mut env = Environment::new();
    // Bind stdlib exports into AST environment
    if let Some(stdlib) = module_registry.get_module("stdlib") {
        if let Ok(exports) = stdlib.exports.read() {
            for (name, export) in exports.iter() {
                let sym = Symbol(name.clone());
                env.define(&sym, export.value.clone());
            }
        }
    }

    // Host bridge to marketplace
    let host: Arc<dyn HostInterface> = Arc::new(MarketplaceHost {
        marketplace: marketplace.clone(),
        async_ctx: async_ctx.clone(),
    });

    let evaluator = rtfs::runtime::evaluator::Evaluator::with_environment(
        module_registry.clone(),
        env,
        RuntimeContext::full(),
        host,
    );
    let mut runtime = Runtime::new(Box::new(TreeWalkingStrategy::new(evaluator)));

    // Execute
    match runtime.run(&parsed) {
        Ok(val) => {
            let json = rtfs_value_to_json(&val)
                .unwrap_or_else(|_| serde_json::Value::String(format!("{:?}", val)));
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_default()
            );
            Ok(())
        }
        Err(e) => Err(RuntimeError::Generic(format!(
            "RTFS evaluation failed: {}",
            e
        ))),
    }
}

async fn load_caps_into_marketplace(marketplace: &Arc<CapabilityMarketplace>) -> RuntimeResult<()> {
    // Approved capabilities
    let approved_dir = std::path::Path::new("capabilities/servers/approved");
    if approved_dir.exists() {
        if let Err(e) = marketplace
            .import_capabilities_from_rtfs_dir_recursive(approved_dir)
            .await
        {
            eprintln!("Failed to load capabilities from approved servers: {}", e);
        }
    }
    // Generated capabilities (workspace only)
    let gen_dir = std::path::Path::new("capabilities/generated");
    if gen_dir.exists() {
        if let Err(e) = marketplace
            .import_capabilities_from_rtfs_dir_recursive(gen_dir)
            .await
        {
            eprintln!(
                "Failed to load generated capabilities from {}: {}",
                gen_dir.display(),
                e
            );
        }
    }
    Ok(())
}

struct MarketplaceHost {
    marketplace: Arc<CapabilityMarketplace>,
    async_ctx: AsyncContext,
}

impl std::fmt::Debug for MarketplaceHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MarketplaceHost")
    }
}

#[derive(Clone)]
struct AsyncContext {
    handle: Handle,
    /// Keep runtime alive if we had to create it
    #[allow(dead_code)]
    rt: Option<Arc<TokioRuntime>>,
}

impl AsyncContext {
    fn new() -> Result<Self, String> {
        if let Ok(handle) = Handle::try_current() {
            Ok(Self { handle, rt: None })
        } else {
            let rt = TokioRuntime::new().map_err(|e| format!("tokio init failed: {}", e))?;
            let handle = rt.handle().clone();
            Ok(Self {
                handle,
                rt: Some(Arc::new(rt)),
            })
        }
    }

    fn block_on<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        if let Some(rt) = &self.rt {
            rt.block_on(fut)
        } else {
            // Already inside a runtime: block_in_place to avoid nested runtime panic
            tokio::task::block_in_place(|| self.handle.block_on(fut))
        }
    }
}

impl HostInterface for MarketplaceHost {
    fn execute_capability(&self, name: &str, args: &[Value]) -> RuntimeResult<Value> {
        // Convert args to a single Value (map/vector) to feed marketplace
        let input = if args.len() == 1 {
            args[0].clone()
        } else if args.is_empty() {
            Value::Map(std::collections::HashMap::new())
        } else {
            Value::Vector(args.to_vec())
        };
        self.async_ctx.block_on(async {
            self.marketplace
                .execute_capability(name, &input)
                .await
                .map_err(|e| RuntimeError::Generic(format!("capability {} failed: {}", name, e)))
        })
    }

    fn notify_step_started(&self, _step_name: &str) -> RuntimeResult<String> {
        Ok("0".to_string())
    }
    fn notify_step_completed(
        &self,
        _step_action_id: &str,
        _result: &rtfs::runtime::stubs::ExecutionResultStruct,
    ) -> RuntimeResult<()> {
        Ok(())
    }
    fn notify_step_failed(&self, _step_action_id: &str, _error: &str) -> RuntimeResult<()> {
        Ok(())
    }
    fn set_execution_context(
        &self,
        _plan_id: String,
        _intent_ids: Vec<String>,
        _parent_action_id: String,
    ) {
    }
    fn clear_execution_context(&self) {}
    fn set_step_exposure_override(&self, _expose: bool, _context_keys: Option<Vec<String>>) {}
    fn clear_step_exposure_override(&self) {}
    fn get_context_value(&self, _key: &str) -> Option<Value> {
        None
    }
}
