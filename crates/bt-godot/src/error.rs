//! Errors of a Godot export.

use crate::Refusal;

/// Why an export wrote nothing.
#[derive(Debug)]
pub enum Error {
    /// The map can't be exported there (exit 1).
    Failed(String),
    /// Files the export would replace without being able to show they are its own (exit 3).
    Refused(Vec<Refusal>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Failed(message) => f.write_str(message),
            Error::Refused(refusals) => {
                let lines: Vec<String> = refusals
                    .iter()
                    .map(|r| format!("{}: {}", r.path.display(), r.reason))
                    .collect();
                f.write_str(&lines.join("\n"))
            }
        }
    }
}

impl From<String> for Error {
    fn from(message: String) -> Error {
        Error::Failed(message)
    }
}

impl From<Error> for String {
    fn from(error: Error) -> String {
        error.to_string()
    }
}

impl std::error::Error for Error {}
