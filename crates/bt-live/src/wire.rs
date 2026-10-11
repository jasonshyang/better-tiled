//! One line each way (`11-live-link.md`, §6): a connection carries one request, a line of JSON,
//! and one answer, a line of JSON: `{"ok": …}` or `{"error": "…"}`. There is no session and no
//! second message, so neither side keeps state between calls, and a crashed peer costs the other
//! at most one timeout. [`ask`] is the client's half, [`serve`] the server's.
//!
//! Sizes are checked before anything is parsed: a server reads at most [`Limits::request_bytes`]
//! of a request, a client at most [`MAX_ANSWER`] of an answer.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, wait_text};
use crate::jobs::deadline_after;
use crate::protocol::{Request, Response};

/// The longest answer a client reads, its newline included: an answer may hold a whole map as
/// text, so far more than a request.
pub(crate) const MAX_ANSWER: usize = 256 << 20;

/// How much is read from a socket at a time.
const READ_BUFFER: usize = 64 << 10;

/// What a server allows its connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limits {
    /// The longest request read, its newline included.
    pub(crate) request_bytes: usize,
    /// How long a connection has to send its request.
    pub(crate) request_time: Duration,
    /// How many connections are served at once; more are answered that the editor is busy.
    pub(crate) connections: usize,
}

impl Limits {
    /// The limits of `11-live-link.md`: 16 MiB and ten seconds for a request, sixteen
    /// connections at once.
    pub(crate) const STANDARD: Limits = Limits {
        request_bytes: 16 << 20,
        request_time: Duration::from_secs(10),
        connections: 16,
    };
}

/// An answer as it travels: `{"ok": …}` or `{"error": "…"}`.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Envelope<T> {
    Ok(T),
    Error(String),
}

/// Why a line could not be read.
#[derive(Debug)]
enum LineError {
    /// No newline within the limit.
    TooLarge,
    /// The deadline passed first.
    TimedOut,
    /// The peer closed the connection before sending anything.
    Closed,
    /// The connection failed.
    Failed {
        /// What the system said.
        source: std::io::Error,
    },
}

/// Sends `request` to the editor listening on `socket` on a connection of its own and waits up
/// to `wait` for the answer: the answer's value (what `{"ok": …}` holds), or the editor's
/// sentence as [`Error::Failed`] when it answered `{"error": …}`.
pub(crate) fn ask(
    socket: &Path,
    request: &Request,
    wait: Duration,
) -> Result<serde_json::Value, Error> {
    let line = request_line(request)?;
    let deadline = deadline_after(wait)?;
    let connection = |source: std::io::Error| Error::Connection {
        socket: socket.to_path_buf(),
        source,
    };
    let stream = UnixStream::connect(socket).map_err(connection)?;
    unless_closed(stream.set_write_timeout(Some(wait))).map_err(connection)?;
    if let Err(source) = (&stream).write_all(&line) {
        match source.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                return Err(Error::NoAnswer { wait });
            }
            // An editor that answers before reading (it is busy, say) and closes: its answer
            // is waiting to be read.
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset => {}
            _ => return Err(connection(source)),
        }
    }
    let answer = read_line(&stream, deadline, MAX_ANSWER).map_err(|error| match error {
        LineError::TooLarge => Error::Garbled {
            reason: format!("it is longer than the {MAX_ANSWER} bytes an answer may have"),
        },
        LineError::TimedOut => Error::NoAnswer { wait },
        LineError::Closed => Error::Garbled {
            reason: "the editor closed the connection without answering".to_owned(),
        },
        LineError::Failed { source } => connection(source),
    })?;
    match serde_json::from_slice::<Envelope<serde_json::Value>>(&answer) {
        Ok(Envelope::Ok(value)) => Ok(value),
        Ok(Envelope::Error(message)) => Err(Error::Failed { message }),
        Err(error) => Err(Error::Garbled {
            reason: error.to_string(),
        }),
    }
}

/// Serves one connection: reads its request within `limits`, hands it to `handle` and writes
/// the answer. A line that is not a request, is too long or comes too late is answered with an
/// error; a connection closed before sending anything gets no answer.
pub(crate) fn serve(
    stream: &UnixStream,
    limits: &Limits,
    handle: impl FnOnce(&Request) -> Result<Response, String>,
) {
    let Ok(deadline) = deadline_after(limits.request_time) else {
        return;
    };
    let answer = match read_line(stream, deadline, limits.request_bytes) {
        Ok(line) => match serde_json::from_slice::<Request>(&line) {
            Ok(request) => handle(&request),
            Err(error) => Err(format!("not a request: {error}")),
        },
        Err(LineError::TooLarge) => Err(format!(
            "the request is longer than the {} bytes an editor reads",
            limits.request_bytes
        )),
        Err(LineError::TimedOut) => Err(format!(
            "no request came within {}",
            wait_text(limits.request_time)
        )),
        Err(LineError::Closed | LineError::Failed { .. }) => return,
    };
    // The client may have gone; there is no one to tell if the answer can't be written.
    let _ = stream.set_write_timeout(Some(limits.request_time));
    let _ = (&*stream).write_all(&answer_line(answer));
}

/// The request as the line a client sends, or [`Error::TooLarge`] when no editor would read it.
fn request_line(request: &Request) -> Result<Vec<u8>, Error> {
    let mut line = serde_json::to_vec(request).map_err(|error| Error::Garbled {
        reason: format!("the request can't be written: {error}"),
    })?;
    line.push(b'\n');
    let limit = Limits::STANDARD.request_bytes;
    if line.len() > limit {
        return Err(Error::TooLarge {
            size: line.len(),
            limit,
        });
    }
    Ok(line)
}

/// The line a server answers with.
fn answer_line(answer: Result<Response, String>) -> Vec<u8> {
    let written = match &answer {
        Ok(response) => serde_json::to_vec(&Envelope::Ok(response)),
        Err(message) => serde_json::to_vec(&Envelope::<()>::Error(message.clone())),
    };
    let mut line = match written {
        Ok(line) => line,
        // A path that is not UTF-8 has no JSON; the answer says so rather than nothing.
        Err(error) => serde_json::to_vec(&Envelope::<()>::Error(format!(
            "the editor's answer can't be written as JSON: {error}"
        )))
        .unwrap_or_else(|_| {
            br#"{"error":"the editor's answer can't be written as JSON"}"#.to_vec()
        }),
    };
    line.push(b'\n');
    line
}

/// Reads one line from `stream`, its newline included, giving up at `deadline` or past `limit`
/// bytes. A last line without a newline counts when the peer closes after it.
fn read_line(stream: &UnixStream, deadline: Instant, limit: usize) -> Result<Vec<u8>, LineError> {
    let limit = u64::try_from(limit).unwrap_or(u64::MAX);
    let mut reader =
        BufReader::with_capacity(READ_BUFFER, DeadlineReader { stream, deadline }).take(limit);
    let mut line = Vec::new();
    match reader.read_until(b'\n', &mut line) {
        Ok(_) => {}
        Err(error) => {
            return Err(match error.kind() {
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                    LineError::TimedOut
                }
                _ => LineError::Failed { source: error },
            });
        }
    }
    if line.last() == Some(&b'\n') {
        Ok(line)
    } else if u64::try_from(line.len()).is_ok_and(|read| read >= limit) {
        Err(LineError::TooLarge)
    } else if line.is_empty() {
        Err(LineError::Closed)
    } else {
        Ok(line)
    }
}

/// A stream whose every read gives up at one deadline, however many reads a line takes.
struct DeadlineReader<'s> {
    stream: &'s UnixStream,
    deadline: Instant,
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        unless_closed(self.stream.set_read_timeout(Some(left)))?;
        let mut stream = self.stream;
        stream.read(buf)
    }
}

/// The result of setting a socket's timeout, with the refusal macOS gives once the peer has
/// closed taken as success: reading and writing no longer wait then (what was sent is read, then
/// the end; a write fails at once), so the timeout does not matter.
fn unless_closed(set: std::io::Result<()>) -> std::io::Result<()> {
    match set {
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_travel_as_ok_or_error() {
        let done = Response::Done(crate::Done {
            message: "saved".to_owned(),
            nothing: false,
        });
        assert_eq!(
            String::from_utf8(answer_line(Ok(done))).unwrap(),
            "{\"ok\":{\"answer\":\"done\",\"message\":\"saved\",\"nothing\":false}}\n"
        );
        assert_eq!(
            String::from_utf8(answer_line(
                Err("a.tmx isn't open in the editor".to_owned())
            ))
            .unwrap(),
            "{\"error\":\"a.tmx isn't open in the editor\"}\n"
        );
    }

    #[test]
    fn an_answer_that_cannot_be_json_says_so() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let text = Response::MapText(crate::MapText {
            dirty: false,
            map: String::new(),
            tilesets: vec![crate::TilesetText {
                path: OsStr::from_bytes(b"/maps/\xff.tsx").into(),
                tsx: String::new(),
            }],
        });
        let line = String::from_utf8(answer_line(Ok(text))).unwrap();
        assert!(
            line.starts_with("{\"error\":\"the editor's answer can't be written as JSON: "),
            "{line}"
        );
    }

    #[test]
    fn a_request_longer_than_an_editor_reads_is_not_sent() {
        let limit = Limits::STANDARD.request_bytes;
        let edit = |script_bytes: usize| {
            Request::Edit(crate::EditRequest::new("/a.tmx", "x".repeat(script_bytes)))
        };
        let overhead = request_line(&edit(0)).unwrap().len();
        let at_limit = request_line(&edit(limit - overhead)).unwrap();
        assert_eq!(at_limit.len(), limit, "a line of exactly the limit is sent");
        match request_line(&edit(limit - overhead + 1)) {
            Err(Error::TooLarge { size, limit: max }) => {
                assert_eq!(size, limit + 1);
                assert_eq!(max, limit);
            }
            other => panic!("sent: {:?}", other.map(|line| line.len())),
        }
    }

    #[test]
    fn lines_are_read_to_their_newline_within_the_limit_and_the_deadline() {
        let (client, server) = UnixStream::pair().unwrap();
        let soon = || Instant::now() + Duration::from_secs(5);
        (&client).write_all(b"{\"op\":\"status\"}\nextra").unwrap();
        assert_eq!(
            read_line(&server, soon(), 64).unwrap(),
            b"{\"op\":\"status\"}\n"
        );

        let (client, server) = UnixStream::pair().unwrap();
        (&client).write_all(&[b' '; 64]).unwrap();
        assert!(matches!(
            read_line(&server, soon(), 64),
            Err(LineError::TooLarge)
        ));

        let (client, server) = UnixStream::pair().unwrap();
        (&client).write_all(b"{\"op\":").unwrap();
        let started = Instant::now();
        assert!(matches!(
            read_line(&server, Instant::now() + Duration::from_millis(100), 64),
            Err(LineError::TimedOut)
        ));
        assert!(started.elapsed() < Duration::from_secs(2));

        let (client, server) = UnixStream::pair().unwrap();
        (&client).write_all(b"{\"op\":\"status\"}").unwrap();
        drop(client);
        assert_eq!(
            read_line(&server, soon(), 64).unwrap(),
            b"{\"op\":\"status\"}",
            "a last line without its newline"
        );

        let (client, server) = UnixStream::pair().unwrap();
        drop(client);
        assert!(matches!(
            read_line(&server, soon(), 64),
            Err(LineError::Closed)
        ));
    }

    #[test]
    fn a_long_answer_is_read_whole_after_the_editor_has_closed() {
        // macOS refuses to set a timeout on a socket whose peer has closed; what is still
        // waiting in it is read all the same, many reads long.
        let (editor, client) = UnixStream::pair().unwrap();
        let mut answer = vec![b'x'; 4 * READ_BUFFER + 7];
        answer.push(b'\n');
        let writer = {
            let answer = answer.clone();
            std::thread::spawn(move || {
                (&editor).write_all(&answer).unwrap();
                drop(editor);
            })
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(read_line(&client, deadline, MAX_ANSWER).unwrap(), answer);
        writer.join().unwrap();

        let (editor, client) = UnixStream::pair().unwrap();
        (&editor).write_all(b"{\"ok\":{}}\n").unwrap();
        drop(editor);
        assert_eq!(
            read_line(&client, deadline, MAX_ANSWER).unwrap(),
            b"{\"ok\":{}}\n",
            "the whole answer came before the close"
        );
    }
}
