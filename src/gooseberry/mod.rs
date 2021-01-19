use std::fs;

use color_eyre::Help;
use dialoguer::Confirm;
use hypothesis::annotations::{Annotation, Order, SearchQuery};
use hypothesis::Hypothesis;

use crate::configuration::GooseberryConfig;
use crate::errors::Apologize;
use crate::gooseberry::cli::{ConfigCommand, Filters, GooseberryCLI};
use crate::gooseberry::knowledge_base::{get_handlebars, AnnotationTemplate};

/// Command-line interface with `structopt`
pub mod cli;
/// `sled` database related
pub mod database;
/// Convert annotations to text for the wiki and for the terminal
pub mod knowledge_base;
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
        let mut gooseberry = Self {
            db: Self::get_db(&config.db_dir)?,
            api,
            config,
        };
        gooseberry.set_merge()?;
        gooseberry.run(cli).await?;
        Ok(())
    }

    /// Run knowledge-base related functions
    pub async fn run(&mut self, cli: GooseberryCLI) -> color_eyre::Result<()> {
        match cli {
            GooseberryCLI::Sync => self.sync().await,
            GooseberryCLI::Search { filters, fuzzy } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters, None).await?;
                let hbs = get_handlebars(
                    self.config.annotation_template.as_ref().unwrap(),
                    self.config.index_link_template.as_ref().unwrap(),
                )?;
                self.search(annotations, &hbs, fuzzy).await
            }
            GooseberryCLI::Tag {
                filters,
                delete,
                tag,
            } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters, None).await?;
                self.tag(annotations, delete, tag).await
            }
            GooseberryCLI::Delete { filters, force } => {
                let annotations = self.filter_annotations(filters, None).await?;
                self.delete(annotations, force).await
            }
            GooseberryCLI::View { filters, id } => self.view(filters, id).await,
            GooseberryCLI::Move {
                group_id,
                filters,
                search,
                fuzzy,
            } => self.sync_group(group_id, filters, search, fuzzy).await,
            GooseberryCLI::Make { force } => self.make(force).await,
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
        let (added, updated) =
            self.sync_annotations(&self.api.search_annotations_return_all(&mut query).await?)?;
        self.set_sync_time(&query.search_after)?;
        spinner.finish_with_message("Done!");
        if added > 0 {
            if added == 1 {
                println!("Added 1 annotation");
            } else {
                println!("Added {} annotations", added);
            }
        }
        if updated > 0 {
            if updated == 1 {
                println!("Updated 1 annotation");
            } else {
                println!("Updated {} annotations", updated);
            }
        }
        if added == 0 && updated == 0 {
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
        fuzzy: bool,
    ) -> color_eyre::Result<()> {
        let mut annotations = self
            .filter_annotations(filters, Some(group_id.to_owned()))
            .await?;
        if search || fuzzy {
            // Run a search window.
            let hbs = get_handlebars(
                self.config.annotation_template.as_ref().unwrap(),
                self.config.index_link_template.as_ref().unwrap(),
            )?;
            let annotation_ids = Self::search_group(&annotations, &hbs, fuzzy)?;
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
        annotations: Vec<Annotation>,
        delete: bool,
        tag: Option<String>,
    ) -> color_eyre::Result<()> {
        let mut annotations = annotations;
        if !annotations.is_empty() {
            if delete {
                let tag = match tag {
                    Some(tag) => tag,
                    None => crate::utils::user_input("Tag to delete", None, false, false)?,
                };
                annotations = annotations
                    .into_iter()
                    .filter(|a| a.tags.contains(&tag))
                    .collect();
                if annotations.is_empty() {
                    println!("None of the selected annotations have that tag.");
                } else {
                    println!(
                        "Deleting tag `{}` from {} annotations",
                        tag,
                        annotations.len()
                    );
                    self.api
                        .update_annotations(
                            &annotations
                                .into_iter()
                                .map(|mut a| {
                                    a.tags.retain(|t| t != &tag);
                                    a
                                })
                                .collect::<Vec<_>>(),
                        )
                        .await?;
                    self.sync().await?;
                }
            } else {
                let tag = match tag {
                    Some(tag) => tag,
                    None => crate::utils::user_input("Tag to add", None, false, false)?,
                };
                annotations = annotations
                    .into_iter()
                    .filter(|a| !a.tags.contains(&tag))
                    .collect();
                if annotations.is_empty() {
                    println!("All of the selected annotations already have that tag.");
                } else {
                    println!("Adding tag `{}` to {} annotations", tag, annotations.len());
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
                    self.sync().await?;
                }
            }
        } else {
            println!("No matching annotations");
        }
        Ok(())
    }

    /// Delete filtered annotations from gooseberry (by adding an ignore tag) or also from Hypothesis
    pub async fn delete(
        &self,
        annotations: Vec<Annotation>,
        force: bool,
    ) -> color_eyre::Result<()> {
        let num_annotations = annotations.len();
        if !annotations.is_empty()
            && (force
                || Confirm::new()
                    .with_prompt(&format!("Delete {} annotations?", num_annotations))
                    .default(false)
                    .interact()?)
        {
            let ids = annotations
                .iter()
                .map(|a| a.id.to_owned())
                .collect::<Vec<_>>();
            self.delete_annotations(&ids)?;
            self.api.delete_annotations(&ids).await?;
            println!("{} annotations deleted", num_annotations);
        }
        Ok(())
    }

    /// View optionally filtered annotations in the terminal
    pub async fn view(&self, filters: Filters, id: Option<String>) -> color_eyre::Result<()> {
        let hbs = get_handlebars(
            self.config.annotation_template.as_ref().unwrap(),
            self.config.index_link_template.as_ref().unwrap(),
        )?;
        if let Some(id) = id {
            let annotation = self
                .api
                .fetch_annotation(&id)
                .await
                .suggestion("Are you sure this is a valid and existing annotation ID?")?;
            let markdown = hbs.render(
                "annotation",
                &AnnotationTemplate::from_annotation(annotation),
            )?;
            bat::PrettyPrinter::new()
                .language("markdown")
                .input_from_bytes(markdown.as_ref())
                .print()
                .unwrap();
            return Ok(());
        }
        let annotations: Vec<Annotation> = self
            .filter_annotations(filters, None)
            .await?
            .into_iter()
            .collect();
        let inputs: Vec<_> = annotations
            .into_iter()
            .map(|annotation| {
                hbs.render(
                    "annotation",
                    &AnnotationTemplate::from_annotation(annotation),
                )
            })
            .collect::<Result<_, _>>()?;
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
