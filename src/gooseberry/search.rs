use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use console::style;
use hypothesis::annotations::{Annotation, Selector};
use skim::prelude::{unbounded, Key, SkimOptionsBuilder};
use skim::{
    AnsiString, DisplayContext, ItemPreview, Matches, PreviewContext, Skim, SkimItem,
    SkimItemReceiver, SkimItemSender,
};

use crate::errors::Apologize;
use crate::gooseberry::Gooseberry;

/// searchable annotation information
#[derive(Debug)]
pub struct SearchAnnotation {
    /// Annotation ID
    id: String,
    /// Highlighted text, quote, URL, and tag information
    highlight: String,
}

impl<'a> SkimItem for SearchAnnotation {
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
        ItemPreview::Text(
            "Arrow keys to scroll, TAB to toggle selection, CTRL-A to select all\nCTRL-C to abort, Enter to confirm"
                .into(),
        )
    }
}

impl From<&Annotation> for SearchAnnotation {
    /// Write annotation on a single line for searching
    /// Format: <highlighted quote in green> <comment in white> < '|' separated tags in red> <uri in cyan>  
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
        Self {
            highlight,
            id: annotation.id.to_owned(),
        }
    }
}

/// ## Search
/// `skim` search window functions
impl Gooseberry {
    /// Makes a skim search window for given annotations
    pub fn search(annotations: &[Annotation], exact: bool) -> color_eyre::Result<HashSet<String>> {
        let options = SkimOptionsBuilder::default()
            .height(Some("70%"))
            .preview(Some(""))
            .preview_window(Some("down:10%"))
            .bind(vec![
                "ctrl-a:select-all",
                "left:scroll-left",
                "right:scroll-right",
                "ctrl-c:abort",
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
