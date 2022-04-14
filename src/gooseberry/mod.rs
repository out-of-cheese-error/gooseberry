use std::collections::HashSet;
use std::fs;

use color_eyre::Help;
use dialoguer::Confirm;
use eyre::eyre;
use hypothesis::annotations::{Annotation, Order, SearchQuery};
use hypothesis::Hypothesis;

use crate::configuration::GooseberryConfig;
use crate::errors::Apologize;
use crate::gooseberry::cli::{ConfigCommand, Filters, GooseberryCLI, GooseberrySubcommand};
use crate::gooseberry::knowledge_base::AnnotationTemplate;
use crate::utils;

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
        if let GooseberrySubcommand::Config { cmd } = &cli.cmd {
            return ConfigCommand::run(cmd, cli.config.as_deref()).await;
        }
        if let GooseberrySubcommand::Complete { shell } = &cli.cmd {
            GooseberryCLI::complete(*shell);
            return Ok(());
        }
        // Reads the GOOSEBERRY_CONFIG environment variable to get config file location
        let config = GooseberryConfig::load(cli.config.as_deref()).await?;
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
        match cli.cmd {
            GooseberrySubcommand::Sync => self.sync().await,
            GooseberrySubcommand::Search { filters, fuzzy } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters, None).await?;
                self.search(annotations, fuzzy).await
            }
            GooseberrySubcommand::Tag {
                filters,
                delete,
                tag,
            } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters, None).await?;
                let tags = if tag.is_empty() { None } else { Some(tag) };
                self.tag(annotations, delete, tags).await
            }
            GooseberrySubcommand::Delete { filters, force } => {
                let annotations = self.filter_annotations(filters, None).await?;
                self.delete(annotations, force).await
            }
            GooseberrySubcommand::View { filters, id } => self.view(filters, id).await,
            GooseberrySubcommand::Move {
                group_id,
                filters,
                search,
                fuzzy,
            } => self.sync_group(group_id, filters, search, fuzzy).await,
            GooseberrySubcommand::Make {
                filters,
                clear,
                force,
                no_index,
            } => {
                self.make(
                    self.filter_annotations_make(filters).await?,
                    clear,
                    force,
                    true,
                    !no_index,
                )
                .await
            }
            GooseberrySubcommand::Index { filters } => {
                self.make(
                    self.filter_annotations_make(filters).await?,
                    false,
                    false,
                    false,
                    true,
                )
                .await
            }
            GooseberrySubcommand::Clear { force } => self.clear(force),
            GooseberrySubcommand::Uri { filters, ids } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters, None).await?;
                self.uri(annotations, ids)
            }
            _ => Ok(()), // Already handled
        }
    }

    /// Sync newly added / updated annotations
    pub async fn sync(&self) -> color_eyre::Result<()> {
        let spinner = crate::utils::get_spinner("Syncing...");
        // Sleep to make sure the previous requests are processed
        let duration = core::time::Duration::from_millis(500);
        std::thread::sleep(duration);

        let mut query = SearchQuery::builder()
            .limit(200)
            .order(Order::Asc)
            .search_after(self.get_sync_time()?)
            .user(&self.api.user.0)
            .group(
                self.config
                    .hypothesis_group
                    .as_deref()
                    .ok_or_else(|| eyre!("No Hypothesis group"))?,
            )
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
        &mut self,
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
            let annotation_ids = self.search_group(&annotations, fuzzy)?;
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
        let group = match group {
            Some(group) => group,
            None => self
                .config
                .hypothesis_group
                .clone()
                .ok_or_else(|| eyre!("This should have been set by Config"))?,
        };
        let mut query: SearchQuery = filters.clone().into();
        query.user = self.api.user.0.to_owned();
        query.group = group.to_string();
        let mut annotations = if filters.or && !filters.tags.is_empty() {
            let mut annotations = Vec::new();
            for tag in &filters.tags {
                let mut tag_query = query.clone();
                tag_query.tags = vec![tag.to_string()];
                annotations.extend(
                    self.api
                        .search_annotations_return_all(&mut tag_query)
                        .await?,
                );
            }
            annotations
        } else {
            self.api.search_annotations_return_all(&mut query).await?
        };
        if !filters.exclude_tags.is_empty() {
            annotations.retain(|a| !a.tags.iter().any(|t| filters.exclude_tags.contains(t)));
        }
        if filters.page {
            annotations.retain(|a| a.target.iter().all(|t| t.selector.is_empty()));
        }
        if filters.annotation {
            annotations.retain(|a| a.target.iter().any(|t| !t.selector.is_empty()));
        }
        if filters.not {
            let mut query: SearchQuery = Filters::default().into();
            query.user = self.api.user.0.to_owned();
            query.group = group;
            let mut all_annotations: Vec<_> =
                self.api.search_annotations_return_all(&mut query).await?;
            let remove_ids = annotations.iter().map(|a| &a.id).collect::<HashSet<_>>();
            all_annotations.retain(|a| !remove_ids.contains(&a.id));
            annotations = all_annotations;
        }
        annotations.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(annotations)
    }

    /// Fetch annotations for knowledge base
    /// Ignores annotations with tags in `ignore_tags` configuration option.
    pub async fn filter_annotations_make(
        &self,
        filters: Filters,
    ) -> color_eyre::Result<Vec<Annotation>> {
        let pb = utils::get_spinner("Fetching annotations...");
        // Get all annotations
        let annotations: Vec<_> = self
            .filter_annotations(filters, None)
            .await?
            .into_iter()
            .filter(|a| {
                !a.tags.iter().any(|t| {
                    self.config
                        .ignore_tags
                        .as_ref()
                        .map(|ignore_tags| ignore_tags.contains(t))
                        .unwrap_or(false)
                })
            })
            .collect();
        pb.finish_with_message(format!("Fetched {} annotations", annotations.len()));
        Ok(annotations)
    }

    async fn add_tags(
        &self,
        annotations: Vec<Annotation>,
        tags: Vec<String>,
    ) -> color_eyre::Result<()> {
        let annotations: Vec<_> = annotations
            .into_iter()
            .filter(|a| tags.iter().all(|tag| !a.tags.contains(tag)))
            .collect();
        if annotations.is_empty() {
            println!("All of the selected annotations already have all of those tags.");
            return Ok(());
        }
        println!(
            "Adding {} tag(s) to {} annotation(s)",
            tags.len(),
            annotations.len()
        );
        self.api
            .update_annotations(
                &annotations
                    .clone()
                    .into_iter()
                    .map(|mut a| {
                        a.tags.extend_from_slice(&tags);
                        a
                    })
                    .collect::<Vec<_>>(),
            )
            .await?;

        self.sync().await?;

        Ok(())
    }

    async fn delete_tags(
        &self,
        annotations: Vec<Annotation>,
        tags: Vec<String>,
    ) -> color_eyre::Result<()> {
        let annotations: Vec<_> = annotations
            .into_iter()
            .filter(|a| tags.iter().any(|tag| a.tags.contains(tag)))
            .collect();
        if annotations.is_empty() {
            println!("None of the selected annotations have any of those tags.");
            return Ok(());
        }
        println!(
            "Deleting {} tag(s) from {} annotation(s)",
            tags.len(),
            annotations.len()
        );
        self.api
            .update_annotations(
                &annotations
                    .clone()
                    .into_iter()
                    .map(|mut a| {
                        a.tags.retain(|t| tags.iter().all(|tag| t != tag));
                        a
                    })
                    .collect::<Vec<_>>(),
            )
            .await?;
        self.sync().await?;
        Ok(())
    }
    /// Tag a filtered set of annotations with given tags
    pub async fn tag(
        &self,
        annotations: Vec<Annotation>,
        delete: bool,
        tags: Option<Vec<String>>,
    ) -> color_eyre::Result<()> {
        if annotations.is_empty() {
            println!("No matching annotations");
            return Ok(());
        }
        let tags = match tags {
            Some(tags) => tags,
            None => {
                if delete {
                    self.search_tags(&annotations, false)?
                } else {
                    self.search_tags(&annotations, true)?
                }
            }
        };
        if tags.is_empty() {
            println!("No tags selected");
            return Ok(());
        }

        if delete {
            self.delete_tags(annotations, tags).await?;
        } else {
            self.add_tags(annotations, tags).await?;
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
    pub async fn view(&mut self, filters: Filters, id: Option<String>) -> color_eyre::Result<()> {
        if self.config.annotation_template.is_none() {
            self.config.set_annotation_template()?;
        }
        let hbs = self.get_handlebars()?;
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
                .map_err(|_| eyre!("Bat printing error"))?;
            return Ok(());
        }
        let inputs: Vec<_> = self
            .filter_annotations(filters, None)
            .await?
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
            .map_err(|_| eyre!("Bat printing error"))?;
        Ok(())
    }

    pub fn uri(&self, annotations: Vec<Annotation>, ids: Vec<String>) -> color_eyre::Result<()> {
        let mut annotations = annotations;
        if !ids.is_empty() {
            annotations.retain(|a| ids.contains(&a.id));
        }
        let uris: HashSet<_> = annotations.into_iter().map(|a| a.uri).collect();
        for uri in uris {
            println!("{}", uri);
        }
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
