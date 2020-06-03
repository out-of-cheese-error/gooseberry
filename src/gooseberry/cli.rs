//! Command-line interface
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use structopt::clap::AppSettings;
use structopt::clap::Shell;
use structopt::StructOpt;

use hypothesis::annotations::{Order, SearchQuery, Sort};

use crate::configuration::GooseberryConfig;
use crate::utils;
use crate::NAME;

#[derive(Debug, StructOpt)]
#[structopt(
name = "gooseberry",
about = "Create and manage your Hypothesis knowledge-base",
rename_all = "kebab-case",
global_settings = & [AppSettings::DeriveDisplayOrder]
)]
/// Create and manage your Hypothesis knowledge-base
pub enum GooseberryCLI {
    /// Sync newly added or updated Hypothesis annotations.
    Sync,
    /// Tag annotations according to topic.
    Tag {
        #[structopt(flatten)]
        filters: Filters,
        /// Use this flag to remove the given tag from the filtered annotations instead of adding it
        #[structopt(short, long)]
        delete: bool,
        /// Open a search buffer to see and fuzzy search filtered annotations to further filter them
        #[structopt(short, long)]
        search: bool,
        /// Exact search (not fuzzy) - this works better for short (<4 letter) search terms
        #[structopt(short, long, conflicts_with = "search")]
        exact: bool,
        /// The tag to add to / remove from the filtered annotations
        tag: String,
    },
    /// Delete annotations in bulk, using filters and fuzzy search,
    /// either just from gooseberry or from both gooseberry and Hypothesis
    Delete {
        #[structopt(flatten)]
        filters: Filters,
        /// Open a search buffer to see and fuzzy search filtered annotations to further filter them
        #[structopt(short, long)]
        search: bool,
        /// Exact search (not fuzzy) - this works better for short (<4 letter) search terms
        #[structopt(short, long, conflicts_with = "search")]
        exact: bool,
        /// Also delete from Hypothesis.
        /// Without this flag, the "gooseberry_ignore" flag is added to the selected annotations to ensure that they are not synced by gooseberry in the future.
        /// If the flag is given then the annotations are also deleted from Hypothesis.
        #[structopt(short = "a", long)]
        hypothesis: bool,
        /// Don't ask for confirmation
        #[structopt(short, long)]
        force: bool,
    },
    /// View (optionally filtered) annotations
    View {
        #[structopt(flatten)]
        filters: Filters,
        /// Open a search buffer to see and fuzzy search filtered annotations to further filter them
        #[structopt(short, long, conflicts_with = "id")]
        search: bool,
        /// Exact search (not fuzzy) - this works better for short (<4 letter) search terms
        #[structopt(short, long, conflicts_with = "search", conflicts_with = "id")]
        exact: bool,
        /// View annotation by ID
        #[structopt(conflicts_with = "filters")]
        id: Option<String>,
    },
    /// Create and update your knowledge-base markdown files
    Make,
    /// Generate shell completions
    Complete {
        /// type of shell
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
    /// Clear all gooseberry data
    /// "ob oggle sobble obble"
    Clear {
        /// Don't ask for confirmation
        #[structopt(short, long)]
        force: bool,
    },
    /// Move (optionally filtered) annotations from a different hypothesis group to Gooseberry's
    /// Only moves annotations created by the current user
    Move {
        /// Group ID to move from
        group_id: String,
        #[structopt(flatten)]
        filters: Filters,
        /// Open a search buffer to see and fuzzy search filtered annotations to further filter them
        #[structopt(short, long)]
        search: bool,
        /// Exact search (not fuzzy) - this works better for short (<4 letter) search terms
        #[structopt(short, long, conflicts_with = "search", conflicts_with = "id")]
        exact: bool,
    },
}

/// CLI options for filtering annotations
#[derive(StructOpt, Debug)]
pub struct Filters {
    /// Only annotations created after this date and time
    /// Can be colloquial, e.g. "last Friday 8pm"
    #[structopt(long, parse(try_from_str = utils::parse_datetime))]
    pub from: Option<DateTime<Utc>>,
    /// Only annotations created before this date and time
    /// Can be colloquial, e.g. "last Friday 8pm"
    #[structopt(long, parse(try_from_str = utils::parse_datetime), conflicts_with = "from")]
    pub before: Option<DateTime<Utc>>,
    /// If true, includes annotations updated after --from or before --before (instead of just created)
    #[structopt(short, long)]
    pub include_updated: bool,
    /// Only annotations with this pattern in their URL
    /// Doesn't have to be the full URL, e.g. "wikipedia"
    #[structopt(default_value, long)]
    pub uri: String,
    /// Only annotations with this pattern in their `quote`, `tags`, `text`, or `url`
    #[structopt(default_value, long)]
    pub any: String,
    /// Only annotations with these tags
    #[structopt(long)]
    pub tags: Vec<String>,
}

impl Into<SearchQuery> for Filters {
    fn into(self) -> SearchQuery {
        SearchQuery {
            limit: 200,
            search_after: match (self.from, self.before) {
                (Some(date), None) => date.to_rfc3339(),
                (None, Some(date)) => date.to_rfc3339(),
                (None, None) => crate::MIN_DATE.to_string(),
                _ => panic!("can't use both --from and --before"),
            },
            uri_parts: self.uri,
            any: self.any,
            tags: self.tags,
            order: if self.before.is_some() {
                Order::Desc
            } else {
                Order::Asc
            },
            sort: if self.include_updated {
                Sort::Updated
            } else {
                Sort::Created
            },
            ..Default::default()
        }
    }
}

impl GooseberryCLI {
    /// Generate shell completions for gooseberry
    pub fn complete(shell: Shell) {
        Self::clap().gen_completions_to(NAME, shell, &mut io::stdout());
    }
}

/// CLI options related to configuration management
#[derive(StructOpt, Debug)]
pub enum ConfigCommand {
    /// Prints / writes the default configuration options.
    /// Set the generated config file as default by setting the $GOOSEBERRY_CONFIG environment variable
    Default {
        /// Write to (TOML-formatted) file
        #[structopt(parse(from_os_str))]
        file: Option<PathBuf>,
    },
    /// Prints current configuration
    Get,
    /// Prints location of currently set configuration file
    Where,
    /// Change Hypothesis credentials
    Authorize,
    /// Change the group used for Hypothesis annotations
    Group,
}

impl ConfigCommand {
    /// Handle config related commands
    pub async fn run(&self) -> color_eyre::Result<()> {
        match self {
            Self::Default { file } => {
                GooseberryConfig::default_config(file.as_deref())?;
            }
            Self::Get => {
                GooseberryConfig::load().await?;
                println!("{}", GooseberryConfig::get()?);
            }
            Self::Where => {
                GooseberryConfig::print_location()?;
            }
            Self::Authorize => {
                let mut config = GooseberryConfig::load().await?;
                config.request_credentials().await?;
            }
            Self::Group => {
                let mut config = GooseberryConfig::load().await?;
                config.set_group().await?;
            }
        }
        Ok(())
    }
}
