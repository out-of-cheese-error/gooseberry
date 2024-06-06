use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::{fs, vec};

use chrono::Utc;
use color_eyre::Help;
use dialoguer::Confirm;
use eyre::eyre;
use hypothesis::annotations::{Annotation, Order, SearchQuery};
use hypothesis::{Hypothesis, UserAccountID};

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
    pub async fn new(config: GooseberryConfig) -> color_eyre::Result<Self> {
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
        let db = Self::get_db(&config.db_dir)?;
        let gooseberry = Self { db, api, config };
        gooseberry.set_merge()?;
        Ok(gooseberry)
    }

    pub async fn reset(config_file: Option<&Path>) -> color_eyre::Result<()> {
        let gooseberry = Self::new(GooseberryConfig::load(config_file).await?).await?;
        gooseberry.clear(true)?;
        let gooseberry = Self::new(GooseberryConfig::load(config_file).await?).await?;
        gooseberry.sync().await?;
        Ok(())
    }

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
        let mut gooseberry = Gooseberry::new(config).await?;
        gooseberry.run(cli).await?;
        Ok(())
    }

    /// Run knowledge-base related functions
    pub async fn run(&mut self, cli: GooseberryCLI) -> color_eyre::Result<()> {
        match cli.cmd {
            GooseberrySubcommand::Sync => self.sync().await,
            GooseberrySubcommand::Search { filters, fuzzy } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters)?;
                self.search(annotations, fuzzy).await
            }
            GooseberrySubcommand::Tag {
                filters,
                delete,
                tag,
            } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters)?;
                let tags = if tag.is_empty() { None } else { Some(tag) };
                self.tag(annotations, delete, tags).await
            }
            GooseberrySubcommand::Delete { filters, force } => {
                let annotations = self.filter_annotations(filters)?;
                self.delete(annotations, force).await
            }
            GooseberrySubcommand::View { filters, id } => self.view(filters, id),
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
            } => self.make(
                self.filter_annotations_make(filters)?,
                clear,
                force,
                true,
                !no_index,
            ),
            GooseberrySubcommand::Index { filters } => self.make(
                self.filter_annotations_make(filters)?,
                false,
                false,
                false,
                true,
            ),
            GooseberrySubcommand::Clear { force } => self.clear(force),
            GooseberrySubcommand::Uri { filters, ids } => {
                let annotations: Vec<Annotation> = self.filter_annotations(filters)?;
                self.uri(annotations, ids)
            }
            _ => Ok(()), // Already handled
        }
    }

    /// Sync newly added / updated annotations
    pub async fn sync(&self) -> color_eyre::Result<()> {
        let spinner = utils::get_spinner("Syncing...")?;
        // Sleep to make sure the previous requests are processed
        let duration = core::time::Duration::from_millis(500);
        std::thread::sleep(duration);

        let groups = self
            .config
            .hypothesis_groups
            .keys()
            .cloned()
            .collect::<Vec<_>>();

        if groups.is_empty() {
            spinner.finish_with_message("No groups to sync!");
            return Ok(());
        }
        let mut user_ids = vec![self.api.user.to_user_id()];
        if let Some(users) = &self.config.hypothesis_users {
            for user in users {
                user_ids.push(UserAccountID::from_str(user)?.to_user_id());
            }
        }
        let mut annotations = Vec::new();
        let sync_time = self.get_sync_time()?;
        for user_id in user_ids {
            let mut query = SearchQuery::builder()
                .limit(200)
                .order(Order::Asc)
                .search_after(&sync_time)
                .user(&user_id)
                .group(groups.clone())
                .build()?;
            annotations.extend(self.api.search_annotations_return_all(&mut query).await?);
        }
        let (added, updated) = self.sync_annotations(annotations)?;
        self.set_sync_time(&Utc::now().to_rfc3339())?;
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
            .filter_annotations_api(filters, vec![group_id.clone()], self.api.user.to_user_id())
            .await?;
        if search || fuzzy {
            // Run a search window.
            let annotation_ids = self.search_group(&annotations, fuzzy)?;
            annotations.retain(|a| annotation_ids.contains(&a.id))
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

    /// Filter annotations using hypothesis API based on command-line flags
    pub async fn filter_annotations_api(
        &self,
        filters: Filters,
        groups: Vec<String>,
        user_id: String,
    ) -> color_eyre::Result<Vec<Annotation>> {
        let mut query: SearchQuery = filters.clone().into();
        query.user = user_id.to_owned();
        query.group = groups.clone();
        let mut annotations = if !filters.and && !filters.tags.is_empty() {
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
            query.user = user_id;
            query.group = groups;
            let mut all_annotations: Vec<_> =
                self.api.search_annotations_return_all(&mut query).await?;
            let remove_ids = annotations.iter().map(|a| &a.id).collect::<HashSet<_>>();
            all_annotations.retain(|a| !remove_ids.contains(&a.id));
            annotations = all_annotations;
        }
        annotations.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(annotations)
    }

    pub fn filter_annotation(&self, annotation: &Annotation, filters: &Filters) -> bool {
        // Check if in groups
        if !filters.groups.is_empty()
            && !filters.groups.contains(&annotation.group)
            && !filters.groups.contains(
                self.config
                    .hypothesis_groups
                    .get(&annotation.group)
                    .unwrap_or(&annotation.group),
            )
        {
            return false;
        }

        // Check if page note
        if filters.page && annotation.target.iter().any(|t| !t.selector.is_empty()) {
            return false;
        }
        // Check if annotation
        if filters.annotation && annotation.target.iter().all(|t| t.selector.is_empty()) {
            return false;
        }
        // Check if date > from date
        if let Some(from) = filters.from {
            if filters.include_updated {
                if annotation.updated < from {
                    return false;
                }
            } else if annotation.created < from {
                return false;
            }
        }
        // Check if date < before date
        if let Some(before) = filters.before {
            if filters.include_updated {
                if annotation.updated > before {
                    return false;
                }
            } else if annotation.created > before {
                return false;
            }
        }
        // Check if URI has pattern
        if !filters.uri.is_empty() && !annotation.uri.contains(&filters.uri) {
            return false;
        }

        // Check if pattern in quote, tags, text, or URI
        if !(filters.any.is_empty()
            || utils::get_quotes(annotation)
                .join(" ")
                .contains(&filters.any)
            || annotation.tags.iter().any(|t| t.contains(&filters.any))
            || annotation.text.contains(&filters.any)
            || annotation.uri.contains(&filters.any))
        {
            return false;
        }

        // Check if tags overlap
        if !filters.tags.is_empty() {
            if filters.and {
                // all tags must match
                if !annotation.tags.iter().all(|t| filters.tags.contains(t)) {
                    return false;
                }
                // any tag can match
            } else if !annotation.tags.iter().any(|t| filters.tags.contains(t)) {
                return false;
            }
        }

        // Check if tags are in excluded tags
        if !filters.exclude_tags.is_empty()
            && annotation
                .tags
                .iter()
                .any(|t| filters.exclude_tags.contains(t))
        {
            return false;
        }

        // Check if pattern in quote
        if !filters.quote.is_empty()
            && !utils::get_quotes(annotation)
                .join(" ")
                .contains(&filters.quote)
        {
            return false;
        }

        // Check if pattern in text
        if !filters.text.is_empty() && !annotation.text.contains(&filters.text) {
            return false;
        }
        true
    }

    /// Filter annotations based on command-line flags
    pub fn filter_annotations(&self, filters: Filters) -> color_eyre::Result<Vec<Annotation>> {
        let mut annotations = Vec::new();
        for annotation in self.iter_annotations()? {
            let annotation = annotation?;
            let keep = self.filter_annotation(&annotation, &filters);
            if filters.not {
                // If NOT, keep everything that doesn't match
                if !keep {
                    annotations.push(annotation);
                }
            } else if keep {
                annotations.push(annotation);
            }
        }
        annotations.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(annotations)
    }

    /// Fetch annotations for knowledge base
    /// Ignores annotations with tags in `ignore_tags` configuration option.
    pub fn filter_annotations_make(&self, filters: Filters) -> color_eyre::Result<Vec<Annotation>> {
        let pb = utils::get_spinner("Fetching annotations...")?;
        // Get all annotations
        let annotations: Vec<_> = self
            .filter_annotations(filters)?
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
    pub fn view(&mut self, filters: Filters, id: Option<String>) -> color_eyre::Result<()> {
        if self.config.annotation_template.is_none() {
            self.config.set_annotation_template()?;
        }
        let hbs = self.get_handlebars()?;
        if let Some(id) = id {
            let annotation = self
                .get_annotation(&id)
                .suggestion("Are you sure this is a valid and existing annotation ID?")?;
            let markdown = hbs.render(
                "annotation",
                &AnnotationTemplate::from_annotation(annotation, &self.config.hypothesis_groups),
            )?;
            bat::PrettyPrinter::new()
                .language("markdown")
                .input_from_bytes(markdown.as_ref())
                .print()
                .map_err(|_| eyre!("Bat printing error"))?;
            return Ok(());
        }
        let inputs: Vec<_> = self
            .filter_annotations(filters)?
            .into_iter()
            .map(|annotation| {
                hbs.render(
                    "annotation",
                    &AnnotationTemplate::from_annotation(
                        annotation,
                        &self.config.hypothesis_groups,
                    ),
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
