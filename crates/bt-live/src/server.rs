//! The editor's side (`11-live-link.md`, §8): a socket in the folder named after the editor's
//! process, a thread that accepts connections, and a thread per connection, a bounded number at
//! once, that reads the request and hands it to the [`Handler`] the application gave.
//!
//! The handler answers on the connection's thread. What needs the editor's state becomes a job
//! for the window's thread ([`Jobs`](crate::Jobs)); a script runs on the connection's thread,
//! so the window never waits for one. Dropping the [`Server`] stops listening and removes the
//! socket. macOS cleans old files out of `/tmp`, so the window asks [`Server::lost`] every
//! [`SOCKET_CHECK_INTERVAL`] and starts a new server when the socket has gone.

use std::path::Path;
use std::time::Duration;

use crate::error::Error;
use crate::protocol::{Request, Response};

/// How often the window asks [`Server::lost`] whether its socket is still there.
pub const SOCKET_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Answers requests, on the connection's thread: the application's half of the server. An
/// error is a sentence for the person, sent back as `{"error": …}`. A closure taking a
/// `&Request` and returning `Result<Response, String>` is a handler.
pub trait Handler: Send + Sync + 'static {
    /// The answer to `request`, or why there is none.
    fn handle(&self, request: &Request) -> Result<Response, String>;
}

impl<F> Handler for F
where
    F: Fn(&Request) -> Result<Response, String> + Send + Sync + 'static,
{
    fn handle(&self, request: &Request) -> Result<Response, String> {
        self(request)
    }
}

/// A running editor's listening socket. Dropping it stops listening and removes the socket.
#[derive(Debug)]
pub struct Server {
    #[cfg(unix)]
    listening: listen::Listening,
    #[cfg(not(unix))]
    _unsupported: (),
}

impl Server {
    /// Listens in `folder` (from [`live_dir`](crate::live_dir)) on `<pid>.sock`, replacing a
    /// leftover of the same name, and answers each request with `handler`. The folder is
    /// checked first, as `bt` checks it: made `0700` when missing (macOS may have cleaned it
    /// away), refused when it is not the user's alone. On Windows there is no live link, and
    /// this is [`Error::Unsupported`].
    pub fn start(folder: &Path, handler: impl Handler) -> Result<Server, Error> {
        #[cfg(unix)]
        {
            bt_utils::paths::make_private_dir(folder)?;
            let socket = crate::dir::socket_path(folder, std::process::id());
            Server::bind(socket, handler, crate::wire::Limits::STANDARD)
        }
        #[cfg(not(unix))]
        {
            let _ = folder;
            let _ = handler;
            Err(Error::Unsupported)
        }
    }

    /// Whether the server can no longer be reached: its socket was removed or replaced (macOS
    /// cleans old files out of `/tmp`), or its listening thread stopped. Then drop it and start
    /// another. A look at the file system, made every [`SOCKET_CHECK_INTERVAL`].
    pub fn lost(&self) -> bool {
        #[cfg(unix)]
        {
            self.listening.lost()
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Listens on `socket` with `limits`.
    #[cfg(unix)]
    pub(crate) fn bind(
        socket: std::path::PathBuf,
        handler: impl Handler,
        limits: crate::wire::Limits,
    ) -> Result<Server, Error> {
        Ok(Server {
            listening: listen::Listening::start(socket, Box::new(handler), limits)?,
        })
    }
}

/// The listening socket and its threads.
#[cfg(unix)]
mod listen {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use rustix::event::{PollFd, PollFlags};

    use super::Handler;
    use crate::error::Error;
    use crate::protocol::{Request, Response};
    use crate::wire::{self, Limits};

    /// How long the listening thread pauses after `accept` fails for a reason other than there
    /// being no connection (out of file descriptors, say), so it does not spin.
    const ACCEPT_PAUSE: Duration = Duration::from_millis(100);

    /// How long a refusal to a connection beyond the bound may take to write.
    const REFUSAL_TIME: Duration = Duration::from_secs(1);

    /// A socket being listened on.
    #[derive(Debug)]
    pub(super) struct Listening {
        /// The socket's path.
        socket: PathBuf,
        /// The socket file as it was made, to tell it from another put in its place.
        identity: FileIdentity,
        /// Dropped to stop the listening thread: its other end then reads as closed.
        stop: Option<UnixStream>,
        /// The listening thread.
        thread: Option<JoinHandle<()>>,
    }

    /// Which file a path held: its device and inode, and when it last changed, which tells a
    /// file from another that reused its inode.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct FileIdentity {
        device: u64,
        inode: u64,
        changed_seconds: i64,
        changed_nanoseconds: i64,
    }

    /// What the connection threads share.
    struct Shared {
        handler: Box<dyn Handler>,
        limits: Limits,
        /// How many connections are being served.
        active: AtomicUsize,
    }

    /// A connection being served: counted while it lives.
    struct Active {
        shared: Arc<Shared>,
    }

    impl Listening {
        /// Binds `socket`, replacing a leftover of the same name, and starts the listening
        /// thread.
        pub(super) fn start(
            socket: PathBuf,
            handler: Box<dyn Handler>,
            limits: Limits,
        ) -> Result<Listening, Error> {
            let listen_error = |source: std::io::Error| Error::Listen {
                socket: socket.clone(),
                source,
            };
            match std::fs::remove_file(&socket) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(listen_error(error)),
            }
            let listener = UnixListener::bind(&socket).map_err(listen_error)?;
            Listening::start_thread(listener, &socket, handler, limits).map_err(|source| {
                let _ = std::fs::remove_file(&socket);
                listen_error(source)
            })
        }

        /// Starts the thread listening on `listener`, bound at `socket`.
        fn start_thread(
            listener: UnixListener,
            socket: &Path,
            handler: Box<dyn Handler>,
            limits: Limits,
        ) -> std::io::Result<Listening> {
            let identity = FileIdentity::of(&std::fs::symlink_metadata(socket)?);
            // Accepting never blocks, so the thread only waits in `poll`, where it can be
            // stopped.
            listener.set_nonblocking(true)?;
            let (stop, stopped) = UnixStream::pair()?;
            let shared = Arc::new(Shared {
                handler,
                limits,
                active: AtomicUsize::new(0),
            });
            let thread = std::thread::Builder::new()
                .name("live link".to_owned())
                .spawn(move || listen(&listener, &stopped, &shared))?;
            Ok(Listening {
                socket: socket.to_path_buf(),
                identity,
                stop: Some(stop),
                thread: Some(thread),
            })
        }

        /// Whether the socket is gone or replaced, or the listening thread has stopped.
        pub(super) fn lost(&self) -> bool {
            !self.socket_is_ours() || self.thread.as_ref().is_none_or(JoinHandle::is_finished)
        }

        /// Whether the socket's path still holds the socket this made.
        fn socket_is_ours(&self) -> bool {
            std::fs::symlink_metadata(&self.socket)
                .is_ok_and(|metadata| FileIdentity::of(&metadata) == self.identity)
        }
    }

    impl Drop for Listening {
        fn drop(&mut self) {
            // A socket another server put in its place (a server started before this one was
            // dropped) is left alone.
            if self.socket_is_ours() {
                let _ = std::fs::remove_file(&self.socket);
            }
            drop(self.stop.take());
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    impl FileIdentity {
        fn of(metadata: &std::fs::Metadata) -> FileIdentity {
            FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                changed_seconds: metadata.ctime(),
                changed_nanoseconds: metadata.ctime_nsec(),
            }
        }
    }

    impl Drop for Active {
        fn drop(&mut self) {
            self.shared.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// The listening thread: waits for connections, or for `stopped` to close, and serves each
    /// connection on a thread of its own.
    fn listen(listener: &UnixListener, stopped: &UnixStream, shared: &Arc<Shared>) {
        while wait_for_connection(listener, stopped) {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => dispatch(stream, shared),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        std::thread::sleep(ACCEPT_PAUSE);
                        break;
                    }
                }
            }
        }
    }

    /// Waits until a connection may be waiting (true), or the server is stopped or the socket
    /// can no longer be waited on (false).
    fn wait_for_connection(listener: &UnixListener, stopped: &UnixStream) -> bool {
        loop {
            let mut fds = [
                PollFd::new(listener, PollFlags::IN),
                PollFd::new(stopped, PollFlags::IN),
            ];
            match rustix::event::poll(&mut fds, None) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(_) => return false,
            }
            let [listening, stopping] = &fds;
            if !stopping.revents().is_empty() {
                return false;
            }
            let events = listening.revents();
            if events.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                return false;
            }
            if events.contains(PollFlags::IN) {
                return true;
            }
        }
    }

    /// Serves `stream` on a thread of its own, or answers that the editor is busy when the
    /// bound is reached.
    fn dispatch(stream: UnixStream, shared: &Arc<Shared>) {
        // An accepted socket takes the listener's non-blocking mode on macOS and the BSDs.
        if stream.set_nonblocking(false).is_err() {
            return;
        }
        if shared.active.load(Ordering::SeqCst) >= shared.limits.connections {
            refuse(
                &stream,
                "the editor is answering too many requests at once: try again in a moment",
            );
            return;
        }
        shared.active.fetch_add(1, Ordering::SeqCst);
        let active = Active {
            shared: Arc::clone(shared),
        };
        // Without a thread the connection is dropped, and the client sees it closed.
        let _ = std::thread::Builder::new()
            .name("live link connection".to_owned())
            .spawn(move || {
                let shared = &active.shared;
                wire::serve(&stream, &shared.limits, |request| {
                    handle(shared.handler.as_ref(), request)
                });
            });
    }

    /// The handler's answer, or an error answer when it panicked: a connection's failure never
    /// takes the editor down.
    fn handle(handler: &dyn Handler, request: &Request) -> Result<Response, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler.handle(request)))
            .unwrap_or_else(|panic| {
                let what = panic
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "no message".to_owned());
                Err(format!("the editor failed while answering ({what})"))
            })
    }

    /// Answers a connection that is not served with an error line, without reading it.
    fn refuse(stream: &UnixStream, message: &str) {
        let line = format!("{}\n", serde_json::json!({ "error": message }));
        let _ = stream.set_write_timeout(Some(REFUSAL_TIME));
        let _ = std::io::Write::write_all(&mut &*stream, line.as_bytes());
    }
}

#[cfg(test)]
#[cfg(not(unix))]
mod unsupported_tests {
    use super::*;

    #[test]
    fn there_is_no_live_link_without_unix_sockets() {
        let handler = |_: &Request| -> Result<Response, String> { Err("unused".to_owned()) };
        let error = Server::start(Path::new("C:\\live"), handler).unwrap_err();
        assert!(matches!(error, Error::Unsupported), "{error:?}");
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;
    use std::time::Instant;

    use bt_utils::NotPrivate;

    use super::*;
    use crate::client::{Problem, editors};
    use crate::dir::socket_path;
    use crate::jobs::Jobs;
    use crate::testing::{TestFolder, dead_socket, every_answer, every_request};
    use crate::wire::{self, Limits};

    /// The answer the tests' handlers give to each kind of request.
    fn answer_to(request: &Request) -> Response {
        let [status, map_text, edited, done] = <[Response; 4]>::try_from(every_answer()).unwrap();
        match request {
            Request::Status => status,
            Request::Snapshot { .. } => map_text,
            Request::Edit(_) => edited,
            _ => done,
        }
    }

    /// A handler answering every request with [`answer_to`].
    fn answering(request: &Request) -> Result<Response, String> {
        Ok(answer_to(request))
    }

    /// The socket a server of this process listens on in `folder`.
    fn own_socket(folder: &TestFolder) -> PathBuf {
        socket_path(folder.path(), std::process::id())
    }

    /// Sends `line` on a connection of its own, as it is, and reads the answer's line.
    fn raw_ask(socket: &Path, line: &[u8]) -> String {
        let stream = UnixStream::connect(socket).unwrap();
        (&stream).write_all(line).unwrap();
        let mut answer = String::new();
        BufReader::new(&stream).read_line(&mut answer).unwrap();
        answer
    }

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    #[test]
    fn every_request_reaches_the_handler_and_its_answer_comes_back() {
        let folder = TestFolder::new();
        let received = Arc::new(Mutex::new(Vec::new()));
        let server = Server::start(folder.path(), {
            let received = Arc::clone(&received);
            move |request: &Request| {
                received.lock().unwrap().push(request.clone());
                answering(request)
            }
        })
        .unwrap();
        let found = editors(folder.path()).unwrap();
        assert!(found.problems.is_empty(), "{:?}", found.problems);
        let [editor] = found.answered.as_slice() else {
            panic!("{:?}", found.answered);
        };
        assert_eq!(editor.socket(), own_socket(&folder));
        assert_eq!(
            editor.status(),
            &answer_to(&Request::Status).into_status().unwrap()
        );
        for request in every_request() {
            assert_eq!(
                editor.ask(&request).unwrap(),
                answer_to(&request),
                "{request:?}"
            );
        }
        let mut expected = vec![Request::Status];
        expected.extend(every_request());
        assert_eq!(
            *received.lock().unwrap(),
            expected,
            "each request as it was sent"
        );
        drop(server);
    }

    #[test]
    fn a_line_that_is_not_a_request_is_answered_with_an_error_and_the_server_goes_on() {
        let folder = TestFolder::new();
        let _server = Server::start(folder.path(), answering).unwrap();
        let socket = own_socket(&folder);
        for line in [
            "hello\n",
            "{\"op\":\"remember_export\",\"path\":\"/a.tmx\"}\n",
            "{\"op\":\"snapshot\"}\n",
            "[1, 2]\n",
            "\n",
        ] {
            let answer = raw_ask(&socket, line.as_bytes());
            assert!(
                answer.starts_with("{\"error\":\"not a request: ") && answer.ends_with("\"}\n"),
                "{line:?}: {answer}"
            );
        }
        let answer = raw_ask(&socket, b"{\"op\":\"status\"}\n");
        assert!(
            answer.starts_with("{\"ok\":{\"answer\":\"status\","),
            "{answer}"
        );
    }

    #[test]
    fn requests_are_read_up_to_16_mib() {
        let folder = TestFolder::new();
        let _server = Server::start(folder.path(), answering).unwrap();
        let socket = own_socket(&folder);
        let limit = Limits::STANDARD.request_bytes;
        assert_eq!(limit, 16 << 20);

        let mut at_limit = b"{\"op\":\"status\"}".to_vec();
        at_limit.resize(limit - 1, b' ');
        at_limit.push(b'\n');
        let answer = raw_ask(&socket, &at_limit);
        assert!(
            answer.starts_with("{\"ok\":"),
            "a request of exactly the limit is read"
        );

        let answer = raw_ask(&socket, &vec![b' '; limit]);
        assert_eq!(
            answer,
            format!(
                "{{\"error\":\"the request is longer than the {limit} bytes an editor reads\"}}\n"
            )
        );
        assert!(
            raw_ask(&socket, b"{\"op\":\"status\"}\n").starts_with("{\"ok\":"),
            "the server goes on"
        );
    }

    #[test]
    fn a_connection_that_sends_no_request_in_time_is_answered_and_closed() {
        let folder = TestFolder::new();
        let socket = own_socket(&folder);
        let limits = Limits {
            request_time: Duration::from_millis(100),
            ..Limits::STANDARD
        };
        let _server = Server::bind(socket.clone(), answering, limits).unwrap();
        for partial in [&b""[..], b"{\"op\":"] {
            let started = Instant::now();
            let answer = raw_ask(&socket, partial);
            assert_eq!(
                answer,
                "{\"error\":\"no request came within 100 milliseconds\"}\n"
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    fn a_window_that_takes_no_jobs_times_out_within_the_wait() {
        let folder = TestFolder::new();
        // A slack far longer than the test: a job never taken does not wait for it.
        let jobs: Jobs<(), Response> = Jobs::with_slack(|| {}, Duration::from_secs(30));
        let _server = Server::start(folder.path(), {
            let jobs = jobs.clone();
            move |_: &Request| Ok(jobs.ask((), Duration::from_millis(100))?)
        })
        .unwrap();
        let taking_no_jobs =
            "the editor's window didn't take the request in time: is a dialog open in it?";

        let started = Instant::now();
        let error = wire::ask(
            &own_socket(&folder),
            &Request::Status,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(matches!(&error, Error::Failed { message } if message == taking_no_jobs));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );

        let found = editors(folder.path()).unwrap();
        assert!(found.answered.is_empty());
        assert_eq!(
            found.problems,
            [Problem::Failed {
                pid: std::process::id(),
                reason: taking_no_jobs.to_owned()
            }]
        );
        assert!(
            jobs.take().is_empty(),
            "the jobs were dropped, never to be done"
        );
    }

    #[test]
    fn a_dropped_server_removes_its_socket() {
        let folder = TestFolder::new();
        let server = Server::start(folder.path(), answering).unwrap();
        let socket = own_socket(&folder);
        assert!(socket.exists());
        drop(server);
        assert!(!socket.exists());
        assert!(UnixStream::connect(&socket).is_err());
        let found = editors(folder.path()).unwrap();
        assert!(found.answered.is_empty() && found.problems.is_empty());
    }

    #[test]
    fn a_lost_socket_is_detected_and_a_new_server_takes_its_place() {
        let folder = TestFolder::new();
        let socket = own_socket(&folder);
        let first = Server::start(folder.path(), answering).unwrap();
        assert!(!first.lost());
        std::fs::remove_file(&socket).unwrap();
        assert!(first.lost(), "the socket was cleaned away");

        // Started before the first is dropped: the first leaves the new socket alone.
        let second = Server::start(folder.path(), answering).unwrap();
        assert!(first.lost(), "another socket in its place");
        assert!(!second.lost());
        drop(first);
        assert!(socket.exists(), "the second's socket stays");
        assert_eq!(editors(folder.path()).unwrap().answered.len(), 1);

        // The whole folder cleaned away: a new server makes it again, private.
        std::fs::remove_dir_all(folder.path()).unwrap();
        assert!(second.lost());
        drop(second);
        let third = Server::start(folder.path(), answering).unwrap();
        assert_eq!(mode(folder.path()), 0o700);
        assert!(!third.lost());
        assert_eq!(editors(folder.path()).unwrap().answered.len(), 1);
    }

    #[test]
    fn a_leftover_socket_of_the_same_name_is_replaced() {
        let folder = TestFolder::new();
        let socket = own_socket(&folder);
        dead_socket(&socket);
        assert!(
            UnixStream::connect(&socket).is_err(),
            "no one listens on it"
        );
        let _server = Server::start(folder.path(), answering).unwrap();
        assert!(raw_ask(&socket, b"{\"op\":\"status\"}\n").starts_with("{\"ok\":"));
    }

    #[test]
    fn connections_past_the_bound_are_told_the_editor_is_busy() {
        let folder = TestFolder::new();
        let socket = own_socket(&folder);
        let (entered, saving_entered) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let released = Mutex::new(released);
        let limits = Limits {
            connections: 1,
            ..Limits::STANDARD
        };
        let handler = move |request: &Request| {
            if matches!(request, Request::Save { .. }) {
                entered.send(()).unwrap();
                released.lock().unwrap().recv().unwrap();
            }
            answering(request)
        };
        let _server = Server::bind(socket.clone(), handler, limits).unwrap();
        let save = Request::Save {
            path: PathBuf::from("/maps/a.tmx"),
        };
        let saving = {
            let socket = socket.clone();
            thread::spawn(move || wire::ask(&socket, &save, Duration::from_secs(10)))
        };
        saving_entered.recv().unwrap();
        let error = wire::ask(&socket, &Request::Status, Duration::from_secs(5)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the editor is answering too many requests at once: try again in a moment"
        );
        release.send(()).unwrap();
        assert!(saving.join().unwrap().is_ok());

        // Once the first connection's thread is done, the next is served.
        let started = Instant::now();
        while wire::ask(&socket, &Request::Status, Duration::from_secs(5)).is_err() {
            assert!(started.elapsed() < Duration::from_secs(5), "never served");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_handler_that_panics_is_answered_with_an_error() {
        let folder = TestFolder::new();
        let socket = own_socket(&folder);
        let _server = Server::start(folder.path(), |request: &Request| {
            if matches!(request, Request::Snapshot { .. }) {
                panic!("no map here");
            }
            answering(request)
        })
        .unwrap();
        let snapshot = Request::Snapshot {
            path: PathBuf::from("/maps/a.tmx"),
        };
        let error = wire::ask(&socket, &snapshot, Duration::from_secs(5)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the editor failed while answering (no map here)"
        );
        assert!(
            wire::ask(&socket, &Request::Status, Duration::from_secs(5)).is_ok(),
            "the server goes on"
        );
    }

    #[test]
    fn a_folder_that_is_not_the_users_alone_is_refused_on_both_sides() {
        let parent = TestFolder::new();
        let real = parent.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = parent.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let file = parent.path().join("file");
        std::fs::write(&file, "").unwrap();
        let reason = |error: Error| match error {
            Error::Folder {
                source: bt_utils::Error::NotPrivate { reason, .. },
            } => reason,
            other => panic!("not refused as not private: {other:?}"),
        };
        for (folder, expected) in [
            (&link, NotPrivate::SymbolicLink),
            (&file, NotPrivate::NotAFolder),
        ] {
            assert_eq!(
                reason(Server::start(folder, answering).unwrap_err()),
                expected
            );
            assert_eq!(reason(editors(folder).unwrap_err()), expected);
        }
        assert_eq!(
            std::fs::read_dir(&real).unwrap().count(),
            0,
            "nothing made through the link"
        );

        // Another user's folder: the root folder is root's, when the tests are not run as root.
        let root = Path::new("/");
        if !rustix::process::getuid().is_root() && std::fs::metadata(root).unwrap().uid() == 0 {
            let owned_by_root = NotPrivate::OwnedByOther { owner: 0 };
            assert_eq!(
                reason(Server::start(root, answering).unwrap_err()),
                owned_by_root
            );
            assert_eq!(reason(editors(root).unwrap_err()), owned_by_root);
        }
    }

    #[test]
    fn a_folder_open_to_others_is_narrowed_before_either_side_uses_it() {
        let folder = TestFolder::new();
        std::fs::set_permissions(folder.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
        let _server = Server::start(folder.path(), answering).unwrap();
        assert_eq!(mode(folder.path()), 0o700, "narrowed by the editor");
        std::fs::set_permissions(folder.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = editors(folder.path()).unwrap();
        assert_eq!(mode(folder.path()), 0o700, "narrowed by bt");
        assert_eq!(found.answered.len(), 1);
    }
}
