use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use color_eyre::Help;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Confirm;
use eyre::eyre;
use handlebars::{Handlebars, RenderError};
use hypothesis::annotations::Annotation;
use sanitize_filename::sanitize;
use serde::Serialize;
use serde_json::Value as Json;
use url::Url;

use crate::configuration::{
    OrderBy, DEFAULT_ANNOTATION_TEMPLATE, DEFAULT_INDEX_LINK_TEMPLATE, DEFAULT_PAGE_TEMPLATE,
};
use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::utils::{clean_uri, uri_to_filename};
use crate::EMPTY_TAG;

/// To convert an annotation to text
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnnotationTemplate {
    #[serde(flatten)]
    pub annotation: Annotation,
    pub base_uri: String,
    pub title: String,
    pub incontext: String,
    pub highlight: Vec<String>,
    pub display_name: Option<String>,
}

pub fn replace_spaces(astring: &str) -> String {
    astring.replace(' ', "\\ ")
}

impl AnnotationTemplate {
    pub(crate) fn from_annotation(annotation: Annotation) -> Self {
        let base_uri = if let Ok(uri) = Url::parse(&annotation.uri) {
            uri[..url::Position::BeforePath].to_string()
        } else {
            annotation.uri.to_string()
        };
        let incontext = annotation
            .links
            .get("incontext")
            .unwrap_or(&annotation.uri)
            .to_owned();
        let highlight = utils::get_quotes(&annotation)
            .into_iter()
            .map(|s| s.to_owned())
            .collect();
        let display_name = if let Some(user_info) = &annotation.user_info {
            user_info.display_name.clone()
        } else {
            None
        };
        let mut title = String::from("Untitled document");
        if let Some(document) = &annotation.document {
            if !document.title.is_empty() {
                title = document.title[0].to_owned();
            }
        }
        AnnotationTemplate {
            annotation,
            base_uri,
            title,
            incontext,
            highlight,
            display_name,
        }
    }
}

pub(crate) fn format_date<E: AsRef<str>>(
    format: E,
    date: &Json,
) -> Result<String, serde_json::Error> {
    let date: DateTime<Utc> = serde_json::from_value(date.clone())?;
    Ok(format!("{}", date.format(format.as_ref())))
}

handlebars_helper!(date_format: |format: str, date: Json| format_date(format, date).map_err(|e| RenderError::from_error("serde_json", e))?);

pub(crate) struct Templates<'a> {
    pub(crate) annotation_template: &'a str,
    pub(crate) page_template: &'a str,
    pub(crate) index_link_template: &'a str,
}

impl<'a> Default for Templates<'a> {
    fn default() -> Self {
        Templates {
            annotation_template: DEFAULT_ANNOTATION_TEMPLATE,
            page_template: DEFAULT_PAGE_TEMPLATE,
            index_link_template: DEFAULT_INDEX_LINK_TEMPLATE,
        }
    }
}

pub(crate) fn get_handlebars(templates: Templates) -> color_eyre::Result<Handlebars> {
    let mut hbs = Handlebars::new();
    handlebars_misc_helpers::register(&mut hbs);
    hbs.register_escape_fn(handlebars::no_escape);
    hbs.register_helper("date_format", Box::new(date_format));
    hbs.register_template_string("annotation", templates.annotation_template)?;
    hbs.register_template_string("page", templates.page_template)?;
    hbs.register_template_string("index_link", templates.index_link_template)?;
    Ok(hbs)
}

/// To convert an annotation to text
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LinkTemplate {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: String,
}

fn get_link_data(path: &Path, src_dir: &Path) -> color_eyre::Result<LinkTemplate> {
    Ok(LinkTemplate {
        name: path
            .file_stem()
            .unwrap_or_else(|| "EMPTY".as_ref())
            .to_string_lossy()
            .to_string(),
        relative_path: path
            .strip_prefix(&src_dir)?
            .to_str()
            .ok_or(Apologize::KBError {
                message: format!("{:?} has non-unicode characters", path),
            })?
            .to_string()
            .replace(' ', "%20"),
        absolute_path: path
            .to_str()
            .ok_or(Apologize::KBError {
                message: format!("{:?} has non-unicode characters", path),
            })?
            .to_string()
            .replace(' ', "%20"),
    })
}

/// To convert an annotation to text
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PageTemplate {
    #[serde(flatten)]
    pub link_data: LinkTemplate,
    pub annotations: Vec<String>,
    pub raw_annotations: Vec<AnnotationTemplate>,
}

fn group_annotations_by_order(
    order: OrderBy,
    annotations: Vec<AnnotationTemplate>,
    nested_tag: Option<&String>,
) -> HashMap<String, Vec<AnnotationTemplate>> {
    let mut order_to_annotations = HashMap::new();
    match order {
        OrderBy::Tag => {
            let path_separator = &std::path::MAIN_SEPARATOR.to_string();
            for annotation in annotations {
                if annotation.annotation.tags.is_empty() {
                    order_to_annotations
                        .entry(EMPTY_TAG.to_owned())
                        .or_insert_with(Vec::new)
                        .push(annotation);
                } else {
                    for tag in &annotation.annotation.tags {
                        let mut tag = tag.to_owned();
                        if let Some(nested_tag) = nested_tag {
                            tag = tag.replace(nested_tag, path_separator);
                        }
                        order_to_annotations
                            .entry(tag)
                            .or_insert_with(Vec::new)
                            .push(annotation.clone());
                    }
                }
            }
        }
        OrderBy::URI => {
            for annotation in annotations {
                order_to_annotations
                    .entry(uri_to_filename(&annotation.annotation.uri))
                    .or_insert_with(Vec::new)
                    .push(annotation);
            }
        }
        OrderBy::Title => {
            for annotation in annotations {
                order_to_annotations
                    .entry(sanitize(&annotation.title))
                    .or_insert_with(Vec::new)
                    .push(annotation);
            }
        }
        OrderBy::BaseURI => {
            for annotation in annotations {
                order_to_annotations
                    .entry(uri_to_filename(&annotation.base_uri))
                    .or_insert_with(Vec::new)
                    .push(annotation);
            }
        }
        OrderBy::ID => {
            for annotation in annotations {
                order_to_annotations
                    .entry(annotation.annotation.id.to_string())
                    .or_insert_with(Vec::new)
                    .push(annotation);
            }
        }
        OrderBy::Empty => panic!("Shouldn't happen"),
        _ => panic!("{} shouldn't occur in hierarchy", order),
    }
    order_to_annotations
}

fn sort_annotations(sort: &[OrderBy], annotations: &mut Vec<AnnotationTemplate>) {
    annotations.sort_by(|a, b| {
        sort.iter().fold(Ordering::Equal, |acc, &field| {
            acc.then_with(|| match field {
                OrderBy::Tag => a
                    .annotation
                    .tags
                    .join(",")
                    .cmp(&b.annotation.tags.join(",")),
                OrderBy::URI => clean_uri(&a.annotation.uri).cmp(&clean_uri(&b.annotation.uri)),
                OrderBy::BaseURI => clean_uri(&a.base_uri).cmp(&clean_uri(&b.base_uri)),
                OrderBy::Title => a.title.cmp(&b.title),
                OrderBy::ID => a.annotation.id.cmp(&b.annotation.id),
                OrderBy::Created => format!("{}", a.annotation.created.format("%+"))
                    .cmp(&format!("{}", b.annotation.created.format("%+"))),
                OrderBy::Updated => format!("{}", a.annotation.updated.format("%+"))
                    .cmp(&format!("{}", b.annotation.updated.format("%+"))),
                OrderBy::Empty => panic!("Shouldn't happen"),
            })
        })
    });
}

/// ## Markdown generation
/// functions related to generating the `mdBook` wiki
impl Gooseberry {
    pub(crate) fn get_handlebars(&self) -> color_eyre::Result<Handlebars> {
        get_handlebars(self.config.get_templates())
    }

    fn configure_kb(&mut self) -> color_eyre::Result<()> {
        if self.config.kb_dir.is_none() {
            self.config.set_kb_all()?;
        }
        if self.config.kb_dir.is_none() || !self.config.kb_dir.as_ref().unwrap().exists() {
            return Err(Apologize::ConfigError {
                message: "Knowledge base directory not set or does not exist.".into(),
            })
                .suggestion(
                    "Set and create the knowledge base directory using \'gooseberry config kb directory\'",
                );
        }
        Ok(())
    }

    /// Make mdBook wiki
    pub async fn make(
        &mut self,
        annotations: Vec<Annotation>,
        clear: bool,
        force: bool,
        make: bool,
        index: bool,
    ) -> color_eyre::Result<()> {
        self.configure_kb()?;
        let kb_dir = self
            .config
            .kb_dir
            .as_ref()
            .ok_or_else(|| eyre!("No knowledge base directory"))?;
        if clear
            && kb_dir.exists()
            && (force
                || Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("Clear knowledge base directory?")
                    .default(true)
                    .interact()?)
        {
            fs::remove_dir_all(&kb_dir)?;
            fs::create_dir_all(&kb_dir)?;
        }
        self.make_book(annotations, kb_dir, make, index).await?;
        Ok(())
    }
    /// Write markdown files for wiki
    async fn make_book(
        &self,
        annotations: Vec<Annotation>,
        src_dir: &Path,
        make: bool,
        index: bool,
    ) -> color_eyre::Result<()> {
        let mut annotations = annotations
            .into_iter()
            .map(AnnotationTemplate::from_annotation)
            .collect();
        let extension = self
            .config
            .file_extension
            .as_ref()
            .ok_or_else(|| eyre!("No file extension"))?;
        let index_file = src_dir.join(format!(
            "{}.{}",
            self.config
                .index_name
                .as_ref()
                .ok_or_else(|| eyre!("No index name"))?,
            extension
        ));
        if index && index_file.exists() {
            // Initialize
            fs::remove_file(&index_file)?;
        }

        // Register templates
        let hbs = self.get_handlebars()?;
        let pb = utils::get_spinner("Building knowledge base...");
        sort_annotations(
            self.config.sort.as_ref().unwrap_or(&vec![OrderBy::Created]),
            &mut annotations,
        );

        let order = self
            .config
            .hierarchy
            .as_ref()
            .ok_or_else(|| eyre!("No hierarchy"))?;
        if order.is_empty() {
            // Index file has all annotations
            fs::File::create(&index_file)?.write_all(
                annotations
                    .into_iter()
                    .map(|a| hbs.render("annotation", &a))
                    .collect::<Result<String, _>>()?
                    .as_bytes(),
            )?;
        } else {
            // Index file has links to each page
            let mut index_links = vec![];
            struct RecurseFolder<'s> {
                f: &'s dyn Fn(
                    &RecurseFolder,
                    Vec<AnnotationTemplate>,
                    PathBuf,
                    usize,
                    &mut Vec<String>,
                ) -> color_eyre::Result<()>,
            }
            let recurse_folder = RecurseFolder {
                f: &|recurse_folder, inner_annotations, folder, depth, index_links| {
                    if depth == order.len() {
                        let folder_name = folder.to_str().ok_or(Apologize::KBError {
                            message: format!("{:?} has non-unicode characters", folder),
                        })?;
                        let folder_name: String = folder_name
                            .chars()
                            .take(250.min(folder_name.len()))
                            .collect();
                        let path = PathBuf::from(format!("{}.{}", folder_name, extension));
                        let link_data = get_link_data(&path, src_dir)?;
                        if index {
                            index_links.push(hbs.render("index_link", &link_data)?);
                        }
                        if make {
                            let page_data = PageTemplate {
                                link_data,
                                annotations: inner_annotations
                                    .iter()
                                    .map(|a| hbs.render("annotation", &a))
                                    .collect::<Result<Vec<String>, _>>()?,
                                raw_annotations: inner_annotations,
                            };
                            // TODO: check if nested tags work on Windows
                            if let Some(prefix) = path.parent() {
                                fs::create_dir_all(prefix)?;
                            }
                            fs::File::create(&path)?
                                .write_all(hbs.render("page", &page_data)?.as_bytes())?;
                        }
                    } else {
                        if make && !folder.exists() {
                            fs::create_dir(&folder)?;
                        }
                        for (new_folder, annotations) in group_annotations_by_order(
                            order[depth],
                            inner_annotations,
                            self.config.nested_tag.as_ref(),
                        ) {
                            (recurse_folder.f)(
                                recurse_folder,
                                annotations,
                                folder.join(new_folder),
                                depth + 1,
                                index_links,
                            )?;
                        }
                    }
                    Ok(())
                },
            };
            // Make directory structure
            (recurse_folder.f)(
                &recurse_folder,
                annotations,
                PathBuf::from(src_dir),
                0,
                &mut index_links,
            )?;
            if index {
                // Make Index file
                fs::File::create(&index_file)?
                    .write_all(index_links.into_iter().collect::<String>().as_bytes())?;
            }
        }
        pb.finish_with_message("Done!");
        if make {
            println!(
                "Knowledge base built at: {:?}",
                self.config
                    .kb_dir
                    .as_ref()
                    .ok_or_else(|| eyre!("No knowledge base directory"))?
            );
        }
        if index {
            println!("Index file location: {:?}", index_file);
        }
        Ok(())
    }
}
