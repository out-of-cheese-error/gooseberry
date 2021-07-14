use chrono::{DateTime, Local, Utc};
use chrono_english::{parse_date_string, Dialect};
use color_eyre::Section;
use dialoguer::{theme, Editor, Input};
use hypothesis::annotations::Selector;
use url::Url;

use crate::errors::Apologize;

/// ASCII code of semicolon
pub const SEMICOLON: u8 = 59;

/// Makes `DateTime` from a string, can be colloquial like "last Friday 8pm"
pub fn parse_datetime(datetime_string: &str) -> color_eyre::Result<DateTime<Utc>> {
    if datetime_string.to_ascii_lowercase() == "today" {
        Ok(Local::now().date().and_hms(0, 0, 0).with_timezone(&Utc))
    } else {
        Ok(parse_date_string(datetime_string, Local::now(), Dialect::Uk)?.with_timezone(&Utc))
    }
}

/// Splits byte array by semicolon into list of Annotation IDs
pub fn split_ids(index_list: &[u8]) -> color_eyre::Result<Vec<String>> {
    let index_list_string = std::str::from_utf8(index_list)?;
    Ok(index_list_string
        .split(';')
        .map(|x| x.to_string())
        .collect())
}

/// List of String into semicolon-joined byte array
pub fn join_ids(index_list: &[String]) -> color_eyre::Result<Vec<u8>> {
    Ok(index_list.join(";").as_bytes().to_vec())
}

/// Takes user input from terminal, optionally has a default and optionally displays it.
pub fn user_input(
    message: &str,
    default: Option<&str>,
    show_default: bool,
    allow_empty: bool,
) -> color_eyre::Result<String> {
    match default {
        Some(default) => Ok(Input::with_theme(&theme::ColorfulTheme::default())
            .with_prompt(message)
            .default(default.to_owned())
            .show_default(show_default)
            .allow_empty(allow_empty)
            .interact()?
            .trim()
            .to_owned()),
        None => Ok(
            Input::<String>::with_theme(&theme::ColorfulTheme::default())
                .with_prompt(message)
                .allow_empty(allow_empty)
                .interact()?
                .trim()
                .to_owned(),
        ),
    }
}

/// Gets input from external editor, optionally displays default text in editor
pub fn external_editor_input(default: Option<&str>, extension: &str) -> color_eyre::Result<String> {
    Editor::new()
        .trim_newlines(false)
        .extension(extension)
        .edit(default.unwrap_or(""))
        .suggestion("Set your default editor using the $EDITOR or $VISUAL environment variables")?
        .ok_or(Apologize::EditorError)
        .suggestion("Make sure to save next time!")
}

pub fn get_spinner(message: &str) -> indicatif::ProgressBar {
    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.enable_steady_tick(200);
    spinner.set_style(
        indicatif::ProgressStyle::default_spinner()
            .tick_chars("/|\\- ")
            .template("{spinner:.dim.bold.blue} {wide_msg}"),
    );
    spinner.set_message(message.to_owned());
    spinner
}

pub fn get_quotes(annotation: &hypothesis::annotations::Annotation) -> Vec<&str> {
    annotation
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
                Some(quotes)
            }
        })
        .flat_map(|v| v.into_iter())
        .collect::<Vec<_>>()
}

pub fn clean_uri(uri: &str) -> String {
    match Url::parse(uri) {
        Ok(parsed_uri) => {
            if parsed_uri.scheme() == "urn" {
                uri.to_owned()
            } else {
                parsed_uri[url::Position::AfterScheme..]
                    .trim_start_matches("://")
                    .trim_end_matches('/')
                    .to_owned()
            }
        }
        Err(_) => uri.to_owned(),
    }
}

/// Converts a URI into something that can be used as a folder/filename
pub fn uri_to_filename(uri: &str) -> String {
    clean_uri(uri)
        .replace("://", "_")
        .replace(".", "_")
        .replace("/", "_")
        .replace(":", "_")
}
