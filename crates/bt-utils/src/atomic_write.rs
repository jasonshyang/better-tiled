//! Writing files so that a crash never leaves half a file and a read-only file is never written
//! over. An atomic write puts the bytes in a temporary file next to the target, flushes it to disk
//! and renames it over the target, as Tiled's `QSaveFile` does.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

use crate::error::{Error, Refused};

/// How a file is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriteMode {
    /// To a temporary file next to the target, then renamed over it: a crash leaves the old file
    /// or the new one, never half of either.
    #[default]
    Atomic,
    /// Straight into the target, for synced and network folders where renaming over a file
    /// misbehaves.
    InPlace,
}

/// Writes `bytes` to the file at `path`.
///
/// Symbolic links are followed (the file they point to is replaced and the link stays), and an
/// existing file keeps its permissions. A read-only file is refused rather than replaced, and so
/// is a file in a folder that cannot be written to; [`Error::refused`] says which. Nothing is
/// written when the write fails, and the temporary file is removed.
pub fn write_file(path: &Path, bytes: &[u8], mode: WriteMode) -> Result<(), Error> {
    let target = resolve_target(path)?;
    refuse_read_only_file(path, &target)?;
    let folder_or_io = |source: std::io::Error| folder_refused_or(path, source);
    match mode {
        WriteMode::InPlace => fs::write(&target, bytes).map_err(folder_or_io),
        WriteMode::Atomic => {
            let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
                return Err(Error::Write {
                    path: path.to_path_buf(),
                    source: std::io::Error::new(ErrorKind::InvalidInput, "not a path to a file"),
                });
            };
            let temp =
                TempFile::create_next_to(dir, &name.to_string_lossy()).map_err(folder_or_io)?;
            let temp_path = temp.path.clone();
            let result = temp.replace(&target, bytes);
            if result.is_err() {
                // The write failed and says so; a temporary file that cannot be removed either is
                // left for the user to see next to the target.
                let _ = fs::remove_file(&temp_path);
            }
            result.map_err(folder_or_io)
        }
    }
}

/// The file `path` stands for: the file a symbolic link points to, or `path` made absolute for
/// a new file (a bare file name's folder is the working folder, where its temporary file goes).
fn resolve_target(path: &Path) -> Result<PathBuf, Error> {
    let failed = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };
    match fs::canonicalize(path) {
        Ok(target) => Ok(target),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            std::path::absolute(path).map_err(failed)
        }
        Err(source) => Err(failed(source)),
    }
}

/// Refuses an existing file that cannot be opened for writing: renaming another file over it
/// would get around a lock that version control put there on purpose.
fn refuse_read_only_file(path: &Path, target: &Path) -> Result<(), Error> {
    match OpenOptions::new().write(true).open(target) {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Err(Error::Refused {
            path: path.to_path_buf(),
            reason: Refused::ReadOnlyFile,
        }),
        // Writable, or not there yet (then its folder decides), or a problem the write itself
        // will run into and report.
        _ => Ok(()),
    }
}

/// The error for a write that failed after the file itself was found writable: permissions
/// refused are the folder's.
fn folder_refused_or(path: &Path, source: std::io::Error) -> Error {
    if source.kind() == ErrorKind::PermissionDenied {
        Error::Refused {
            path: path.to_path_buf(),
            reason: Refused::ReadOnlyFolder,
        }
    } else {
        Error::Write {
            path: path.to_path_buf(),
            source,
        }
    }
}

/// A temporary file being written.
struct TempFile {
    path: PathBuf,
    file: File,
}

impl TempFile {
    /// A new file `.<name>.<process id>-<n>.tmp` in `dir`, with the first `n` not taken.
    fn create_next_to(dir: &Path, name: &str) -> std::io::Result<TempFile> {
        const ATTEMPTS: u32 = 1000;
        for n in 0..ATTEMPTS {
            let path = dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok(TempFile { path, file }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "{ATTEMPTS} temporary files for {name} are already in {}",
                dir.display()
            ),
        ))
    }

    /// Writes `bytes`, gives the file the target's permissions, flushes it to disk and renames it
    /// over `target`.
    fn replace(self, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let TempFile { path, mut file } = self;
        file.write_all(bytes)?;
        if let Ok(metadata) = fs::metadata(target) {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&path, target)?;
        // Makes the rename itself durable. Windows cannot open a folder, and there the rename is
        // durable once it returns.
        #[cfg(unix)]
        if let Some(dir) = target.parent() {
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_test_data::Scratch;

    #[test]
    fn a_new_file_named_without_a_folder_is_in_the_working_folder() {
        let name = Path::new("bt-utils-no-such-file-anywhere.png");
        let target = resolve_target(name).unwrap();
        assert_eq!(target, std::env::current_dir().unwrap().join(name));
        assert!(target.parent().is_some_and(|dir| dir.is_dir()));
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn replaces_the_file_and_leaves_nothing_else() {
        let scratch = Scratch::new("bt-utils-write").unwrap();
        let path = scratch.join("map.tmx");
        fs::write(&path, "old contents, longer than the new ones").unwrap();
        write_file(&path, b"new", WriteMode::Atomic).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        write_file(&scratch.join("fresh.tmx"), b"created", WriteMode::Atomic).unwrap();
        write_file(&scratch.join("direct.tmx"), b"in place", WriteMode::InPlace).unwrap();
        assert_eq!(fs::read(scratch.join("direct.tmx")).unwrap(), b"in place");
        assert_eq!(
            entries(scratch.path()),
            ["direct.tmx", "fresh.tmx", "map.tmx"]
        );
    }

    #[test]
    fn read_only_files_are_refused_and_left_alone() {
        let scratch = Scratch::new("bt-utils-write-read-only").unwrap();
        let path = scratch.join("map.tmx");
        fs::write(&path, "locked by version control").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions.clone()).unwrap();
        for mode in [WriteMode::Atomic, WriteMode::InPlace] {
            let error = write_file(&path, b"new", mode).unwrap_err();
            assert_eq!(error.refused(), Some(Refused::ReadOnlyFile), "{error}");
            assert!(
                error
                    .to_string()
                    .ends_with("map.tmx: the file is read-only"),
                "{error}"
            );
        }
        assert_eq!(fs::read(&path).unwrap(), b"locked by version control");
        assert_eq!(
            entries(scratch.path()),
            ["map.tmx"],
            "no temporary file is left"
        );
        #[allow(
            clippy::permissions_set_readonly_false,
            reason = "letting the scratch folder be removed"
        )]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
    }

    #[test]
    fn a_failure_keeps_the_original_and_cleans_up() {
        let scratch = Scratch::new("bt-utils-write-failure").unwrap();
        // Renaming a file over a folder fails after the temporary file was written.
        fs::create_dir(scratch.join("map.tmx")).unwrap();
        fs::write(scratch.join("map.tmx/keep.txt"), "x").unwrap();
        let error = write_file(&scratch.join("map.tmx"), b"data", WriteMode::Atomic).unwrap_err();
        assert!(
            matches!(error, Error::Write { .. }) && error.refused().is_none(),
            "{error}"
        );
        assert!(scratch.join("map.tmx/keep.txt").is_file());
        assert_eq!(entries(scratch.path()), ["map.tmx"]);
        assert!(write_file(&scratch.join("missing/map.tmx"), b"data", WriteMode::Atomic).is_err());
    }

    #[test]
    fn temporary_names_skip_the_ones_taken() {
        let scratch = Scratch::new("bt-utils-write-temp").unwrap();
        let first = format!(".map.tmx.{}-0.tmp", std::process::id());
        fs::write(scratch.join(&first), "someone else's").unwrap();
        let temp = TempFile::create_next_to(scratch.path(), "map.tmx").unwrap();
        assert!(
            temp.path
                .ends_with(format!(".map.tmx.{}-1.tmp", std::process::id()))
        );
        temp.replace(&scratch.join("map.tmx"), b"done").unwrap();
        assert_eq!(
            fs::read_to_string(scratch.join(first)).unwrap(),
            "someone else's"
        );
    }

    #[cfg(unix)]
    #[test]
    fn keeps_permissions_and_follows_links() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("bt-utils-write-unix").unwrap();
        let path = scratch.join("real.tmx");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&path, scratch.join("link.tmx")).unwrap();
        write_file(&scratch.join("link.tmx"), b"new", WriteMode::Atomic).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(
            fs::symlink_metadata(scratch.join("link.tmx"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_is_blamed_not_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("bt-utils-write-locked-folder").unwrap();
        let folder = scratch.join("locked");
        fs::create_dir(&folder).unwrap();
        let original = folder.join("map.tmx");
        fs::write(&original, "original").unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let replaced = write_file(&original, b"new", WriteMode::Atomic);
        let created = write_file(&folder.join("fresh.tmx"), b"new", WriteMode::Atomic);
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        let Err(error) = replaced else {
            return; // Running as root, where folder permissions are not enforced.
        };
        assert_eq!(error.refused(), Some(Refused::ReadOnlyFolder));
        assert!(
            error
                .to_string()
                .ends_with("map.tmx: its folder can't be written to"),
            "{error}"
        );
        assert_eq!(
            created.unwrap_err().refused(),
            Some(Refused::ReadOnlyFolder)
        );
        assert_eq!(fs::read(&original).unwrap(), b"original");
        assert_eq!(entries(&folder), ["map.tmx"]);
    }
}
