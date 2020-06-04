use thiserror::Error;

/// Errors which can be caused by normal Gooseberry operation.
/// Those caused by external libraries throw their own errors when possible
#[derive(Debug, Error)]
pub enum Apologize {
    /// Thrown when trying to access an unrecorded tag
    #[error("You haven't tagged anything as {tag:?} yet.")]
    TagNotFound { tag: String },
    /// Thrown when trying annotation ID doesn't match any recorded annotations
    #[error("Couldn't find an annotation with ID {id:?}")]
    AnnotationNotFound { id: String },
    /// Thrown when trying to access an unrecorded tag
    #[error("Couldn't find group {id:?}. The Group ID can be found in the URL of the group: https://hypothes.is/groups/<group_id>/<group_name>")]
    GroupNotFound { id: String },
    /// Thrown when explicit Y not received from user for destructive things
    #[error("I'm a coward. Doing nothing.")]
    DoingNothing,
    /// Thrown when $HOME is not set
    #[error("Homeless: $HOME not set")]
    Homeless,
    /// Thrown when `skim` doesn't work
    #[error("SearchError: Search failed")]
    SearchError,
    /// Errors related to changing the configuration file
    #[error("ConfigError: {message:?}")]
    ConfigError { message: String },
    /// Errors related to making the mdBook wiki
    #[error("mdBookError: {message:?}")]
    MdBookError { message: String },
    /// Catch-all for stuff that should never happen
    #[error("OutOfCheeseError: {message:?}\nRedo from start.")]
    OutOfCheeseError { message: String },
}
