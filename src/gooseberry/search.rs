//! skim-based search capabilities
use std::borrow::Cow;
use std::sync::Arc;

use console::{strip_ansi_codes, style};
use skim::prelude::{unbounded, SkimOptionsBuilder};
use skim::{AnsiString, ItemPreview, Skim, SkimItem, SkimItemReceiver, SkimItemSender};

use hypothesis::annotations::{Annotation, Selector};

use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;

/// searchable annotation information
#[derive(Debug)]
struct SearchAnnotation {
    /// Annotation ID
    id: String,
    /// Highlighted text, quote, URL, and tag information
    highlight: String,
    /// Plain text, quote, URL, and tag information
    plain: String,
}

impl<'a> SkimItem for SearchAnnotation {
    fn display(&self) -> Cow<AnsiString> {
        Cow::Owned(AnsiString::parse(&self.highlight))
    }

    fn text(&self) -> Cow<str> {
        Cow::Borrowed(&self.plain)
    }

    fn preview(&self) -> ItemPreview {
        ItemPreview::Text(
            "Arrow keys to scroll, TAB to toggle selection, CTRL-A to select all\nCTRL-C to abort, Enter to confirm"
                .into(),
        )
    }

    fn output(&self) -> Cow<str> {
        Cow::Borrowed(&self.id)
    }
}

impl From<&Annotation> for SearchAnnotation {
    fn from(annotation: &Annotation) -> Self {
        // Find highlighted text from `TextQuoteSelector`s
        let quotes: String = annotation
            .target
            .iter()
            .filter_map(|target| {
                let quotes = target
                    .selector
                    .iter()
                    .filter_map(|selector| match selector {
                        Selector::TextQuoteSelector(selector) => Some(selector.exact.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if quotes.is_empty() {
                    None
                } else {
                    Some(format!("{}", style(quotes.join(" ")).green()))
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let tags = style(annotation.tags.join("|")).red();
        let uri = style(&annotation.uri).cyan().italic().underlined();
        let highlight = format!("{} {} {} {}", quotes, annotation.text, tags, uri);
        let plain = strip_ansi_codes(&highlight).to_string();
        Self {
            highlight,
            plain,
            id: annotation.id.to_owned(),
        }
    }
}

impl Gooseberry {
    /// Makes a skim search window
    pub fn search(
        annotations: &[Annotation],
        exact: bool,
    ) -> color_eyre::Result<impl Iterator<Item = String>> {
        let options = SkimOptionsBuilder::default()
            .height(Some("70%"))
            .preview(Some(""))
            .preview_window(Some("down:10%"))
            .bind(vec![
                "ctrl-a:select-all",
                "left:scroll-left",
                "right:scroll-right",
            ])
            .exact(exact)
            .multi(true)
            .reverse(true)
            .build()
            .map_err(|_| Apologize::SearchError)?;

        let (tx_item, rx_item): (SkimItemSender, SkimItemReceiver) = unbounded();
        for annotation in annotations {
            let _ = tx_item.send(Arc::new(SearchAnnotation::from(annotation)));
        }
        drop(tx_item); // so that skim could know when to stop waiting for more items.

        Ok(Skim::run_with(&options, Some(rx_item))
            .map_or_else(Vec::new, |out| out.selected_items)
            .into_iter()
            .map(|s| s.output().to_string()))
    }
}
