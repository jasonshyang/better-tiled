//! What the user should be told: a status bar flash, or an error with its details.

/// How serious a notice is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    /// For information.
    Info,
    /// Something was done, but not all of it.
    Warning,
    /// Something was not done.
    Error,
}

/// A message for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// How serious it is.
    pub severity: Severity,
    /// The sentence shown.
    pub text: String,
    /// More, for the error dialog.
    pub detail: Option<String>,
}

impl Notice {
    /// For information.
    pub fn info(text: impl Into<String>) -> Notice {
        Notice {
            severity: Severity::Info,
            text: text.into(),
            detail: None,
        }
    }

    /// Something done in part.
    pub fn warning(text: impl Into<String>) -> Notice {
        Notice {
            severity: Severity::Warning,
            text: text.into(),
            detail: None,
        }
    }

    /// Something not done.
    pub fn error(text: impl Into<String>) -> Notice {
        Notice {
            severity: Severity::Error,
            text: text.into(),
            detail: None,
        }
    }

    /// The notice with more detail.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Notice {
        self.detail = Some(detail.into());
        self
    }
}
