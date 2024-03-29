use hypothesis::errors::HypothesisError;
use thiserror::Error;

/// "It claimed to have 15 functions, although it appeared that at least ten were apologizing for
/// the useless manner in which it performed the others." - [Dis-organizer](https://wiki.lspace.org/mediawiki/Dis-organiser)
#[derive(Debug, Error)]
pub enum Apologize {
    /// Thrown when trying to access an unrecorded tag
    #[error("You haven't tagged anything as {tag:?} yet.")]
    TagNotFound { tag: String },
    /// Thrown when trying annotation ID doesn't match any recorded annotations
    #[error("Couldn't find an annotation with ID {id:?}")]
    AnnotationNotFound { id: String },
    /// Thrown when trying to access an unrecorded group
    #[error("Couldn't access group {id:?}: {error:?}. The Group ID can be found in the URL of the group: https://hypothes.is/groups/<group_id>/<group_name>")]
    GroupNotFound { id: String, error: HypothesisError },
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
    /// Errors related to making the knowledge base
    #[error("KBError: {message:?}")]
    KBError { message: String },
    /// Thrown when no text is returned from an external editor
    #[error("EditorError")]
    EditorError,
    /// Catch-all for stuff that should never happen
    #[error("OutOfCheeseError: {message:?}\nRedo from start.")]
    OutOfCheeseError { message: String },
}
