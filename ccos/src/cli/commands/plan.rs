use crate::cli::CliContext;
use crate::ops::plan::{CreatePlanOptions, ExecutePlanOptions};
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| "expected KEY=VALUE format".to_string())?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

#[derive(Subcommand)]
pub enum PlanCommand {
    /// Create plan from goal
    Create {
        /// Goal description
        goal: String,

        /// Show the plan without executing (dry-run mode)
        #[arg(long)]
        dry_run: bool,

        /// Save plan to file
        #[arg(long)]
        save: Option<String>,

        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,

        /// Skip capability validation
        #[arg(long)]
        skip_validation: bool,

        /// Execute low-risk capabilities during planning to ground prompts
        #[arg(long)]
        enable_safe_exec: bool,

        /// Disable pushing grounded snippets into runtime context for prompts
        #[arg(long = "no-grounding-context", action = clap::ArgAction::SetFalse, default_value_t = true)]
        allow_grounding_context: bool,

        /// Seed grounding parameters (repeatable, KEY=VALUE)
        #[arg(long = "ground", value_name = "KEY=VALUE", value_parser = parse_key_val)]
        grounding_param: Vec<(String, String)>,

        /// Force LLM decomposition (skip pattern path)
        #[arg(long)]
        force_llm: bool,
    },

    /// Execute a plan
    Execute {
        /// Plan ID or path
        plan: String,

        /// Maximum repair attempts on failure (0 = no repair)
        #[arg(long, default_value = "0")]
        repair: usize,

        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,
    },

    /// Validate plan syntax and capability availability
    Validate {
        /// Plan ID or path
        plan: String,
    },

    /// List archived plans (by ID, name, or goal)
    List {
        /// Optional filter (matches id, name, or goal substring)
        #[arg(long, short)]
        filter: Option<String>,
    },

    /// Delete a plan from the archive
    Delete {
        /// Plan ID, prefix, or name/goal substring
        plan: String,
    },

    /// Test individual planning steps (for debugging/development)
    Step {
        #[command(subcommand)]
        step: StepCommand,
    },
}

/// Individual planning step commands for testing
#[derive(Subcommand)]
pub enum StepCommand {
    /// Test tool discovery for a goal
    Discover {
        /// Goal description
        goal: String,
        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,
    },

    /// Test goal decomposition
    Decompose {
        /// Goal description
        goal: String,
        /// Show verbose output (LLM prompts/responses)
        #[arg(long, short)]
        verbose: bool,
        /// Force LLM decomposition (skip pattern path)
        #[arg(long)]
        force_llm: bool,
        /// Show LLM prompt only
        #[arg(long)]
        show_prompt: bool,
    },

    /// Test intent resolution
    Resolve {
        /// Goal description (will decompose first, then resolve)
        goal: String,
        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,
    },

    /// Run all steps in sequence with detailed output
    Full {
        /// Goal description
        goal: String,
        /// Show verbose output
        #[arg(long, short)]
        verbose: bool,
        /// Force LLM decomposition
        #[arg(long)]
        force_llm: bool,
    },
}

pub async fn execute(_ctx: &mut CliContext, command: PlanCommand) -> RuntimeResult<()> {
    match command {
        PlanCommand::Create {
            goal,
            dry_run,
            save,
            verbose,
            skip_validation,
            enable_safe_exec,
            allow_grounding_context,
            grounding_param,
            force_llm,
        } => {
            let options = CreatePlanOptions {
                dry_run,
                save_to: save,
                verbose,
                skip_validation,
                enable_safe_exec,
                allow_grounding_context,
                grounding_params: grounding_param.into_iter().collect(),
                force_llm,
            };
            let result = crate::ops::plan::create_plan_with_options(goal, options).await?;

            // In non-dry-run mode, print the plan
            if !dry_run {
                println!("{}", result.rtfs_code);
            }

            // Show validation summary
            if !result.all_resolved {
                println!(
                    "\n⚠️  {} capability(ies) not found:",
                    result.unresolved_capabilities.len()
                );
                for cap in &result.unresolved_capabilities {
                    println!("   • {}", cap);
                }
            }
        }
        PlanCommand::Execute {
            plan,
            repair,
            verbose,
        } => {
            let options = ExecutePlanOptions {
                max_repair_attempts: repair,
                verbose,
            };
            let result = crate::ops::plan::execute_plan_with_options(plan, options).await?;
            println!("{}", result);
        }
        PlanCommand::Validate { plan } => {
            let valid = crate::ops::plan::validate_plan_full(plan).await?;
            if valid {
                println!("Plan is valid.");
            } else {
                println!("Plan is invalid.");
            }
        }
        PlanCommand::List { filter } => {
            let plans = crate::ops::plan::list_archived_plans(filter.as_deref())?;

            if plans.is_empty() {
                println!("No plans found in archive.");
                return Ok(());
            }

            println!(
                "{:<38}  {:<28}  {:<14}  {}",
                "PLAN ID", "NAME/GOAL", "STATUS", "CREATED_AT"
            );
            for plan in plans {
                let label = plan
                    .name
                    .as_deref()
                    .or_else(|| plan.metadata.get("goal").map(|s| s.as_str()))
                    .unwrap_or("-");
                let truncated = if label.len() > 28 {
                    format!("{}…", &label[..27])
                } else {
                    label.to_string()
                };
                println!(
                    "{:<38}  {:<28}  {:<14}  {}",
                    plan.plan_id,
                    truncated,
                    format!("{:?}", plan.status),
                    plan.created_at
                );
            }
        }
        PlanCommand::Delete { plan } => {
            let removed = crate::ops::plan::delete_plan_by_hint(&plan)?;
            println!("🗑️  Deleted plan {}", removed);
        }
        PlanCommand::Step { step } => {
            execute_step_command(step).await?;
        }
    }
    Ok(())
}

/// Execute a step testing command
async fn execute_step_command(step: StepCommand) -> RuntimeResult<()> {
    use crate::ops::plan::step_testing;

    match step {
        StepCommand::Discover { goal, verbose } => {
            println!("🔍 Discovering tools for: {}", goal);
            let result = step_testing::test_discover(&goal, verbose).await?;
            println!("\n📋 Domain hints: {:?}", result.domain_hints);
            println!("\n🛠️  Found {} tools:", result.tools.len());
            for (i, tool) in result.tools.iter().take(20).enumerate() {
                println!(
                    "   {}. {} - {} ({:?})",
                    i + 1,
                    tool.id,
                    tool.description.chars().take(50).collect::<String>(),
                    tool.action
                );
            }
            if result.tools.len() > 20 {
                println!("   ... and {} more", result.tools.len() - 20);
            }
        }
        StepCommand::Decompose {
            goal,
            verbose,
            force_llm,
            show_prompt,
        } => {
            println!("🧩 Decomposing goal: {}", goal);
            let result =
                step_testing::test_decompose(&goal, verbose, force_llm, show_prompt).await?;
            println!(
                "\n✅ Decomposed into {} sub-intents (confidence: {:.2}):",
                result.sub_intents.len(),
                result.confidence
            );
            for (i, intent) in result.sub_intents.iter().enumerate() {
                println!(
                    "   {}. {} [{:?}]",
                    i + 1,
                    intent.description,
                    intent.intent_type
                );
                if !intent.dependencies.is_empty() {
                    println!("      depends on: {:?}", intent.dependencies);
                }
            }
        }
        StepCommand::Resolve { goal, verbose } => {
            println!("🎯 Resolving goal: {}", goal);
            let (decomp, resolutions, unresolved) =
                step_testing::test_resolve(&goal, verbose).await?;
            println!(
                "\n📦 Resolved {} of {} intents:",
                resolutions.len(),
                decomp.sub_intents.len()
            );
            for (id, resolved) in &resolutions {
                if let Some(cap_id) = resolved.capability_id() {
                    println!("   ✅ {} → {}", id.split(':').last().unwrap_or(id), cap_id);
                }
            }
            if !unresolved.is_empty() {
                println!("\n❌ Unresolved:");
                for id in &unresolved {
                    println!("   • {}", id);
                }
            }
        }
        StepCommand::Full {
            goal,
            verbose,
            force_llm,
        } => {
            println!("🚀 Full planning pipeline for: {}", goal);
            step_testing::test_full_pipeline(&goal, verbose, force_llm).await?;
        }
    }
    Ok(())
}
