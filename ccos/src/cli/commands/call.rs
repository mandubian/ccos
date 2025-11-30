use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use clap::Args;
use rtfs::runtime::error::RuntimeResult;

#[derive(Args)]
pub struct CallArgs {
    /// Capability ID to execute
    pub capability_id: String,

    /// Arguments as JSON or key=value pairs
    #[arg(trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub async fn execute(
    ctx: &mut CliContext,
    args: CallArgs,
) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    formatter.warning(&format!(
        "Capability call not yet implemented. ID: {}, Args: {:?}",
        args.capability_id, args.args
    ));
    formatter.list_item("See: https://github.com/mandubian/ccos/issues/172");

    Ok(())
}

