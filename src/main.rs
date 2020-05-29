use gooseberry::gooseberry::cli::GooseberryCLI;
use gooseberry::gooseberry::Gooseberry;
use structopt::StructOpt;

fn main() -> color_eyre::Result<()> {
    let cli = GooseberryCLI::from_args();
    Gooseberry::start(cli)?;
    Ok(())
}
