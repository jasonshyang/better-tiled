//! How a run of `bt` ended, as its exit code says (`10-agents.md`, §4.2): scripts tell
//! "problems found" and "refused" from "could not run".

use std::process::ExitCode;

/// How a command ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// It did what was asked: 0.
    Done,
    /// It could not: 1. Nothing was written.
    Failed,
    /// It was asked wrongly: 2.
    Usage,
    /// It would have lost or overwritten something, so it did nothing: 3.
    Refused,
    /// What it checked is out of date: 4.
    OutOfDate,
    /// `bt check` found problems: 5.
    Problems,
}

impl Status {
    /// The exit code.
    pub(crate) fn code(self) -> ExitCode {
        ExitCode::from(match self {
            Status::Done => 0,
            Status::Failed => 1,
            Status::Usage => 2,
            Status::Refused => 3,
            Status::OutOfDate => 4,
            Status::Problems => 5,
        })
    }
}

/// What a command prints, and how it ended.
#[derive(Clone, Debug)]
pub(crate) struct Printed {
    /// The text (or JSON) for standard output.
    pub(crate) text: String,
    /// How it ended.
    pub(crate) status: Status,
    /// Why it stopped, for standard error after `bt:`: a command that did part of what was
    /// asked (`bt edit` of several maps) prints what it did and why it stopped.
    pub(crate) failure: Option<String>,
}

impl Printed {
    /// Text of a command that did what was asked.
    pub(crate) fn done(text: impl Into<String>) -> Printed {
        Printed {
            text: text.into(),
            status: Status::Done,
            failure: None,
        }
    }

    /// `text` of what a command did before it stopped, ending as `status` says, with why.
    pub(crate) fn stopped(text: impl Into<String>, status: Status, failure: String) -> Printed {
        Printed {
            text: text.into(),
            status,
            failure: Some(failure),
        }
    }
}
