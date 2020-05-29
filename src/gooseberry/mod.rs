use crate::configuration::GooseberryConfig;
use crate::errors::Apologize;
use crate::gooseberry::cli::{ConfigCommand, GooseberryCLI};
use hypothesis::Hypothesis;

pub mod cli;
pub mod database;

pub struct Gooseberry {
    /// StructOpt struct
    cli: GooseberryCLI,
    /// database storing annotations and links
    db: sled::Db,
    /// hypothesis API client
    api: hypothesis::Hypothesis,
    /// configuration for directories and Hypothesis authorization
    config: GooseberryConfig,
}

impl Gooseberry {
    /// Initialize program with command line input.
    /// Reads `sled` trees and metadata file from the locations specified in config.
    /// (makes new ones the first time).
    pub fn start(cli: GooseberryCLI) -> color_eyre::Result<()> {
        if let GooseberryCLI::Config { cmd } = &cli {
            return Ok(ConfigCommand::run(cmd)?);
        }
        if let GooseberryCLI::Complete { shell } = &cli {
            GooseberryCLI::complete(*shell);
            return Ok(());
        }
        let config = GooseberryConfig::load()?;
        let api = Hypothesis::new(
            config
                .hypothesis_username
                .as_deref()
                .ok_or(Apologize::ConfigError {
                    message: "Hypothesis username isn't stored".into(),
                })?,
            config
                .hypothesis_key
                .as_deref()
                .ok_or(Apologize::ConfigError {
                    message: "Hypothesis developer API key isn't stored".into(),
                })?,
        )?;
        let mut gooseberry = Self {
            db: Self::get_db(&config.db_dir)?,
            cli,
            api,
            config,
        };
        gooseberry.set_merge()?;
        gooseberry.run()?;
        Ok(())
    }

    pub fn run(&mut self) -> color_eyre::Result<()> {
        match &self.cli {
            GooseberryCLI::Sync => Ok(()),
            GooseberryCLI::Tag => Ok(()),
            GooseberryCLI::Make => Ok(()),
            _ => Ok(()), // Already handled
        }
    }
}
