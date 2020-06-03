//! Convert annotations to markdown for the wiki and for the terminal
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use mdbook::MDBook;
use url::Url;

use hypothesis::annotations::{Annotation, Selector};

use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::EMPTY_TAG;

#[derive(Debug)]
pub(crate) struct MarkdownAnnotation<'a>(pub(crate) &'a Annotation);

impl<'a> MarkdownAnnotation<'a> {
    fn format_quote(&self) -> String {
        self.0
            .target
            .iter()
            .map(|target| {
                target
                    .selector
                    .iter()
                    .filter_map(|selector| match selector {
                        Selector::TextQuoteSelector(selector) => {
                            Some(format!("> {}", selector.exact))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_tags(&self, with_links: bool) -> String {
        if self.0.tags.is_empty() {
            String::new()
        } else if with_links {
            format!(
                "|{}|",
                self.0
                    .tags
                    .iter()
                    .map(|tag| format!(" **[{}]({}.md)** ", tag, tag))
                    .collect::<Vec<_>>()
                    .join("|")
            )
        } else {
            format!(
                "|{}|",
                self.0
                    .tags
                    .iter()
                    .map(|tag| format!(" **{}** ", tag))
                    .collect::<Vec<_>>()
                    .join("|")
            )
        }
    }

    pub(crate) fn to_md(&self, with_links: bool) -> color_eyre::Result<String> {
        let quote = self.format_quote();
        let tags = self.format_tags(with_links);
        let incontext = self.0.links.get("incontext").unwrap_or(&self.0.uri);
        let base_url = utils::base_url(Url::parse(&self.0.uri)?);
        let incontext = if with_links {
            match base_url {
                Some(url) => format!("[[*see in context at {}*]({})]", url.as_str(), incontext),
                None => format!("[[*see in context*]({})]", incontext),
            }
        } else {
            format!("Source - *{}*", self.0.uri)
        };
        let date = self.0.created.format("%c");
        let formatted = if quote.trim().is_empty() {
            format!(
                "##### {} - *{}*\n\n{}\n{}\n\n{}\n",
                date, self.0.id, tags, self.0.text, incontext
            )
        } else {
            format!(
                "##### {} - *{}*\n\n{}\n{}\n\n{}\n\n{}\n",
                date, self.0.id, tags, quote, self.0.text, incontext
            )
        };
        Ok(formatted)
    }
}

impl Gooseberry {
    /// Make mdBook wiki
    pub async fn make(&self) -> color_eyre::Result<()> {
        self.make_book_toml()?;
        let src_dir = self.config.kb_dir.join("src");
        if src_dir.exists() {
            fs::remove_dir_all(&src_dir)?;
        }
        fs::create_dir(&src_dir)?;
        self.start_mermaid()?;
        self.make_book(&src_dir).await?;
        MDBook::load(&self.config.kb_dir)
            .map_err(|e| Apologize::MdBookError {
                message: format!("Couldn't load book: {:?}", e),
            })?
            .build()
            .map_err(|e| Apologize::MdBookError {
                message: format!("Couldn't build book: {:?}", e),
            })?;
        Ok(())
    }

    /// Sets up mermaid-js support
    /// Needs to already be installed
    fn start_mermaid(&self) -> color_eyre::Result<()> {
        Command::new("cargo")
            .arg("mdbook-mermaid")
            .arg(&self.config.kb_dir)
            .output()?;
        Ok(())
    }

    /// Write default book.toml if not present
    fn make_book_toml(&self) -> color_eyre::Result<()> {
        let book_toml = self.config.kb_dir.join("book.toml");
        if book_toml.exists() {
            return Ok(());
        }

        let book_toml_string = format!(
            "[book]\ntitle = \"Gooseberry\"\nauthors=[\"{}\"]\n[output.html]\nmathjax-support = true",
            self.api.username
        );

        fs::File::create(book_toml)?.write_all(book_toml_string.as_bytes())?;
        Ok(())
    }

    /// Write markdown files for wiki
    async fn make_book(&self, src_dir: &PathBuf) -> color_eyre::Result<()> {
        let summary = src_dir.join("SUMMARY.md");
        if summary.exists() {
            // Initialize
            fs::remove_file(&summary)?;
        }
        let index_page = src_dir.join("index.md");
        if index_page.exists() {
            // Initialize
            fs::remove_file(&index_page)?;
        }

        let mut summary_links = vec!["[Index](index.md)\n".to_string()];

        let mut tag_graph = HashMap::new();
        let mut tag_counts = HashMap::new();
        for tag in self.tag_to_annotations()?.iter() {
            let (tag, annotation_ids) = tag?;
            let tag = utils::u8_to_str(&tag)?;
            let annotation_ids = utils::split_ids(&annotation_ids)?;
            let mut annotations = self.api.fetch_annotations(&annotation_ids).await?;
            annotations.sort_by(|a, b| a.created.cmp(&b.created));

            let mut tag_file = fs::File::create(src_dir.join(format!("{}.md", tag)))?;
            let mut annotations_string = if tag == EMPTY_TAG {
                String::new()
            } else {
                format!("# {}\n", tag)
            };
            tag_counts.insert(tag.to_owned(), annotations.len());
            for annotation in &annotations {
                annotations_string.push_str(&MarkdownAnnotation(annotation).to_md(true)?);
                annotations_string.push_str("\n---\n");
                for other_tag in &annotation.tags {
                    if other_tag == &tag
                        || tag_graph.contains_key(&(other_tag.to_owned(), tag.to_owned()))
                    {
                        continue;
                    }
                    let count = tag_graph
                        .entry((tag.to_owned(), other_tag.to_owned()))
                        .or_insert(0usize);
                    *count += 1;
                }
            }
            tag_file.write_all(annotations_string.as_bytes())?;
            let link_string = format!("- [{}]({}.md)\n", tag, tag);
            summary_links.push(link_string);
        }
        fs::File::create(index_page)?
            .write_all(Self::make_mermaid_graph(&tag_graph, &tag_counts)?.as_bytes())?;
        let summary_links = summary_links.into_iter().collect::<String>();
        fs::File::create(summary)?.write_all(summary_links.as_bytes())?;
        Ok(())
    }

    fn make_mermaid_graph(
        tag_graph: &HashMap<(String, String), usize>,
        tag_counts: &HashMap<String, usize>,
    ) -> color_eyre::Result<String> {
        let mut graph = String::from("```mermaid\ngraph TD;\n");
        graph.push_str(
            &tag_counts
                .iter()
                .map(|(t, c)| {
                    if *c == 1 {
                        format!("    {}[\"{}<br/>1 note\"];\n", t, t)
                    } else {
                        format!("    {}[\"{}<br/>{} notes\"];\n", t, t, c)
                    }
                })
                .collect::<String>(),
        );
        for ((tag_1, tag_2), count) in tag_graph {
            if *count == 1 {
                graph.push_str(&format!("    {}-- 1 note ---{};\n", tag_1, tag_2));
            } else {
                graph.push_str(&format!("    {}-- {} notes ---{};\n", tag_1, count, tag_2));
            }
        }
        graph.push_str(
            &tag_counts
                .keys()
                .map(|t| format!("    click {} \"/{}.html\";\n", t, t))
                .collect::<String>(),
        );
        graph.push_str("```\n");
        Ok(graph)
    }
}
