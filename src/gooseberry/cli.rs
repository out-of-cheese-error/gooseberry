use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use hypothesis::annotations::{Order, SearchQuery, Sort};
use structopt::clap::AppSettings;
use structopt::clap::Shell;
use structopt::StructOpt;

use crate::configuration::GooseberryConfig;
use crate::utils;
use crate::NAME;

#[derive(Debug, StructOpt)]
#[structopt(
name = "gooseberry",
about = "Create and manage your Hypothesis knowledge-base",
rename_all = "kebab-case",
global_settings = & [AppSettings::DeriveDisplayOrder, AppSettings::ColoredHelp]
)]
/// Create and manage your Hypothesis knowledge-base
pub struct GooseberryCLI {
    /// Location of config file (uses default XDG location or environment variable if not given)
    #[structopt(short, long, parse(from_os_str), env = "GOOSEBERRY_CONFIG")]
    pub(crate) config: Option<PathBuf>,
    #[structopt(subcommand)]
    pub(crate) cmd: GooseberrySubcommand,
}

#[derive(StructOpt, Debug)]
pub enum GooseberrySubcommand {
    /// Sync newly added or updated Hypothesis annotations.
    Sync,
    /// Opens a search buffer to see, filter, delete, add tags to and delete tags from annotations
    Search {
        #[structopt(flatten)]
        filters: Filters,
        /// Toggle fuzzy search
        #[structopt(short, long)]
        fuzzy: bool,
    },
    /// Tag annotations according to topic.
    Tag {
        #[structopt(flatten)]
        filters: Filters,
        /// Use this flag to remove the given tag from the filtered annotations instead of adding it
        #[structopt(short, long)]
        delete: bool,
        /// The tags to add to / remove from the filtered annotations (comma-separated)
        #[structopt(use_delimiter = true)]
        tag: Vec<String>,
    },
    /// Delete annotations in bulk
    Delete {
        #[structopt(flatten)]
        filters: Filters,
        /// Don't ask for confirmation
        #[structopt(short, long)]
        force: bool,
    },
    /// View (optionally filtered) annotations
    View {
        #[structopt(flatten)]
        filters: Filters,
        /// View annotation by ID
        #[structopt(conflicts_with = "filters")]
        id: Option<String>,
    },
    /// Get the set of URIs from a list of (optionally filtered) annotations
    Uri {
        #[structopt(flatten)]
        filters: Filters,
        /// list of comma-separated annotation IDs
        #[structopt(use_delimiter = true)]
        ids: Vec<String>,
    },
    /// Create knowledge-base text files using optionally filtered annotations
    Make {
        #[structopt(flatten)]
        filters: Filters,
        /// Clear knowledge base directory before recreating
        #[structopt(short, long)]
        clear: bool,
        /// Don't ask for confirmation before clearing
        #[structopt(short, long, requires = "clear")]
        force: bool,
        /// Don't make index file
        #[structopt(short, long)]
        no_index: bool,
    },
    /// Create an index file using hierarchy and optionally filtered annotations
    Index {
        #[structopt(flatten)]
        filters: Filters,
    },
    /// Generate shell completions
    Complete {
        /// type of shell
        #[structopt(possible_values = & Shell::variants())]
        shell: Shell,
    },
    /// Manage configuration
    Config {
        #[structopt(subcommand)]
        cmd: ConfigCommand,
    },
    /// Clear all gooseberry data
    ///
    /// "ob oggle sobble obble"
    Clear {
        /// Don't ask for confirmation
        #[structopt(short, long)]
        force: bool,
    },
    /// Move (optionally filtered) annotations from a different hypothesis group to Gooseberry's
    ///
    /// Only moves annotations created by the current user
    Move {
        /// Group ID to move from
        group_id: String,
        #[structopt(flatten)]
        filters: Filters,
        /// Open a search buffer to see and search filtered annotations to further filter them
        #[structopt(short, long)]
        search: bool,
        /// Toggle fuzzy search
        #[structopt(short, long, conflicts_with = "search")]
        fuzzy: bool,
    },
}

/// CLI options for filtering annotations
#[derive(StructOpt, Debug, Default)]
pub struct Filters {
    /// Only annotations created after this date and time
    ///
    /// Can be colloquial, e.g. "last Friday 8pm"
    #[structopt(long, parse(try_from_str = utils::parse_datetime))]
    pub from: Option<DateTime<Utc>>,
    /// Only annotations created before this date and time
    ///
    /// Can be colloquial, e.g. "last Friday 8pm"
    #[structopt(long, parse(try_from_str = utils::parse_datetime), conflicts_with = "from")]
    pub before: Option<DateTime<Utc>>,
    /// If true, includes annotations updated after --from or before --before (instead of just created)
    #[structopt(short, long)]
    pub include_updated: bool,
    /// Only annotations with this pattern in their URL
    ///
    /// Doesn't have to be the full URL, e.g. "wikipedia"
    #[structopt(default_value, long)]
    pub uri: String,
    /// Only annotations with this pattern in their `quote`, `tags`, `text`, or `uri`
    #[structopt(default_value, long)]
    pub any: String,
    /// Only annotations with these tags
    #[structopt(long)]
    pub tags: Vec<String>,
}

impl From<Filters> for SearchQuery {
    fn from(filters: Filters) -> SearchQuery {
        SearchQuery {
            limit: 200,
            search_after: match (filters.from, filters.before) {
                (Some(date), None) | (None, Some(date)) => date.to_rfc3339(),
                (None, None) => crate::MIN_DATE.to_string(),
                _ => panic!("can't use both --from and --before"),
            },
            uri_parts: filters.uri,
            any: filters.any,
            tags: filters.tags,
            order: if filters.before.is_some() {
                Order::Desc
            } else {
                Order::Asc
            },
            sort: if filters.include_updated {
                Sort::Updated
            } else {
                Sort::Created
            },
            ..SearchQuery::default()
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
    ///
    /// Set the generated config file as default by setting the $GOOSEBERRY_CONFIG
    /// environment variable
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
    /// Change options related to the knowledge base
    Kb(KbConfigCommand),
}

#[derive(StructOpt, Debug)]
pub enum KbConfigCommand {
    /// Change everything related to the knowledge base
    All,
    /// Change knowledge base directory
    Directory,
    /// Change annotation handlebars template
    Annotation,
    /// Change page handlebars template
    Page,
    /// Change index link handlebars template
    Link,
    /// Change index file name
    Index,
    /// Change knowledge base file extension
    Extension,
    /// Change folder & file hierarchy
    Hierarchy,
    /// Change sort order of annotations within a page
    Sort,
    /// Set which tags to ignore
    Ignore,
}

impl ConfigCommand {
    /// Handle config related commands
    pub async fn run(&self, config_file: Option<&Path>) -> color_eyre::Result<()> {
        match self {
            Self::Default { file } => {
                GooseberryConfig::default_config(file.as_deref())?;
            }
            Self::Get => {
                GooseberryConfig::load(config_file).await?;
                println!("{}", GooseberryConfig::get(config_file)?);
            }
            Self::Where => {
                GooseberryConfig::print_location(config_file)?;
            }
            Self::Authorize => {
                let mut config = GooseberryConfig::load(config_file).await?;
                config.request_credentials().await?;
            }
            Self::Group => {
                let mut config = GooseberryConfig::load(config_file).await?;
                config.set_group().await?;
            }
            Self::Kb(cmd) => {
                let mut config = GooseberryConfig::load(config_file).await?;
                match cmd {
                    KbConfigCommand::All => config.set_kb_all()?,
                    KbConfigCommand::Directory => config.set_kb_dir()?,
                    KbConfigCommand::Annotation => config.set_annotation_template()?,
                    KbConfigCommand::Page => config.set_page_template()?,
                    KbConfigCommand::Link => config.set_index_link_template()?,
                    KbConfigCommand::Index => config.set_index_name()?,
                    KbConfigCommand::Extension => config.set_file_extension()?,
                    KbConfigCommand::Hierarchy => config.set_hierarchy()?,
                    KbConfigCommand::Sort => config.set_sort()?,
                    KbConfigCommand::Ignore => config.set_ignore_tags()?,
                };
            }
        }
        Ok(())
    }
}
