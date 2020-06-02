use std::collections::HashSet;
use std::fs;

use color_eyre::Help;
use dialoguer::Confirm;
use hypothesis::annotations::{Annotation, InputAnnotation, Order, SearchQuery};
use hypothesis::Hypothesis;

use crate::configuration::GooseberryConfig;
use crate::errors::Apologize;
use crate::gooseberry::cli::{ConfigCommand, Filters, GooseberryCLI};

pub mod cli;
pub mod database;
pub mod markdown;
pub mod search;
pub mod tag;

pub struct Gooseberry {
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
    pub async fn start(cli: GooseberryCLI) -> color_eyre::Result<()> {
        if let GooseberryCLI::Config { cmd } = &cli {
            return Ok(ConfigCommand::run(cmd).await?);
        }
        if let GooseberryCLI::Complete { shell } = &cli {
            GooseberryCLI::complete(*shell);
            return Ok(());
        }
        let config = GooseberryConfig::load().await?;
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
        let gooseberry = Self {
            db: Self::get_db(&config.db_dir)?,
            api,
            config,
        };
        gooseberry.set_merge()?;
        gooseberry.run(cli).await?;
        Ok(())
    }

    async fn run(&self, cli: GooseberryCLI) -> color_eyre::Result<()> {
        match cli {
            GooseberryCLI::Sync => self.sync().await,
            GooseberryCLI::Tag {
                filters,
                delete,
                search,
                tag,
            } => self.tag(filters, delete, search, &tag).await,
            GooseberryCLI::Delete {
                filters,
                search,
                hypothesis,
                force,
            } => self.delete(filters, search, hypothesis, force).await,
            GooseberryCLI::Make => self.make().await,
            GooseberryCLI::Clear { force } => self.clear(force),
            _ => Ok(()), // Already handled
        }
    }

    async fn sync(&self) -> color_eyre::Result<()> {
        let mut query = SearchQuery {
            limit: 200,
            order: Order::Asc,
            search_after: self.get_sync_time()?,
            user: self.api.user.to_owned(),
            group: self.config.hypothesis_group.clone().unwrap(),
            ..Default::default()
        };
        let (added, updated, ignored) =
            self.sync_annotations(&self.api_search_annotations(&mut query).await?)?;
        self.set_sync_time(&query.search_after)?;
        if added > 0 {
            println!("Added {} new notes", added);
        }
        if updated > 0 {
            println!("Updated {} notes", updated);
        }
        if ignored > 0 {
            println!("Ignored {} notes", ignored);
        }
        Ok(())
    }

    async fn filter_annotations(&self, filters: Filters) -> color_eyre::Result<Vec<Annotation>> {
        let mut query: SearchQuery = filters.into();
        query.user = self.api.user.to_owned();
        query.group = self.config.hypothesis_group.clone().unwrap();
        Ok(self
            .api_search_annotations(&mut query)
            .await?
            .into_iter()
            .collect())
    }

    async fn tag(
        &self,
        filters: Filters,
        delete: bool,
        search: bool,
        tag: &str,
    ) -> color_eyre::Result<()> {
        let mut annotations: Vec<Annotation> = self
            .filter_annotations(filters)
            .await?
            .into_iter()
            .filter(|a| {
                if delete {
                    // only consider annotations with the tag
                    a.tags.contains(&tag.to_string())
                } else {
                    // don't consider annotations which already have the tag
                    !a.tags.contains(&tag.to_string())
                }
            })
            .collect();
        if search {
            // Run a search window for fuzzy search capability.
            let annotation_ids: HashSet<String> = Self::search(&annotations)?.collect();
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }
        if delete {
            self.delete_tag_from_annotations(annotations, tag).await?;
        } else {
            self.add_tag_to_annotations(annotations, tag).await?;
        }
        self.sync().await?;
        Ok(())
    }

    async fn delete(
        &self,
        filters: Filters,
        search: bool,
        hypothesis: bool,
        force: bool,
    ) -> color_eyre::Result<()> {
        let mut annotations = self.filter_annotations(filters).await?;
        if search {
            // Run a search window for fuzzy search capability.
            let annotation_ids: HashSet<String> = Self::search(&annotations)?.collect();
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }
        let num_annotations = annotations.len();
        if !annotations.is_empty()
            && (force
                || Confirm::new()
                    .with_prompt(&format!(
                        "Delete {} notes from gooseberry?",
                        num_annotations
                    ))
                    .default(false)
                    .interact()?)
        {
            let ids = annotations
                .iter()
                .map(|a| a.id.to_owned())
                .collect::<Vec<_>>();
            self.delete_annotations(&ids)?;
            if hypothesis
                && (force
                    || Confirm::new()
                        .with_prompt("Also delete from Hypothesis?")
                        .default(false)
                        .interact()?)
            {
                self.api.delete_annotations(&ids).await?;
                println!(
                    "{} notes deleted from gooseberry and Hypothesis",
                    num_annotations
                );
            } else {
                self.api
                    .update_annotations(
                        &ids,
                        &annotations
                            .into_iter()
                            .map(|a| {
                                let mut tags = a.tags;
                                tags.push(crate::IGNORE_TAG.to_owned());
                                InputAnnotation {
                                    tags: Some(tags),
                                    ..Default::default()
                                }
                            })
                            .collect::<Vec<_>>(),
                    )
                    .await?;
                println!("{} notes deleted from gooseberry.\n\
                 These still exist in Hypothesis but will be ignored in future `gooseberry sync` calls \
                 unless the \"gooseberry_ignore\" tag is removed.", num_annotations);
            }
        }
        Ok(())
    }

    /// Removes all `sled` trees
    fn clear(&self, force: bool) -> color_eyre::Result<()> {
        if force
            || Confirm::new()
                .with_prompt("Clear all gooseberry data?")
                .default(false)
                .interact()?
        {
            for path in fs::read_dir(&self.config.db_dir)? {
                let path = path?.path();
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
            }
            self.reset_sync_time()?;
            Ok(())
        } else {
            let error: color_eyre::Result<()> = Err(Apologize::DoingNothing.into());
            error.suggestion("Press Y next time!")
        }
    }

    /// Retrieve all annotations matching query
    pub async fn api_search_annotations(
        &self,
        query: &mut SearchQuery,
    ) -> color_eyre::Result<Vec<Annotation>> {
        let mut annotations = Vec::new();
        loop {
            let next = self.api.search_annotations(&query).await?;
            if next.is_empty() {
                break;
            }
            query.search_after = next[next.len() - 1].updated.to_rfc3339();
            annotations.extend_from_slice(&next);
        }
        Ok(annotations)
    }
}
