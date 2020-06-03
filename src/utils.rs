//! Utility functions
use chrono::{DateTime, Utc};
use chrono_english::{parse_date_string, Dialect};
use dialoguer::{theme, Input};
use url::Url;

/// ASCII code of semicolon
/// TODO: Tag cannot have semicolon in it, remember to add this to the README
pub const SEMICOLON: u8 = 59;

/// Makes `DateTime` from a string, can be colloquial like "last Friday 8pm"
pub fn parse_datetime(datetime_string: &str) -> color_eyre::Result<DateTime<Utc>> {
    if datetime_string.to_ascii_lowercase() == "today" {
        Ok(Utc::now().date().and_hms(0, 0, 0))
    } else {
        Ok(parse_date_string(datetime_string, Utc::now(), Dialect::Uk)?)
    }
}

/// Splits byte array by semicolon into list of Annotation IDs
pub fn split_ids(index_list: &[u8]) -> color_eyre::Result<Vec<String>> {
    let index_list_string = std::str::from_utf8(index_list)?;
    Ok(index_list_string
        .split(std::str::from_utf8(&[SEMICOLON])?)
        .map(|x| x.to_string())
        .collect())
}

/// List of String into semicolon-joined byte array
pub fn join_ids(index_list: &[String]) -> color_eyre::Result<Vec<u8>> {
    Ok(index_list
        .join(std::str::from_utf8(&[SEMICOLON])?)
        .as_bytes()
        .to_vec())
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

pub(crate) fn base_url(mut url: Url) -> Option<Url> {
    match url.path_segments_mut() {
        Ok(mut path) => {
            path.clear();
        }
        Err(_) => return None,
    }
    url.set_query(None);
    Some(url)
}
