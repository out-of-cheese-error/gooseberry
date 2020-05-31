use gooseberry::gooseberry::cli::GooseberryCLI;
use gooseberry::gooseberry::Gooseberry;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    let cli = GooseberryCLI::from_args();
    Gooseberry::start(cli).await?;
    Ok(())
}
