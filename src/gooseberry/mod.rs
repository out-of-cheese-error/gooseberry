use std::fs;

use color_eyre::Help;
use dialoguer::Confirm;
use hypothesis::annotations::{Annotation, Order, SearchQuery};
use hypothesis::Hypothesis;

use crate::configuration::GooseberryConfig;
use crate::errors::Apologize;
use crate::gooseberry::cli::{ConfigCommand, Filters, GooseberryCLI};
use crate::gooseberry::markdown::MarkdownAnnotation;

/// Command-line interface with `structopt`
pub mod cli;
/// `sled` database related
pub mod database;
/// Convert annotations to markdown for the `mdBook` wiki and for the terminal
pub mod markdown;
/// `skim`-based search capabilities
pub mod search;

/// Gooseberry database, API client, and configuration
pub struct Gooseberry {
    /// database storing annotations and links
    db: sled::Db,
    /// hypothesis API client
    api: hypothesis::Hypothesis,
    /// configuration for directories and Hypothesis authorization
    config: GooseberryConfig,
}

/// ## CLI
/// Functions related to handling CLI commands
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

    /// Run knowledge-base related functions
    pub async fn run(&self, cli: GooseberryCLI) -> color_eyre::Result<()> {
        match cli {
            GooseberryCLI::Sync => self.sync().await,
            GooseberryCLI::Tag {
                filters,
                delete,
                search,
                exact,
                tag,
            } => self.tag(filters, delete, search, exact, &tag).await,
            GooseberryCLI::Delete {
                filters,
                search,
                exact,
                hypothesis,
                force,
            } => self.delete(filters, search, exact, hypothesis, force).await,
            GooseberryCLI::View {
                filters,
                search,
                exact,
                id,
            } => self.view(filters, search, exact, id).await,
            GooseberryCLI::Move {
                group_id,
                filters,
                search,
                exact,
            } => self.sync_group(group_id, filters, search, exact).await,
            GooseberryCLI::Make => self.make().await,
            GooseberryCLI::Clear { force } => self.clear(force),
            _ => Ok(()), // Already handled
        }
    }

    /// Sync newly added / updated annotations
    pub async fn sync(&self) -> color_eyre::Result<()> {
        let spinner = crate::utils::get_spinner("Syncing...");
        let mut query = SearchQuery::builder()
            .limit(200)
            .order(Order::Asc)
            .search_after(self.get_sync_time()?)
            .user(&self.api.user.0)
            .group(self.config.hypothesis_group.as_deref().unwrap())
            .build()?;
        let (added, updated, ignored) =
            self.sync_annotations(&self.api.search_annotations_return_all(&mut query).await?)?;
        self.set_sync_time(&query.search_after)?;
        spinner.finish_with_message("Done!");
        if added > 0 {
            if added == 1 {
                println!("Added 1 note");
            } else {
                println!("Added {} notes", added);
            }
        }
        if updated > 0 {
            if updated == 1 {
                println!("Updated 1 note");
            } else {
                println!("Updated {} notes", updated);
            }
        }
        if ignored > 0 {
            if ignored == 1 {
                println!("Ignored 1 note");
            } else {
                println!("Ignored {} notes", ignored);
            }
        }
        if added == 0 && updated == 0 && ignored == 0 {
            println!("Everything up to date!")
        }
        Ok(())
    }

    /// Move (optionally filtered) annotations from a different group to the group gooseberry looks at (set in config)
    pub async fn sync_group(
        &self,
        group_id: String,
        filters: Filters,
        search: bool,
        exact: bool,
    ) -> color_eyre::Result<()> {
        let mut annotations = self
            .filter_annotations(filters, Some(group_id.to_owned()))
            .await?;
        if search || exact {
            // Run a search window.
            let annotation_ids = Self::search(&annotations, exact)?;
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }
        let num = annotations.len();
        // Change the group ID attached to each annotation
        self.api
            .update_annotations(
                &annotations
                    .into_iter()
                    .map(|mut a| {
                        a.group = group_id.to_owned();
                        a
                    })
                    .collect::<Vec<_>>(),
            )
            .await?;
        if num > 0 {
            self.sync().await?;
        }
        Ok(())
    }

    /// Filter annotations based on command-line flags
    pub async fn filter_annotations(
        &self,
        filters: Filters,
        group: Option<String>,
    ) -> color_eyre::Result<Vec<Annotation>> {
        let mut query: SearchQuery = filters.into();
        query.user = self.api.user.0.to_owned();
        query.group = match group {
            Some(group) => group,
            None => self
                .config
                .hypothesis_group
                .clone()
                .expect("This should have been set by Config"),
        };
        let mut annotations: Vec<_> = self
            .api
            .search_annotations_return_all(&mut query)
            .await?
            .into_iter()
            .collect();
        annotations.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(annotations)
    }

    /// Tag a filtered set of annotations with a given tag
    pub async fn tag(
        &self,
        filters: Filters,
        delete: bool,
        search: bool,
        exact: bool,
        tag: &str,
    ) -> color_eyre::Result<()> {
        let mut annotations: Vec<Annotation> = self
            .filter_annotations(filters, None)
            .await?
            .into_iter()
            .filter(|a| {
                if delete {
                    // only consider annotations with the tag
                    a.tags.iter().any(|t| t == tag)
                } else {
                    // don't consider annotations which already have the tag
                    a.tags.iter().all(|t| t != tag)
                }
            })
            .collect();
        if search || exact {
            // Run a search window.
            let annotation_ids = Self::search(&annotations, exact)?;
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }
        let num = annotations.len();
        if delete {
            self.api
                .update_annotations(
                    &annotations
                        .into_iter()
                        .map(|mut a| {
                            a.tags.retain(|t| t != tag);
                            a
                        })
                        .collect::<Vec<_>>(),
                )
                .await?;
        } else {
            self.api
                .update_annotations(
                    &annotations
                        .into_iter()
                        .map(|mut a| {
                            a.tags.push(tag.to_owned());
                            a
                        })
                        .collect::<Vec<_>>(),
                )
                .await?;
        }
        if num > 0 {
            self.sync().await?;
        }
        Ok(())
    }

    /// Delete filtered annotations from gooseberry (by adding an ignore tag) or also from Hypothesis
    pub async fn delete(
        &self,
        filters: Filters,
        search: bool,
        exact: bool,
        hypothesis: bool,
        force: bool,
    ) -> color_eyre::Result<()> {
        let mut annotations = self.filter_annotations(filters, None).await?;
        if search || exact {
            // Run a search window.
            let annotation_ids = Self::search(&annotations, exact)?;
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
                annotations = annotations
                    .into_iter()
                    .map(|mut a| {
                        a.tags.push(crate::IGNORE_TAG.to_owned());
                        a
                    })
                    .collect();
                self.api.update_annotations(&annotations).await?;
                println!("{} notes deleted from gooseberry.\n\
                 These still exist in Hypothesis but will be ignored in future `gooseberry sync` calls \
                 unless the \"gooseberry_ignore\" tag is removed.", num_annotations);
            }
        }
        Ok(())
    }

    /// View optionally filtered annotations in the terminal
    pub async fn view(
        &self,
        filters: Filters,
        search: bool,
        exact: bool,
        id: Option<String>,
    ) -> color_eyre::Result<()> {
        if let Some(id) = id {
            let annotation = self
                .api
                .fetch_annotation(&id)
                .await
                .suggestion("Are you sure this is a valid and existing annotation ID?")?;
            let markdown = MarkdownAnnotation(&annotation).to_md(false);
            bat::PrettyPrinter::new()
                .language("markdown")
                .input_from_bytes(markdown.as_ref())
                .print()
                .unwrap();
            return Ok(());
        }

        let mut annotations: Vec<Annotation> = self
            .filter_annotations(filters, None)
            .await?
            .into_iter()
            .collect();
        if search || exact {
            // Run a search window.
            let annotation_ids = Self::search(&annotations, exact)?;
            annotations = annotations
                .into_iter()
                .filter(|a| annotation_ids.contains(&a.id))
                .collect();
        }
        let inputs: Vec<_> = annotations
            .into_iter()
            .map(|annotation| format!("\n{}\n---\n", MarkdownAnnotation(&annotation).to_md(false)))
            .collect();
        bat::PrettyPrinter::new()
            .language("markdown")
            .inputs(inputs.iter().map(|i| bat::Input::from_bytes(i.as_bytes())))
            .print()
            .unwrap();
        Ok(())
    }

    /// Removes all `sled` trees
    /// Deletes everything in the `db_dir`
    pub fn clear(&self, force: bool) -> color_eyre::Result<()> {
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
}
