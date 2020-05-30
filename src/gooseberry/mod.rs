use crate::configuration::GooseberryConfig;
use crate::errors::Apologize;
use crate::gooseberry::cli::{ConfigCommand, Filters, GooseberryCLI};
use color_eyre::Help;
use dialoguer::Confirm;
use hypothesis::annotations::{Annotation, Order, SearchQuery};
use hypothesis::Hypothesis;
use std::collections::HashSet;
use std::fs;

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
        let gooseberry = Self {
            db: Self::get_db(&config.db_dir)?,
            api,
            config,
        };
        gooseberry.set_merge()?;
        gooseberry.run(cli)?;
        Ok(())
    }

    pub fn run(&self, cli: GooseberryCLI) -> color_eyre::Result<()> {
        match cli {
            GooseberryCLI::Sync => self.sync(),
            GooseberryCLI::Tag {
                filters,
                delete,
                search,
                tag,
            } => self.tag(filters, delete, search, &tag),
            GooseberryCLI::Delete {
                filters,
                search,
                force,
            } => self.delete(filters, search, force),
            GooseberryCLI::Make => Ok(()),
            GooseberryCLI::Clear { force } => self.clear(force),
            _ => Ok(()), // Already handled
        }
    }

    fn sync(&self) -> color_eyre::Result<()> {
        let mut query = SearchQuery {
            limit: 200,
            order: Order::Asc,
            search_after: self.get_sync_time()?,
            user: self.api.user.to_owned(),
            group: self.config.hypothesis_group.clone().unwrap(),
            ..Default::default()
        };
        let (added, updated) = self.sync_annotations(&self.api_fetch_annotations(&mut query)?)?;
        self.set_sync_time(&query.search_after)?;
        println!(
            "Added {} new annotations\nUpdated {} annotations",
            added, updated
        );
        Ok(())
    }

    fn filter_annotations(&self, filters: Filters) -> color_eyre::Result<Vec<Annotation>> {
        let mut query: SearchQuery = filters.into();
        query.user = self.api.user.to_owned();
        query.group = self.config.hypothesis_group.clone().unwrap();
        Ok(self
            .api_fetch_annotations(&mut query)?
            .into_iter()
            .collect())
    }

    fn tag(
        &self,
        filters: Filters,
        delete: bool,
        search: bool,
        tag: &str,
    ) -> color_eyre::Result<()> {
        let mut annotations: Vec<Annotation> = self
            .filter_annotations(filters)?
            .into_iter()
            .filter(|a| {
                if delete {
                    // only consider annotations with the tag
                    a.tags
                        .as_deref()
                        .unwrap_or_default()
                        .contains(&tag.to_string())
                } else {
                    // don't consider annotations which already have the tag
                    !a.tags
                        .as_deref()
                        .unwrap_or_default()
                        .contains(&tag.to_string())
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
            let mut tag_batch = sled::Batch::default();
            let mut annotation_batch = sled::Batch::default();
            for annotation in annotations {
                self.delete_tag_from_annotation(
                    annotation,
                    &mut annotation_batch,
                    tag,
                    &mut tag_batch,
                )?;
            }
            self.annotation_to_tags()?.apply_batch(annotation_batch)?;
            self.tag_to_annotations()?.apply_batch(tag_batch)?;
        } else {
            for annotation in annotations {
                self.add_tag_to_annotation(annotation, tag)?;
            }
        }
        Ok(())
    }

    fn delete(&self, filters: Filters, search: bool, force: bool) -> color_eyre::Result<()> {
        let mut annotations = self.filter_annotations(filters)?;
        if search {
            // Run a search window for fuzzy search capability.
            let annotation_ids: HashSet<String> = Self::search(&annotations)?.collect();
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }
        if !annotations.is_empty()
            && (force
                || Confirm::new()
                    .with_prompt(&format!("Delete {} annotations?", annotations.len()))
                    .default(false)
                    .interact()?)
        {
            for annotation in annotations {
                self.delete_annotation(&annotation.id)?;
                self.api.delete_annotation(&annotation.id)?;
            }
        }
        Ok(())
    }

    /// Removes all `sled` trees
    fn clear(&self, force: bool) -> color_eyre::Result<()> {
        if force
            || Confirm::new()
                .with_prompt("Clear all data?")
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
    pub fn api_fetch_annotations(
        &self,
        query: &mut SearchQuery,
    ) -> color_eyre::Result<Vec<Annotation>> {
        let mut annotations = Vec::new();
        loop {
            let next = self.api.search_annotations(&query)?;
            if next.is_empty() {
                break;
            }
            query.search_after = next[next.len() - 1].updated.to_rfc3339();
            annotations.extend_from_slice(&next);
        }
        Ok(annotations)
    }
}
