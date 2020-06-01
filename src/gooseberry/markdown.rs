use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use mdbook::MDBook;
use url::Url;

use hypothesis::annotations::{Annotation, Selector};

use crate::gooseberry::Gooseberry;
use crate::utils;
use crate::EMPTY_TAG;

impl Gooseberry {
    pub async fn make(&self) -> color_eyre::Result<()> {
        self.make_book_toml()?;
        let src_dir = self.config.kb_dir.join("src");
        if src_dir.exists() {
            fs::remove_dir_all(&src_dir)?;
        }
        fs::create_dir(&src_dir)?;
        self.start_mermaid()?;
        self.make_book(&src_dir).await?;
        let book = MDBook::load(&self.config.kb_dir);
        assert!(book.is_ok());
        assert!(book.unwrap().build().is_ok());
        Ok(())
    }

    fn start_mermaid(&self) -> color_eyre::Result<()> {
        Command::new("cargo")
            .arg("mdbook-mermaid")
            .arg(&self.config.kb_dir)
            .output()?;
        Ok(())
    }

    fn annotation_to_md(&self, annotation: &Annotation) -> color_eyre::Result<String> {
        let quote = annotation
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
            .join("\n");
        let incontext = annotation.links.get("incontext").unwrap_or(&annotation.uri);
        let text = annotation
            .text
            .split('\n')
            .map(|t| format!("{}\n", t))
            .collect::<String>();
        let tags: String = if annotation.tags.is_empty() {
            String::new()
        } else {
            format!(
                "|{}|",
                annotation
                    .tags
                    .iter()
                    .map(|tag| format!(" **[{}]({}.md)** ", tag, tag))
                    .collect::<Vec<_>>()
                    .join("|")
            )
        };
        let base_url = utils::base_url(Url::parse(&annotation.uri)?);
        let incontext = match base_url {
            Some(url) => format!("[[_see in context at {}_]({})]", url.as_str(), incontext),
            None => format!("[[_see in context_]({})]", incontext),
        };
        let annotation = if quote.trim().is_empty() {
            format!("{}\n{}\n{}\n", tags, text, incontext)
        } else {
            format!("{}\n{}\n\n{}\n{}\n", tags, quote, text, incontext)
        };
        Ok(annotation)
    }

    fn make_book_toml(&self) -> color_eyre::Result<()> {
        let book_toml = self.config.kb_dir.join("book.toml");
        if book_toml.exists() {
            return Ok(());
        }

        let book_toml_string = format!(
            "[book]\ntitle = \"Gooseberry\"\nauthors=[\"{}\"]",
            self.api.username
        );

        fs::File::create(book_toml)?.write_all(book_toml_string.as_bytes())?;
        Ok(())
    }

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
                annotations_string.push_str(&self.annotation_to_md(annotation)?);
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
            let link_string = if tag == EMPTY_TAG {
                format!("- [Untagged]({}.md)\n", tag)
            } else {
                format!("- [{}]({}.md)\n", tag, tag)
            };
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
