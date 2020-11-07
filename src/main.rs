use structopt::StructOpt;

use gooseberry::gooseberry::cli::GooseberryCLI;
use gooseberry::gooseberry::Gooseberry;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::config::HookBuilder::blank()
        .display_env_section(false)
        .install()?;
    let cli = GooseberryCLI::from_args();
    Gooseberry::start(cli).await?;
    Ok(())
}
