use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use dialoguer::console::style;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Confirm;
use hypothesis::annotations::Annotation;
use skim::prelude::{unbounded, Key, SkimOptionsBuilder};
use skim::{
    AnsiString, DisplayContext, ItemPreview, Matches, PreviewContext, Skim, SkimItem,
    SkimItemReceiver, SkimItemSender,
};

use crate::errors::Apologize;
use crate::gooseberry::knowledge_base::AnnotationTemplate;
use crate::gooseberry::Gooseberry;
use crate::utils;

/// searchable annotation information
#[derive(Debug)]
pub struct SearchAnnotation {
    /// Annotation ID
    id: String,
    /// Highlighted text, quote, URL, and tag information on a single line
    highlight: String,
    /// text, quote, URL, and tag information in markdown format
    markdown: String,
}

impl SkimItem for SearchAnnotation {
    fn text(&self) -> Cow<str> {
        AnsiString::parse(&self.highlight).into_inner()
    }

    fn display<'b>(&'b self, context: DisplayContext<'b>) -> AnsiString<'b> {
        let mut text = AnsiString::parse(&self.highlight);
        match context.matches {
            Matches::CharIndices(indices) => {
                text.override_attrs(
                    indices
                        .iter()
                        .map(|i| (context.highlight_attr, (*i as u32, (*i + 1) as u32)))
                        .collect(),
                );
            }
            Matches::CharRange(start, end) => {
                text.override_attrs(vec![(context.highlight_attr, (start as u32, end as u32))]);
            }
            Matches::ByteRange(start, end) => {
                let start = text.stripped()[..start].chars().count();
                let end = start + text.stripped()[start..end].chars().count();
                text.override_attrs(vec![(context.highlight_attr, (start as u32, end as u32))]);
            }
            Matches::None => (),
        }
        text
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        ItemPreview::Command(format!(
            "echo \"{}\" | bat -l markdown --color=always -p",
            self.markdown
        ))
    }
}

/// ## Search
/// `skim` search window functions
impl Gooseberry {
    /// Makes a skim search window for given annotations
    pub async fn search(
        &mut self,
        annotations: Vec<Annotation>,
        fuzzy: bool,
    ) -> color_eyre::Result<()> {
        let mut annotations = annotations;
        if self.config.annotation_template.is_none() {
            self.config.set_annotation_template()?;
        }
        let hbs = self.get_handlebars()?;
        let options = SkimOptionsBuilder::default()
            .height(Some("100%"))
            .preview(Some(""))
            .preview_window(Some("up:40%:wrap"))
            .bind(vec![
                "ctrl-a:select-all",
                "left:scroll-left",
                "right:scroll-right",
                "ctrl-c:abort",
                "shift-left:accept",
                "shift-right:accept",
                "shift-up:accept",
                "shift-down:accept",
                "Enter:accept"
            ])
            .exact(!fuzzy)
            .header(Some("Arrow keys to scroll, Tab to toggle selection, Ctrl-A to select all, Esc to abort\n\
            Enter to add a tag, Shift-Left to delete a tag, Shift-Right to delete annotation\n\
            Shift-Down to make knowledge-base files, Shift-Up to print the set of URIs"))
            .multi(true)
            .reverse(true)
            .build()
            .map_err(|_| Apologize::SearchError)?;

        let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();
        for annotation in &annotations {
            let mut title = String::from("Untitled document");
            if let Some(document) = &annotation.document {
                if !document.title.is_empty() {
                    title = document.title[0].to_owned();
                }
            }
            let mut highlight = format!(
                "{}",
                style(
                    self.config
                        .hypothesis_groups
                        .get(&annotation.group)
                        .unwrap_or(&annotation.group)
                        .replace('\n', " ")
                )
                .fg(dialoguer::console::Color::Yellow)
            );
            highlight.push_str(&format!(
                "| {}",
                style(title.replace('\n', " ")).fg(dialoguer::console::Color::Green)
            ));
            let quote = utils::get_quotes(annotation).join(" ").replace('\n', " ");
            if !quote.is_empty() {
                highlight.push_str(&format!("| {}", quote));
            }
            if !annotation.text.is_empty() {
                highlight.push_str(&format!("| {}", annotation.text.replace('\n', " ")));
            }
            if !annotation.tags.is_empty() {
                highlight.push_str(&format!(
                    "|{}",
                    style(&annotation.tags.join("|")).fg(dialoguer::console::Color::Red)
                ));
            }
            highlight.push_str(&format!(
                "| {}",
                style(&annotation.uri)
                    .fg(dialoguer::console::Color::Cyan)
                    .italic()
                    .underlined()
            ));
            let _ = tx_item.send(Arc::new(SearchAnnotation {
                highlight,
                markdown: hbs.render(
                    "annotation",
                    &AnnotationTemplate::from_annotation(
                        annotation.clone(),
                        &self.config.hypothesis_groups,
                    ),
                )?,
                id: annotation.id.to_owned(),
            }));
        }
        drop(tx_item); // so that skim could know when to stop waiting for more items.
        drop(hbs);
        if let Some(output) = Skim::run_with(&options, Some(rx_item)) {
            let annotation_ids: HashSet<String> = output
                .selected_items
                .into_iter()
                .map(|s| {
                    s.as_any()
                        .downcast_ref::<SearchAnnotation>()
                        .unwrap()
                        .id
                        .to_string()
                })
                .collect();
            annotations.retain(|a| annotation_ids.contains(&a.id));
            if annotations.is_empty() {
                println!("Nothing selected");
                return Ok(());
            }
            let key = output.final_key;
            match key {
                Key::Enter => {
                    let tags = self.search_tags(&annotations, true)?;
                    self.tag(annotations, false, Some(tags)).await?;
                }
                Key::ShiftLeft => {
                    let tags = self.search_tags(&annotations, false)?;
                    self.tag(annotations, true, Some(tags)).await?;
                }
                Key::ShiftRight => {
                    self.delete(annotations, false).await?;
                }
                Key::ShiftDown => {
                    let clear = Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("Clear knowledge base directory?")
                        .default(true)
                        .interact()?;
                    let index = Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("Also make index file?")
                        .default(true)
                        .interact()?;
                    self.make(annotations, clear, true, true, index)?;
                }
                Key::ShiftUp => {
                    self.uri(annotations, Vec::new())?;
                }
                _ => (),
            }
            Ok(())
        } else {
            Err(Apologize::SearchError.into())
        }
    }

    pub fn search_tags(
        &self,
        annotations: &[Annotation],
        add: bool,
    ) -> color_eyre::Result<Vec<String>> {
        let mut tags: Vec<String> = if add {
            // Get all tags
            self.tag_to_annotations()?
                .iter()
                .map(|t| t.map(|(tag_key, _)| std::str::from_utf8(&tag_key).map(|s| s.to_owned())))
                .collect::<Result<Result<HashSet<String>, _>, _>>()??
                .into_iter()
                .filter(|tag| {
                    // ignore tags which all given annotations have
                    !annotations.iter().all(|a| {
                        self.get_annotation_tags(&a.id)
                            .map(|a_tags| a_tags.contains(tag))
                            .unwrap_or(false)
                    })
                })
                .collect()
        } else {
            // Get tags present in given annotations
            annotations
                .iter()
                .map(|a| self.get_annotation_tags(&a.id))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flat_map(|tags| tags.into_iter())
                .collect::<HashSet<String>>()
                .into_iter()
                .collect()
        };
        tags.sort();

        let mut message = if add {
            "Select tags or create new comma-separated tags to add".to_owned()
        } else {
            "Select tags to remove".to_owned()
        };
        message.push_str("\nArrow keys to scroll, Tab to toggle selection, Ctrl-A to select all, Esc to abort, Enter to accept");
        let options = SkimOptionsBuilder::default()
            .height(Some("20%"))
            .exact(true)
            .header(Some(&message))
            .multi(true)
            .bind(vec!["Enter:accept"])
            .reverse(true)
            .build()
            .map_err(|_| Apologize::SearchError)?;

        let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();
        for tag in tags {
            let _ = tx_item.send(Arc::new(tag));
        }
        drop(tx_item); // so that skim could know when to stop waiting for more items.
        if let Some(output) = Skim::run_with(&options, Some(rx_item)) {
            let tags: HashSet<String> = output
                .selected_items
                .iter()
                .map(|i| i.output().to_string())
                .collect();
            let key = output.final_key;
            if let Key::Enter = key {
                return if tags.is_empty() && add {
                    Ok(output
                        .query
                        .split(',')
                        .map(|t| t.trim().to_owned())
                        .collect())
                } else {
                    Ok(tags.into_iter().collect())
                };
            }
            Ok(Vec::new())
        } else {
            Err(Apologize::SearchError.into())
        }
    }

    /// Makes a skim search window for given annotations from an external group
    pub fn search_group(
        &self,
        annotations: &[Annotation],
        fuzzy: bool,
    ) -> color_eyre::Result<HashSet<String>> {
        let hbs = self.get_handlebars()?;
        let options = SkimOptionsBuilder::default()
            .height(Some("100%"))
            .preview(Some(""))
            .preview_window(Some("up:40%:wrap"))
            .bind(vec![
                "ctrl-a:select-all",
                "left:scroll-left",
                "right:scroll-right",
                "ctrl-c:abort",
                "Enter:accept"
            ])
            .exact(!fuzzy)
            .header(Some("Arrow keys to scroll, Tab to toggle selection, Ctrl-A to select all, Ctrl-C to abort\
            Enter to select"))
            .multi(true)
            .reverse(true)
            .build()
            .map_err(|_| Apologize::SearchError)?;

        let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();
        for annotation in annotations {
            let highlight = format!(
                "{} | {} |{}| {}",
                style(&utils::get_quotes(annotation).join(" ").replace('\n', " ")),
                annotation.text.replace('\n', " "),
                style(&annotation.tags.join("|")).fg(dialoguer::console::Color::Red),
                style(&annotation.uri)
                    .fg(dialoguer::console::Color::Cyan)
                    .italic()
                    .underlined()
            );
            let _ = tx_item.send(Arc::new(SearchAnnotation {
                highlight,
                markdown: hbs.render(
                    "annotation",
                    &AnnotationTemplate::from_annotation(
                        annotation.clone(),
                        &self.config.hypothesis_groups,
                    ),
                )?,
                id: annotation.id.to_owned(),
            }));
        }
        drop(tx_item); // so that skim could know when to stop waiting for more items.

        if let Some(output) = Skim::run_with(&options, Some(rx_item)) {
            let key = output.final_key;
            match key {
                Key::Enter => Ok(output
                    .selected_items
                    .into_iter()
                    .map(|s| {
                        s.as_any()
                            .downcast_ref::<SearchAnnotation>()
                            .unwrap()
                            .id
                            .to_string()
                    })
                    .collect()),
                _ => Ok(HashSet::new()),
            }
        } else {
            Err(Apologize::SearchError.into())
        }
    }
}
