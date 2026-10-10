//! The crate's error type.

use std::path::PathBuf;
use std::time::Duration;

use bt_utils::text::plural;

/// Why the live link could not do what was asked. Every message is a sentence `bt` can show as
/// it is, and the editor's [`Handler`](crate::Handler) can answer with: an error converts into
/// its sentence, so `?` works on a handler's `Result<Response, String>`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The live folder is not used: it failed the privacy check (a symbolic link, not a folder,
    /// another user's) or could not be made or listed. The message names the folder and why.
    #[error(transparent)]
    Folder {
        /// What `bt-utils` said.
        #[from]
        source: bt_utils::Error,
    },
    /// `BT_LIVE_DIR` is not an absolute path, so `bt` and the editor could each take it for a
    /// folder of their own working folder.
    #[error(
        "BT_LIVE_DIR is not used: \"{}\" is not an absolute path, so bt and the editor could each take it for another folder",
        value.display()
    )]
    LiveDirNotAbsolute {
        /// The variable's value.
        value: PathBuf,
    },
    /// `BT_LIVE_DIR` is too long for a socket path in it to fit.
    #[error(
        "BT_LIVE_DIR is not used: {} is too long for a socket in it (a socket's path has room for {max} bytes)",
        value.display()
    )]
    LiveDirTooLong {
        /// The variable's value.
        value: PathBuf,
        /// The longest socket path there is room for.
        max: usize,
    },
    /// This system has no Unix domain sockets, so no live link (Windows).
    #[error(
        "the live link needs Unix domain sockets, which this system lacks: bt works with the files here"
    )]
    Unsupported,
    /// The editor's socket could not be made.
    #[error("couldn't listen on {}: {source}", socket.display())]
    Listen {
        /// The socket.
        socket: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// An editor could not be reached, or the connection to it failed partway.
    #[error("couldn't talk to the editor at {}: {source}", socket.display())]
    Connection {
        /// The editor's socket.
        socket: PathBuf,
        /// What the system said.
        #[source]
        source: std::io::Error,
    },
    /// The editor did not answer within the wait.
    #[error("the editor didn't answer within {}: is a dialog open in it?", wait_text(*wait))]
    NoAnswer {
        /// How long the client waited.
        wait: Duration,
    },
    /// A request is longer than an editor reads; it was not sent.
    #[error("the request is {size} bytes, more than the {limit} an editor reads")]
    TooLarge {
        /// The request's length in bytes, its newline included.
        size: usize,
        /// The most an editor reads.
        limit: usize,
    },
    /// What the editor sent back is not an answer of this protocol.
    #[error("the editor's answer can't be read: {reason}")]
    Garbled {
        /// What is wrong with it.
        reason: String,
    },
    /// The editor answered with something other than what the request asks for.
    #[error("the editor answered with {found} where {expected} was expected")]
    Unexpected {
        /// What the request asks for.
        expected: &'static str,
        /// What came.
        found: &'static str,
    },
    /// The editor could not carry the request out, and said why.
    #[error("{message}")]
    Failed {
        /// The editor's sentence.
        message: String,
    },
    /// The window did not take a job before its deadline: `bt` has stopped waiting, so it was
    /// dropped and will not be done.
    #[error("the editor's window didn't take the request in time: is a dialog open in it?")]
    NotTaken,
    /// The window took a job but did not answer it in time.
    #[error("the editor's window took the request but didn't finish it in time")]
    NotFinished,
    /// A wait too long to measure from now.
    #[error("a wait of {wait:?} can't be measured")]
    WaitTooLong {
        /// The wait asked for.
        wait: Duration,
    },
}

/// The sentence of an error, for a [`Handler`](crate::Handler) whose answer is an error line.
impl From<Error> for String {
    fn from(error: Error) -> String {
        error.to_string()
    }
}

/// A wait as people say it: `"15 seconds"`, `"10 minutes"`, `"250 milliseconds"`.
pub(crate) fn wait_text(wait: Duration) -> String {
    let seconds = usize::try_from(wait.as_secs()).unwrap_or(usize::MAX);
    if wait.subsec_nanos() != 0 || seconds == 0 {
        let millis = usize::try_from(wait.as_millis()).unwrap_or(usize::MAX);
        plural(millis, "millisecond", "milliseconds")
    } else if seconds % 60 == 0 {
        plural(seconds / 60, "minute", "minutes")
    } else {
        plural(seconds, "second", "seconds")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_read_as_people_say_them() {
        assert_eq!(wait_text(Duration::from_secs(15)), "15 seconds");
        assert_eq!(wait_text(Duration::from_secs(1)), "1 second");
        assert_eq!(wait_text(Duration::from_secs(600)), "10 minutes");
        assert_eq!(wait_text(Duration::from_secs(60)), "1 minute");
        assert_eq!(wait_text(Duration::from_millis(250)), "250 milliseconds");
        assert_eq!(wait_text(Duration::from_millis(1500)), "1500 milliseconds");
    }

    #[test]
    fn errors_are_sentences_a_handler_can_answer_with() {
        let error = Error::NoAnswer {
            wait: Duration::from_secs(8),
        };
        assert_eq!(
            String::from(error),
            "the editor didn't answer within 8 seconds: is a dialog open in it?"
        );
        let failed = Error::Failed {
            message: "a.tmx isn't open in the editor".to_owned(),
        };
        assert_eq!(failed.to_string(), "a.tmx isn't open in the editor");
    }
}
