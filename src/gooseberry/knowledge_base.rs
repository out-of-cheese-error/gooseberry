use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use color_eyre::Help;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Confirm;
use handlebars::{Handlebars, RenderError};
use hypothesis::annotations::Annotation;
use serde::Serialize;
use serde_json::Value as Json;
use url::Url;

use crate::configuration::OrderBy;
use crate::errors::Apologize;
use crate::gooseberry::cli::Filters;
use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::utils::uri_to_filename;
use crate::EMPTY_TAG;

/// To convert an annotation to markdown
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnnotationTemplate {
    #[serde(flatten)]
    pub annotation: Annotation,
    pub base_uri: String,
    pub incontext: String,
    pub highlight: Vec<String>,
    pub display_name: Option<String>,
}

pub fn replace_spaces(astring: &str) -> String {
    astring.replace(" ", "\\ ")
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
        AnnotationTemplate {
            annotation,
            base_uri,
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

pub(crate) fn get_handlebars<'a>(
    annotation_template: &'a str,
    index_link_template: &'a str,
) -> color_eyre::Result<Handlebars<'a>> {
    let mut hbs = Handlebars::new();
    hbs.register_escape_fn(handlebars::no_escape);
    hbs.register_helper("date_format", Box::new(date_format));
    hbs.register_template_string("annotation", annotation_template)?;
    hbs.register_template_string("index_link", index_link_template)?;
    Ok(hbs)
}

fn get_index_link_data(
    path: &Path,
    src_dir: &Path,
) -> color_eyre::Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    map.insert(
        "name".to_string(),
        path.file_stem()
            .unwrap_or_else(|| "EMPTY".as_ref())
            .to_string_lossy()
            .to_string(),
    );
    map.insert(
        "relative_path".to_string(),
        path.strip_prefix(&src_dir)?
            .to_str()
            .ok_or(Apologize::KBError {
                message: format!("{:?} has non-unicode characters", path),
            })?
            .to_string()
            .replace(' ', "%20"),
    );
    map.insert(
        "absolute_path".to_string(),
        path.to_str()
            .ok_or(Apologize::KBError {
                message: format!("{:?} has non-unicode characters", path),
            })?
            .to_string()
            .replace(' ', "%20"),
    );
    Ok(map)
}

/// ## Markdown generation
/// functions related to generating the `mdBook` wiki
impl Gooseberry {
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
    pub async fn make(&mut self, force: bool) -> color_eyre::Result<()> {
        self.configure_kb()?;
        let kb_dir = self.config.kb_dir.as_ref().unwrap();
        if kb_dir.exists()
            && (force
                || Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("Clear knowledge base directory?")
                    .default(true)
                    .interact()?)
        {
            fs::remove_dir_all(&kb_dir)?;
            fs::create_dir_all(&kb_dir)?;
        }

        let hbs = get_handlebars(
            self.config.annotation_template.as_ref().unwrap(),
            self.config.index_link_template.as_ref().unwrap(),
        )?;
        self.make_book(&kb_dir, &hbs).await?;
        Ok(())
    }

    fn group_annotations_by_order(
        &self,
        order: OrderBy,
        annotations: Vec<AnnotationTemplate>,
    ) -> HashMap<String, Vec<AnnotationTemplate>> {
        let mut order_to_annotations = HashMap::new();
        match order {
            OrderBy::Tag => {
                for annotation in annotations {
                    if annotation.annotation.tags.is_empty() {
                        order_to_annotations
                            .entry(EMPTY_TAG.to_owned())
                            .or_insert_with(Vec::new)
                            .push(annotation);
                    } else {
                        for tag in &annotation.annotation.tags {
                            order_to_annotations
                                .entry(tag.to_owned())
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
        }
        order_to_annotations
    }

    /// Write markdown files for wiki
    async fn make_book(&self, src_dir: &Path, hbs: &Handlebars<'_>) -> color_eyre::Result<()> {
        let pb = utils::get_spinner("Building knowledge base...");
        let extension = self.config.file_extension.as_ref().unwrap();
        let index_file = src_dir.join(format!(
            "{}.{}",
            self.config.index_name.as_ref().unwrap(),
            extension
        ));
        if index_file.exists() {
            // Initialize
            fs::remove_file(&index_file)?;
        }

        // Get all annotations
        let mut annotations = self.filter_annotations(Filters::default(), None).await?;
        annotations.sort_by(|a, b| a.created.cmp(&b.created));
        let annotations: Vec<_> = annotations
            .into_iter()
            .map(AnnotationTemplate::from_annotation)
            .collect();

        let order = self.config.hierarchy.as_ref().unwrap();
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
                        let path = PathBuf::from(format!(
                            "{}.{}",
                            folder.to_str().ok_or(Apologize::KBError {
                                message: format!("{:?} has non-unicode characters", folder)
                            })?,
                            extension
                        ));
                        index_links.push(
                            hbs.render("index_link", &get_index_link_data(&path, &src_dir)?)?,
                        );
                        fs::File::create(&path)?.write_all(
                            inner_annotations
                                .into_iter()
                                .map(|a| hbs.render("annotation", &a))
                                .collect::<Result<String, _>>()?
                                .as_bytes(),
                        )?;
                    } else {
                        if !folder.exists() {
                            fs::create_dir(&folder)?;
                        }
                        for (new_folder, annotations) in
                            self.group_annotations_by_order(order[depth], inner_annotations)
                        {
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
            // Make Index file
            fs::File::create(index_file)?
                .write_all(index_links.into_iter().collect::<String>().as_bytes())?;
        }
        pb.finish_with_message("Done!");
        println!(
            "Knowledge base built at: {:?}",
            self.config.kb_dir.as_ref().unwrap()
        );
        Ok(())
    }
}
