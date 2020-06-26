use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use indicatif::{ProgressBar, ProgressStyle, ProgressIterator};

use hypothesis::annotations::{Annotation, Selector};
use mdbook::MDBook;
use url::Url;

use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::EMPTY_TAG;

/// To convert an annotation to markdown
#[derive(Debug)]
pub struct MarkdownAnnotation<'a>(pub &'a Annotation);

impl<'a> MarkdownAnnotation<'a> {
    /// Format the highlighted quote as a blockquote
    pub fn format_quote(&self) -> String {
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

    /// formats tags with '|'s in between
    pub fn format_tags(&self, with_links: bool) -> String {
        if self.0.tags.is_empty() {
            String::new()
        } else {
            format!(
                "|{}|",
                self.0
                    .tags
                    .iter()
                    .map(|tag| {
                        if with_links {
                            format!(" **[{}]({}.md)** ", tag, tag)
                        } else {
                            format!(" **{}** ", tag)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            )
        }
    }

    /// Format an annotation as markdown. Example:
    ///
    /// ##### Jun 1 23:53:30 2020 - *annotation ID*
    ///
    /// | tag1 | tag2 |
    /// > Highlighted quote from the website
    ///
    /// Comment about quote.
    /// **This** can be *arbitrary* markdown
    /// with LaTeX math $$\pi = 3.14$$.
    ///
    /// Source - *www.source_url.com*
    pub fn to_md(&self, with_links: bool) -> color_eyre::Result<String> {
        let quote = self.format_quote();
        let tags = self.format_tags(with_links);
        let incontext = self.0.links.get("incontext").unwrap_or(&self.0.uri);
        let base_url = if let Ok(uri) = Url::parse(&self.0.uri) {
            let uri = uri.to_owned();
            uri[..url::Position::BeforePath].to_owned()
        } else {
            self.0.uri.to_owned()
        };
        let incontext = if with_links {
            format!("[[*see in context at {}*]({})]", base_url, incontext)
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

/// ## Markdown generation
/// functions related to generating the `mdBook` wiki
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
        println!("Finished building book. Use mdbook serve {:?} and go to localhost:3000 to view it.", self.config.kb_dir);
        Ok(())
    }

    /// Sets up mermaid-js support
    /// Needs to already be installed
    pub fn start_mermaid(&self) -> color_eyre::Result<()> {
        Command::new("cargo")
            .arg("mdbook-mermaid")
            .arg(&self.config.kb_dir)
            .output()?;
        Ok(())
    }

    /// Write default book.toml if not present
    pub fn make_book_toml(&self) -> color_eyre::Result<()> {
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
    pub async fn make_book(&self, src_dir: &PathBuf) -> color_eyre::Result<()> {
        let pb = ProgressBar::new(self.tag_to_annotations().iter().len() as u64);
            
        pb.set_style(ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .progress_chars("##-"));
                
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

        for tag in self.tag_to_annotations()?.iter().progress_with(pb) {
            let (tag, annotation_ids) = tag?;
            let tag = std::str::from_utf8(&tag)?.to_owned();
            let annotation_ids = utils::split_ids(&annotation_ids)?;
            let mut annotations = self.api.fetch_annotations(&annotation_ids).await?;
            annotations.sort_by(|a, b| a.created.cmp(&b.created));
            let mut rel_tags = HashMap::new();


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
                    *tag_graph
                        .entry((tag.to_owned(), other_tag.to_owned()))
                        .or_insert(0_usize) += 1;
                    *rel_tags
                        .entry(other_tag.as_str())
                        .or_insert(0_usize) += 1;
                }
            }
            let mut rel_tags_count: Vec<_> = rel_tags.into_iter().collect();
            rel_tags_count.sort_by(|a, b| b.1.cmp(&a.1));
            let rel_tags_order: Vec<_> = rel_tags_count.into_iter().map(|x| format!("[{}]({}.md)", x.0, x.0)).collect();
            let rel_tags_string = rel_tags_order.join(", ");
            annotations_string.push_str(&rel_tags_string);
            
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

    /// Write index graph of tags in mermaid-js format
    pub fn make_mermaid_graph(
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
