//! Where editors listen (`11-live-link.md`, §4): a folder of the user's alone, each editor's
//! socket in it named after its process, `<folder>/<pid>.sock`.
//!
//! The folder is `BT_LIVE_DIR` when it is set, else the user's runtime folder,
//! `/tmp/better-tiled-<uid>`: short, so that a socket path fits the hundred-odd bytes a socket's
//! path may have, and the same for every program of the user whatever `$HOME` is (agent
//! sandboxes change it). Both sides check it before use: anyone else able to write there could
//! listen in the editor's place and feed an agent a false map.

use std::ffi::OsString;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use crate::error::Error;

/// The environment variable naming another folder for the live link, by its absolute path.
pub const LIVE_DIR_VAR: &str = "BT_LIVE_DIR";

/// The longest path of a socket: a Unix domain socket's address has room for 104 bytes on macOS
/// and the BSDs (108 on Linux), the terminating zero included.
#[cfg(unix)]
const MAX_SOCKET_PATH: usize = 103;

/// The longest name of a socket in the folder, with the `/` before it: ten digits of process id
/// and `.sock`.
#[cfg(unix)]
const MAX_SOCKET_NAME: usize = 16;

/// The folder editors listen in and `bt` looks in: `BT_LIVE_DIR` when it is set, else the user's
/// runtime folder (`bt_utils::paths::runtime_dir`). Both programs call this, so neither reads
/// the variable itself.
///
/// On Unix the folder is made with mode `0700` when it is missing and checked: a folder (not a
/// symbolic link) of the current user's that no one else can use, a wider mode narrowed to
/// `0700`. A folder that fails is [`Error::Folder`] saying why, and is not used. A `BT_LIVE_DIR`
/// that is not an absolute path, or too long for a socket path in it, is refused too. On
/// Windows, which has no live link, the folder is only named.
pub fn live_dir() -> Result<PathBuf, Error> {
    live_dir_from(std::env::var_os(LIVE_DIR_VAR))
}

/// [`live_dir`] with the variable's value given.
pub(crate) fn live_dir_from(value: Option<OsString>) -> Result<PathBuf, Error> {
    let Some(value) = value else {
        return Ok(bt_utils::paths::runtime_dir()?);
    };
    let folder = PathBuf::from(value);
    if !folder.is_absolute() {
        return Err(Error::LiveDirNotAbsolute { value: folder });
    }
    #[cfg(unix)]
    {
        if folder.as_os_str().len() + MAX_SOCKET_NAME > MAX_SOCKET_PATH {
            return Err(Error::LiveDirTooLong {
                value: folder,
                max: MAX_SOCKET_PATH,
            });
        }
        bt_utils::paths::make_private_dir(&folder)?;
    }
    Ok(folder)
}

/// The socket the editor with process id `pid` listens on in `folder`.
#[cfg(unix)]
pub(crate) fn socket_path(folder: &Path, pid: u32) -> PathBuf {
    folder.join(format!("{pid}.sock"))
}

/// The process id an editor's socket is named after, or `None` for a file that is not named as
/// a socket of the live link (`<digits>.sock`).
#[cfg(unix)]
pub(crate) fn socket_pid(socket: &Path) -> Option<u32> {
    if socket.extension()? != "sock" {
        return None;
    }
    let stem = socket.file_stem()?.to_str()?;
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

/// Whether the process `pid` is running: a signal could be sent to it (`kill` with no signal),
/// or it exists but belongs to someone else. A process that has exited is not, and neither is
/// an id no process can have.
#[cfg(unix)]
pub(crate) fn process_alive(pid: u32) -> bool {
    let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return false;
    };
    match rustix::process::test_kill_process(pid) {
        Ok(()) => true,
        Err(error) => error == rustix::io::Errno::PERM,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_live_dir_is_refused() {
        for value in ["", "live", "./live"] {
            match live_dir_from(Some(OsString::from(value))) {
                Err(Error::LiveDirNotAbsolute { value: refused }) => {
                    assert_eq!(refused, PathBuf::from(value));
                }
                other => panic!("{value:?} was not refused: {other:?}"),
            }
        }
        let error = live_dir_from(Some(OsString::from("live"))).unwrap_err();
        assert_eq!(
            error.to_string(),
            "BT_LIVE_DIR is not used: \"live\" is not an absolute path, so bt and the editor could each take it for another folder"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_live_dir_too_long_for_a_socket_is_refused() {
        let long = format!("/tmp/{}", "x".repeat(MAX_SOCKET_PATH));
        match live_dir_from(Some(OsString::from(&long))) {
            Err(Error::LiveDirTooLong { value, max }) => {
                assert_eq!(value, PathBuf::from(&long));
                assert_eq!(max, MAX_SOCKET_PATH);
            }
            other => panic!("not refused: {other:?}"),
        }
        assert!(!Path::new(&long).exists(), "nothing made");
    }

    #[cfg(unix)]
    #[test]
    fn a_live_dir_is_made_private_before_use() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let parent = crate::testing::TestFolder::new();
        let folder = parent.path().join("live");
        let found = live_dir_from(Some(folder.clone().into_os_string())).unwrap();
        assert_eq!(found, folder);
        let mode = |path: &Path| std::fs::symlink_metadata(path).unwrap().mode() & 0o777;
        assert_eq!(mode(&folder), 0o700, "made with mode 0700");
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        live_dir_from(Some(folder.clone().into_os_string())).unwrap();
        assert_eq!(mode(&folder), 0o700, "narrowed");

        let link = parent.path().join("link");
        std::os::unix::fs::symlink(&folder, &link).unwrap();
        let error = live_dir_from(Some(link.clone().into_os_string())).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{} is not used: it is a symbolic link", link.display())
        );
    }

    #[cfg(unix)]
    #[test]
    fn sockets_are_named_after_their_process() {
        let socket = socket_path(Path::new("/tmp/better-tiled-501"), 4242);
        assert_eq!(socket, PathBuf::from("/tmp/better-tiled-501/4242.sock"));
        assert_eq!(socket_pid(&socket), Some(4242));
        for other in [
            "/tmp/x/4242.lock",
            "/tmp/x/+12.sock",
            "/tmp/x/.sock",
            "/tmp/x/editor.sock",
            "/tmp/x/99999999999.sock",
            "/tmp/x/4242",
        ] {
            assert_eq!(socket_pid(Path::new(other)), None, "{other}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn running_processes_are_alive_and_others_are_not() {
        assert!(process_alive(std::process::id()), "this one");
        assert!(process_alive(1), "the first process, someone else's");
        // No process is started here: a child holds a copy of every open socket until it runs
        // its program, which would keep other tests' closed sockets listening. An id above
        // every system's largest is a process that has exited as far as `kill` can tell.
        assert!(
            !process_alive(crate::testing::NO_PROCESS),
            "no such process"
        );
        assert!(!process_alive(0), "no process has id 0");
        assert!(!process_alive(u32::MAX), "too large an id");
    }
}
