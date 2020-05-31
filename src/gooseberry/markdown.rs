use crate::gooseberry::Gooseberry;
use mdbook::config::Config;
use mdbook::MDBook;
use std::collections::HashMap;
use crate::utils;
use hypothesis::annotations::{Annotation, Selector};


impl Gooseberry {
    pub fn make(&self) -> color_eyre::Result<()> {
        // create a default config and change a couple things
        let mut cfg = Config::default();
        cfg.book.title = Some("Gooseberry".to_string());
        cfg.book.authors.push(self.api.username.to_string());

        let mut book = MDBook::init(&self.config.kb_dir)
            .create_gitignore(true)
            .with_config(cfg)
            .build()?;

        Ok(())
    }

    fn write_annotation(&self, annotation: &Annotation) -> color_eyre::Result<String> {
        let quote = annotation.target.iter().map(|target| {
            target.selector.iter().filter_map(|selector| {
                match selector {
                    Selector::TextQuoteSelector(selector) => {
                        Some(format!("> {}", selector.exact))
                    },
                    _ => None
                }
            }).join("\n")
        }).join("\n");
        let annotation = format!()

        Ok(String::new())
    }

    fn write_annotations(&self) -> color_eyre::Result<()> {
        let mut tag_graph = HashMap::new();
        for tag in self.tag_to_annotations()?.iter() {
            let (tag, annotation_ids) = tag?;
            let tag = utils::u8_to_str(&tag)?;
            let annotation_ids = utils::split_ids(&annotation_ids)?;

        }

        Ok(())
    }
}
