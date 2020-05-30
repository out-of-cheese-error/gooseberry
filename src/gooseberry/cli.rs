use std::path::PathBuf;

use crate::configuration::GooseberryConfig;
use crate::utils;
use crate::NAME;
use chrono::{DateTime, Utc};
use hypothesis::GroupID;
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
    Tag {
        #[structopt(flatten)]
        filters: Filters,
        /// Use this flag to remove the given tag from the filtered annotations instead of adding it
        #[structopt(long)]
        delete: bool,
        /// Open a search buffer to see and fuzzy search filtered annotations to further filter them
        #[structopt(short, long)]
        search: bool,
        /// The tag to add to / remove from the filtered annotations
        tag: String,
    },
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
    Clear {
        #[structopt(long)]
        force: bool,
    },
}

#[derive(StructOpt, Debug)]
pub struct Filters {
    /// Filter annotations created after this date and time
    /// Can be colloquial, e.g. "last Friday 8pm"
    #[structopt(long, parse(try_from_str = utils::parse_datetime))]
    pub from: Option<DateTime<Utc>>,
    /// Filter annotations with this pattern in their URL
    /// Doesn't have to be the full URL, e.g. "wikipedia"
    #[structopt(long)]
    pub url: Option<String>,
    /// Filter annotations with this pattern in their `quote`, `tags`, `text`, or `url`
    #[structopt(long)]
    pub any: Option<String>,
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
    /// Change the group ID of the group used for hypothesis annotations
    Group { id: GroupID },
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
            ConfigCommand::Group { id } => {
                let mut config = GooseberryConfig::load()?;
                config.change_group(id.to_owned())?;
            }
        }
        Ok(())
    }
}
