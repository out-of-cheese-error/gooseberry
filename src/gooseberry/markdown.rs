use std::fs;
use std::io::Write;
use std::path::PathBuf;

use mdbook::MDBook;

use hypothesis::annotations::{Annotation, Selector};

use crate::gooseberry::Gooseberry;
use crate::utils;

impl Gooseberry {
    pub async fn make(&self) -> color_eyre::Result<()> {
        self.make_book_toml()?;
        let src_dir = self.config.kb_dir.join("src");
        if src_dir.exists() {
            fs::remove_dir_all(&src_dir)?;
        }
        fs::create_dir(&src_dir)?;
        self.make_book(&src_dir).await?;
        let book = MDBook::load(&self.config.kb_dir);
        assert!(book.is_ok());
        assert!(book.unwrap().build().is_ok());
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
        let annotation = if quote.trim().is_empty() {
            format!("{}\n{}\n[[_see in context_]]({})\n", tags, text, incontext)
        } else {
            format!(
                "{}\n{}\n\n{}\n[[_see in context_]]({})\n",
                tags, quote, text, incontext
            )
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
        let mut summary_links = Vec::new();

        for tag in self.tag_to_annotations()?.iter() {
            let (tag, annotation_ids) = tag?;
            let tag = utils::u8_to_str(&tag)?;
            let annotation_ids = utils::split_ids(&annotation_ids)?;
            let annotations = self.api.fetch_annotations(&annotation_ids).await?;

            let mut tag_file = fs::File::create(src_dir.join(format!("{}.md", tag)))?;
            let mut annotations_string = format!("# {}\n", tag);
            annotations_string.push_str(
                &annotations
                    .iter()
                    .map(|a| self.annotation_to_md(a))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n---\n"),
            );
            tag_file.write_all(annotations_string.as_bytes())?;
            let link_string = format!("- [{}]({}.md)\n", tag, tag);
            summary_links.push(link_string);
        }
        let summary_links = summary_links.into_iter().collect::<String>();
        fs::File::create(summary)?.write_all(summary_links.as_bytes())?;
        Ok(())
    }
}
