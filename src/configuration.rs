use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use color_eyre::Help;
use directories_next::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::errors::Apologize;
use crate::{utils, NAME};
use hypothesis::{GroupID, Hypothesis};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GooseberryConfig {
    /// Directory to store `sled` database files
    pub(crate) db_dir: PathBuf,
    /// Directory to write out knowledge base markdown files
    pub(crate) kb_dir: PathBuf,
    /// Hypothesis username
    pub(crate) hypothesis_username: Option<String>,
    /// Hypothesis personal API key
    pub(crate) hypothesis_key: Option<String>,
    /// Hypothesis group with knowledge base annotations
    pub(crate) hypothesis_group: Option<GroupID>,
}

/// Main project directory, cross-platform
fn get_project_dir() -> color_eyre::Result<ProjectDirs> {
    Ok(ProjectDirs::from("rs", "", NAME).ok_or(Apologize::Homeless)?)
}

impl Default for GooseberryConfig {
    fn default() -> Self {
        let (db_dir, kb_dir) = {
            let dir = get_project_dir().expect("Couldn't get project dir");
            let data_dir = dir.data_dir();
            if !data_dir.exists() {
                fs::create_dir_all(data_dir).expect("Couldn't create data dir");
            }
            (data_dir.join("gooseberry_db"), data_dir.join("gooseberry"))
        };
        let config = Self {
            db_dir,
            kb_dir,
            hypothesis_username: None,
            hypothesis_key: None,
            hypothesis_group: None,
        };
        config.make_dirs().unwrap();
        config
    }
}

impl GooseberryConfig {
    pub(crate) fn default_config(file: Option<&Path>) -> color_eyre::Result<()> {
        let writer: Box<dyn io::Write> = match file {
            Some(file) => Box::new(fs::File::open(file)?),
            None => Box::new(io::stdout()),
        };
        let mut buffered = io::BufWriter::new(writer);
        let contents = "db_dir = 'gooseberry_db'\nkb_dir = 'gooseberry'\nusername = <Hypothesis username>\ndeveloper_key = <Developer key>";
        write!(&mut buffered, "{}", contents)?;
        Ok(())
    }

    pub(crate) fn print_config_location() -> color_eyre::Result<()> {
        println!("{}", GooseberryConfig::get()?.to_string_lossy());
        Ok(())
    }

    fn make_dirs(&self) -> color_eyre::Result<()> {
        if !self.db_dir.exists() {
            fs::create_dir(&self.db_dir).map_err(|e: io::Error| Apologize::ConfigError {
                message: format!(
                    "Couldn't create database directory {:?}, {}",
                    self.db_dir, e
                ),
            })?;
        }
        if !self.kb_dir.exists() {
            fs::create_dir(&self.kb_dir).map_err(|e: io::Error| Apologize::ConfigError {
                message: format!(
                    "Couldn't create knowledge base directory {:?}, {}",
                    self.kb_dir, e
                ),
            })?;
        }
        Ok(())
    }

    fn get_default_config_file() -> color_eyre::Result<PathBuf> {
        let dir = get_project_dir()?;
        let config_dir = dir.config_dir();
        Ok(config_dir.join(format!("{}.toml", NAME)))
    }

    /// Gets the current config file location
    fn get() -> color_eyre::Result<PathBuf> {
        let config_file = env::var("GOOSEBERRY_CONFIG").ok();
        match config_file {
            Some(file) => {
                let path = Path::new(&file).to_owned();
                if path.exists() {
                    Ok(path)
                } else {
                    let error: color_eyre::Result<PathBuf> = Err(Apologize::ConfigError {
                        message: format!("No such file {}", file),
                    }
                    .into());
                    error.suggestion(format!(
                        "Use `gooseberry config default {}` to write out the default configuration",
                        file
                    ))
                }
            }
            None => Self::get_default_config_file(),
        }
    }

    /// Read config from default location
    pub(crate) fn load() -> color_eyre::Result<Self> {
        // Reads the GOOSEBERRY_CONFIG environment variable to get config file location
        let config_file = env::var("GOOSEBERRY_CONFIG").ok();
        let mut config = match config_file {
            Some(file) => {
                let path = Path::new(&file).to_owned();
                if path.exists() {
                    let config: GooseberryConfig = confy::load_path(Path::new(&file))?;
                    config.make_dirs()?;
                    Ok(config)
                } else {
                    let error: color_eyre::Result<Self> = Err(Apologize::ConfigError {
                        message: format!("No such file {}", file),
                    }
                        .into());
                    error.suggestion(format!(
                        "Use `gooseberry config default {}` to write out the default configuration",
                        file
                    ))
                }
            }
            None => {
                Ok(confy::load(NAME).suggestion(Apologize::ConfigError {
                    message: "Couldn't load from the default config location, maybe you don't have access? \
                    Try running `gooseberry config default config_file.toml`, modify the generated file, \
                then `export GOOSEBERRY_CONFIG=<full/path/to/config_file.toml>`".into()
                })?)
            },
        }?;

        if config.hypothesis_username.is_none()
            || config.hypothesis_key.is_none()
            || !Self::authorize(
                config.hypothesis_username.as_deref().unwrap(),
                config.hypothesis_key.as_deref().unwrap(),
            )?
        {
            config.set_credentials()?;
        }

        if config.hypothesis_group.is_none() {
            config.set_group()?;
        }
        Ok(config)
    }

    fn set_group(&mut self) -> color_eyre::Result<()> {
        let group_name = utils::user_input(
            "Enter a group name to annotate with",
            Some(NAME),
            true,
            false,
        )?;
        config.hypothesis_group = Some(
            Hypothesis::new(
                config.hypothesis_username.as_deref().unwrap(),
                config.hypothesis_key.as_deref().unwrap(),
            )?
            .create_group(&group_name, Some("Gooseberry knowledge base annotations"))?
            .id,
        );
        config.store()?;
        Ok(())
    }

    fn authorize(name: &str, key: &str) -> color_eyre::Result<bool> {
        Ok(Hypothesis::new(name, key)?
            .fetch_user_profile()?
            .userid
            .is_some())
    }

    /// Asks user for Hypothesis credentials and sets them in the config
    pub(crate) fn request_credentials(&mut self) -> color_eyre::Result<()> {
        let (mut name, mut key) = (String::new(), String::new());
        loop {
            name = utils::user_input(
                "Hypothesis username",
                if name.is_empty() { None } else { Some(&name) },
                true,
                false,
            )?;
            key = utils::user_input(
                "Hypothesis developer API key",
                if key.is_empty() { None } else { Some(&key) },
                true,
                false,
            )?;
            if Self::authorize(&name, &key)? {
                self.hypothesis_username = Some(name);
                self.hypothesis_key = Some(key);
                self.store()?;
                return Ok(());
            } else {
                println!("Could not authorize your Hypothesis credentials, please try again.");
            }
        }
    }
    /// Reads the HYPOTHESIS_NAME and HYPOTHESIS_KEY environment variables to get Hypothesis credentials.
    /// If not present or invalid, requests credentials from user.
    fn set_credentials(&mut self) -> color_eyre::Result<()> {
        let (name, key) = (
            env::var("HYPOTHESIS_NAME").ok(),
            env::var("HYPOTHESIS_KEY").ok(),
        );
        if let (Some(n), Some(k)) = (&name, &key) {
            if Self::authorize(n, k)? {
                self.hypothesis_username = Some(n.to_owned());
                self.hypothesis_key = Some(k.to_owned());
                self.store()?;
            } else {
                println!(
                    "Authorization with environment variables did not work. Enter details below"
                );
                self.request_credentials()?;
            }
        } else {
            self.request_credentials()?;
        }
        Ok(())
    }

    /// Write possibly modified config
    pub(crate) fn store(&self) -> color_eyre::Result<()> {
        // Reads the GOOSEBERRY_CONFIG environment variable to get config file location
        let config_file = env::var("GOOSEBERRY_CONFIG").ok();
        match config_file {
            Some(file) => confy::store_path(Path::new(&file), &(*self).clone()).suggestion(Apologize::ConfigError {
                message: "The current config_file location does not seem to have write access. \
                   Use `export GOOSEBERRY_CONFIG=<full/path/to/config_file.toml>` to set a new location".into()
            })?,
            None => confy::store(NAME, &(*self).clone()).suggestion(Apologize::ConfigError {
                message: "The current config_file location does not seem to have write access. \
                    Use `export GOOSEBERRY_CONFIG=<full/path/to/config_file.toml>` to set a new location".into()
            })?,
        };
        Ok(())
    }
}
