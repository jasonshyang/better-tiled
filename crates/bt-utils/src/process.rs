//! External processes with streamed, bounded output and cancellation of their process group.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

/// An executable and its already separated arguments; no implicit shell expansion.
#[derive(Clone, Debug)]
pub struct ProcessJob {
    program: String,
    arguments: Vec<String>,
    directory: Option<PathBuf>,
}

/// A chunk from one process output stream.
#[derive(Clone, Debug)]
pub struct ProcessText {
    /// Standard error rather than standard output.
    pub stderr: bool,
    /// Text decoded lossily from the process's bytes.
    pub text: String,
}

/// A completed process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessExit {
    /// Its exit code, or no code when a signal stopped it.
    pub code: Option<i32>,
    /// The caller requested cancellation.
    pub cancelled: bool,
}

impl ProcessJob {
    /// Runs `program` in the caller's working directory, without arguments.
    pub fn new(program: String) -> Self {
        Self {
            program,
            arguments: Vec::new(),
            directory: None,
        }
    }
    /// The arguments to pass as literal values.
    pub fn arguments(mut self, arguments: Vec<String>) -> Self {
        self.arguments = arguments;
        self
    }
    /// The child's working directory.
    pub fn directory(mut self, directory: Option<PathBuf>) -> Self {
        self.directory = directory;
        self
    }
    /// The executable name or path.
    pub fn program(&self) -> &str {
        &self.program
    }
    /// The literal arguments.
    pub fn args(&self) -> &[String] {
        &self.arguments
    }

    /// Runs off the frame thread, reporting up to 2 MiB from each output stream. Excess output
    /// is drained and reported as truncated. Cancellation also stops descendants on Unix.
    pub fn run(
        &self,
        cancel: Arc<AtomicBool>,
        output: impl Fn(ProcessText) + Send + Sync + 'static,
    ) -> Result<ProcessExit, String> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(ProcessExit {
                code: None,
                cancelled: true,
            });
        }
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(directory) = &self.directory {
            command.current_dir(directory);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Cannot start {}: {error}", self.program))?;
        let output = Arc::new(output);
        let readers = (|| -> Result<Readers, String> {
            let stdout = child
                .stdout
                .take()
                .ok_or("The process has no output pipe")?;
            let stderr = child.stderr.take().ok_or("The process has no error pipe")?;
            let first_output = Arc::clone(&output);
            let first = std::thread::Builder::new()
                .name("command stdout".into())
                .spawn(move || drain(stdout, false, first_output))
                .map_err(|error| error.to_string())?;
            let second = std::thread::Builder::new()
                .name("command stderr".into())
                .spawn(move || drain(stderr, true, output))
                .map_err(|error| error.to_string())?;
            Ok(Readers {
                stdout: first,
                stderr: second,
            })
        })();
        let readers = match readers {
            Ok(readers) => readers,
            Err(error) => {
                let _ = stop(&mut child);
                let _ = child.wait();
                return Err(error);
            }
        };
        let mut running = Running {
            child,
            finished: false,
        };
        let mut status = None;
        let mut cancelled = false;
        while status.is_none() || !readers.stdout.is_finished() || !readers.stderr.is_finished() {
            if cancel.load(Ordering::Relaxed) && !cancelled {
                stop(&mut running.child)?;
                cancelled = true;
            }
            if status.is_none() {
                status = running
                    .child
                    .try_wait()
                    .map_err(|error| error.to_string())?;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        readers
            .stdout
            .join()
            .map_err(|_| "The command output reader panicked")??;
        readers
            .stderr
            .join()
            .map_err(|_| "The command error reader panicked")??;
        let status = status.ok_or("The process ended without an exit status")?;
        running.finished = true;
        Ok(ProcessExit {
            code: status.code(),
            cancelled,
        })
    }
}

struct Readers {
    stdout: JoinHandle<Result<(), String>>,
    stderr: JoinHandle<Result<(), String>>,
}

struct Running {
    child: Child,
    finished: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.finished {
            // An early error still owns this process and must not leave it running.
            let _ = stop(&mut self.child);
            let _ = self.child.wait();
        }
    }
}

fn drain(
    mut pipe: impl Read,
    stderr: bool,
    output: Arc<impl Fn(ProcessText)>,
) -> Result<(), String> {
    const LIMIT: usize = 2 * 1024 * 1024;
    let mut remaining = LIMIT;
    let mut truncated = false;
    let mut buffer = [0; 4096];
    let mut pending = Vec::new();
    loop {
        let read = pipe.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        let kept = read.min(remaining);
        if kept > 0 {
            pending.extend_from_slice(buffer.get(..kept).ok_or("Invalid output buffer range")?);
            let text = decode_complete(&mut pending)?;
            if !text.is_empty() {
                output(ProcessText { stderr, text });
            }
            remaining -= kept;
        }
        if kept < read && !truncated {
            output(ProcessText {
                stderr,
                text: "\n[Further output truncated after 2 MiB]\n".into(),
            });
            truncated = true;
        }
    }
    if !pending.is_empty() {
        output(ProcessText {
            stderr,
            text: String::from_utf8_lossy(&pending).into_owned(),
        });
    }
    Ok(())
}

fn decode_complete(pending: &mut Vec<u8>) -> Result<String, String> {
    let mut text = String::new();
    let mut consumed = 0;
    loop {
        let rest = pending
            .get(consumed..)
            .ok_or("Invalid UTF-8 output boundary")?;
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                consumed = pending.len();
                break;
            }
            Err(error) => {
                let prefix = rest
                    .get(..error.valid_up_to())
                    .ok_or("Invalid UTF-8 prefix")?;
                text.push_str(std::str::from_utf8(prefix).map_err(|error| error.to_string())?);
                consumed += error.valid_up_to();
                let Some(invalid) = error.error_len() else {
                    break;
                };
                text.push('\u{fffd}');
                consumed += invalid;
            }
        }
    }
    pending.drain(..consumed);
    Ok(text)
}

fn stop(child: &mut Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        let raw = i32::try_from(child.id()).map_err(|error| error.to_string())?;
        let pid = rustix::process::Pid::from_raw(raw).ok_or("The command has no process id")?;
        match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(error) => Err(format!("Cannot stop the command: {error}")),
        }
    }
    #[cfg(not(unix))]
    {
        child
            .kill()
            .map_err(|error| format!("Cannot stop the command: {error}"))
    }
}

/// Finds an executable for the `%executablepath` working-directory placeholder. Launching a
/// plain program name still uses the operating system's normal PATH lookup.
pub fn executable(program: &str) -> Result<PathBuf, String> {
    let path = std::path::Path::new(program);
    if path.components().count() > 1 || path.is_absolute() {
        return std::path::absolute(path).map_err(|error| error.to_string());
    }
    let search = std::env::var_os("PATH").ok_or("PATH is not set")?;
    for folder in std::env::split_paths(&search) {
        let candidate = folder.join(path);
        if candidate.is_file() {
            return std::path::absolute(candidate).map_err(|error| error.to_string());
        }
        #[cfg(windows)]
        {
            let candidate = candidate.with_extension("exe");
            if candidate.is_file() {
                return std::path::absolute(candidate).map_err(|error| error.to_string());
            }
        }
    }
    Err(format!("Cannot find {program} on PATH"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[cfg(unix)]
    use std::sync::mpsc;

    #[test]
    fn unicode_split_across_output_reads_is_preserved() {
        let mut pending = vec![b'a', 0xe4];
        assert_eq!(decode_complete(&mut pending).unwrap(), "a");
        pending.extend_from_slice(&[0xbd, 0xa0, 0xff, 0xe5]);
        assert_eq!(decode_complete(&mut pending).unwrap(), "你�");
        pending.extend_from_slice(&[0xa5, 0xbd]);
        assert_eq!(decode_complete(&mut pending).unwrap(), "好");
        assert!(pending.is_empty());
    }

    #[test]
    fn excessive_output_is_drained_without_growing_the_console_message_queue() {
        let received = Arc::new(Mutex::new(String::new()));
        let sink = Arc::clone(&received);
        let bytes = std::io::Cursor::new(vec![b'x'; 3 * 1024 * 1024]);
        drain(
            bytes,
            false,
            Arc::new(move |chunk: ProcessText| sink.lock().unwrap().push_str(&chunk.text)),
        )
        .unwrap();
        let text = received.lock().unwrap();
        assert!(text.ends_with("[Further output truncated after 2 MiB]\n"));
        assert!(text.len() < 2 * 1024 * 1024 + 100);
    }

    #[test]
    #[cfg(unix)]
    fn output_streams_and_failure_exit_codes_are_kept() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&chunks);
        let exit = ProcessJob::new("/bin/sh".into())
            .arguments(vec![
                "-c".into(),
                "printf hello; printf problem >&2; exit 7".into(),
            ])
            .run(Arc::new(AtomicBool::new(false)), move |text| {
                received.lock().unwrap().push(text)
            })
            .unwrap();
        assert_eq!(exit.code, Some(7));
        let chunks = chunks.lock().unwrap();
        assert_eq!(
            chunks
                .iter()
                .filter(|text| !text.stderr)
                .map(|text| text.text.as_str())
                .collect::<String>(),
            "hello"
        );
        assert_eq!(
            chunks
                .iter()
                .filter(|text| text.stderr)
                .map(|text| text.text.as_str())
                .collect::<String>(),
            "problem"
        );
    }

    #[test]
    #[cfg(unix)]
    fn cancelling_stops_descendants_that_keep_the_output_pipe_open() {
        let cancel = Arc::new(AtomicBool::new(false));
        let request = Arc::clone(&cancel);
        let (started, started_rx) = mpsc::channel();
        let (finished, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = ProcessJob::new("/bin/sh".into())
                .arguments(vec!["-c".into(), "printf started; sleep 30 & wait".into()])
                .run(request, move |_| {
                    let _ = started.send(());
                });
            finished.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        cancel.store(true, Ordering::Relaxed);
        let exit = finished_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert!(exit.cancelled);
        worker.join().unwrap();
    }
}
