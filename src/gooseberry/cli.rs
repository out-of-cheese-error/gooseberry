use std::path::PathBuf;

use crate::configuration::GooseberryConfig;
use crate::NAME;
use std::io;
use structopt::clap::AppSettings;
use structopt::clap::Shell;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
name = "gooseberry",
about = "Create and manage your Hypothesis knowledge-base",
rename_all = "kebab-case",
global_settings = & [AppSettings::DeriveDisplayOrder]
)]
pub enum GooseberryCLI {
    /// Sync newly added or updated Hypothesis annotations.
    Sync,
    /// Tag annotations according to topic.
    Tag,
    /// Create and update your knowledge-base markdown files
    Make,
    /// Generate shell completions
    Complete {
        #[structopt(possible_values = & Shell::variants())]
        shell: Shell,
    },
    /// Manage data locations.
    /// Controlled by $GOOSEBERRY_CONFIG env variable,
    /// Use this to have independent knowledge bases for different projects.
    Config {
        #[structopt(subcommand)]
        cmd: ConfigCommand,
    },
}

impl GooseberryCLI {
    pub(crate) fn complete(shell: Shell) {
        GooseberryCLI::clap().gen_completions_to(NAME, shell, &mut io::stdout());
    }
}

#[derive(StructOpt, Debug)]
pub enum ConfigCommand {
    /// Prints / writes the default configuration options.
    /// Set the generated config file as default by setting the $GOOSEBERRY_CONFIG environment variable
    Default {
        #[structopt(parse(from_os_str))]
        file: Option<PathBuf>,
    },
    /// Prints location of currently set configuration file
    Where,
    /// Change Hypothesis credentials
    Authorize,
}

impl ConfigCommand {
    pub(crate) fn run(&self) -> color_eyre::Result<()> {
        match self {
            ConfigCommand::Default { file } => {
                GooseberryConfig::default_config(file.as_deref())?;
            }
            ConfigCommand::Where => {
                GooseberryConfig::print_config_location()?;
            }
            ConfigCommand::Authorize => {
                let mut config = GooseberryConfig::load()?;
                config.request_credentials()?;
            }
        }
        Ok(())
    }
}
