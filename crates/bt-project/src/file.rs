//! The project file on disk: finding it for a map, where a new one goes, reading it, the locked
//! read-modify-write that is the only way it is written, and moving a map's entry.
//!
//! The folders that belong to the user rather than to a project are values the caller passes,
//! so that no test touches the user's own: the home folder the search stops below
//! (`std::env::home_dir()` in the programs), and the folder write locks are taken in, the user's
//! runtime folder (`bt_utils::paths::runtime_dir()`), the same for the editor and `bt`.

use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use bt_utils::atomic_write::{WriteMode, write_file};
use bt_utils::hash::fnv1a_64;
use bt_utils::paths::{absolute_path, clean_path, search_up};

use crate::error::Error;
use crate::paths;
use crate::schema::Document;

/// The name of a project file.
pub const FILE_NAME: &str = "better-tiled.json";

/// The operations on project files. Not a value: each takes the paths it works on, and the
/// user's folders as values (see the module's documentation).
#[derive(Debug)]
pub enum ProjectFile {}

/// Where a map's entry is, or goes: the project file that covers the map, and the map's key in
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPlace {
    /// The project file, in the form the map's path has.
    pub file: PathBuf,
    /// The map's key in it (`crate::paths::key`).
    pub key: String,
}

impl EntryPlace {
    /// The place of the entry of the map at `map`: in the file [`ProjectFile::location`] gives
    /// (the one found for the map, else where a new one goes), under the map's key there.
    pub fn for_map(
        map: &Path,
        open_project: Option<&Path>,
        home: Option<&Path>,
    ) -> Result<EntryPlace, Error> {
        let file = ProjectFile::location(map, open_project, home)?;
        let key = paths::key(&file, map)?;
        Ok(EntryPlace { file, key })
    }
}

impl ProjectFile {
    /// The project file of the map at `map`: `better-tiled.json` in the map's own folder, else
    /// in the nearest folder above that has one. The search stops after the top of the map's
    /// repository (a folder with `.git`, `.hg` or `.svn`) and before `home`, the user's home
    /// folder: a file higher up belongs to something else, and a clone of the repository would
    /// not have it. The map's own folder is searched even when it is the home folder.
    ///
    /// The path is in the form the map's path has: made absolute and cleaned, with no link
    /// followed, so the paths the file gives back are in that form too.
    pub fn find(map: &Path, home: Option<&Path>) -> Result<Option<PathBuf>, Error> {
        let folder = folder_of(map)?;
        let home = home.map(clean_path);
        let file_in = |dir: &Path| {
            let file = dir.join(FILE_NAME);
            file.is_file().then_some(file)
        };
        Ok(file_in(&folder).or_else(|| search_up(&folder, home.as_deref(), file_in)))
    }

    /// Where the entry of the map at `map` is or goes: the file [`ProjectFile::find`] finds;
    /// else `better-tiled.json` next to `open_project`, the open `.tiled-project`, when the map
    /// is in that folder or below and the map's search reaches it; else next to the map. Never
    /// where the map's own search would not find it again. In the form the map's path has.
    pub fn location(
        map: &Path,
        open_project: Option<&Path>,
        home: Option<&Path>,
    ) -> Result<PathBuf, Error> {
        if let Some(found) = ProjectFile::find(map, home)? {
            return Ok(found);
        }
        let folder = folder_of(map)?;
        if let Some(project) = open_project {
            let project_folder = folder_of(project)?;
            let home = home.map(clean_path);
            let at_project = |dir: &Path| same_file(dir, &project_folder).then(|| dir.to_owned());
            let reached =
                at_project(&folder).or_else(|| search_up(&folder, home.as_deref(), &at_project));
            if let Some(dir) = reached {
                return Ok(dir.join(FILE_NAME));
            }
        }
        Ok(folder.join(FILE_NAME))
    }

    /// The document of the project file at `path`. A missing file reads as the empty document;
    /// a file that cannot be read or parsed is an error naming the file and the reason, and
    /// callers stop on it rather than start afresh. A file written by a newer version is read as
    /// far as it is understood ([`Document::version_note`]).
    pub fn load(path: &Path) -> Result<Document, Error> {
        match fs::read_to_string(path) {
            Ok(text) => Document::from_json(&text).map_err(|source| Error::Parse {
                path: path.to_path_buf(),
                source,
            }),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(Document::default()),
            Err(source) => Err(Error::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Changes the project file at `path`: the only way the file is written. Under an exclusive
    /// lock taken in `lock_folder` (the user's runtime folder, so the editor and `bt` exclude
    /// each other; never beside the file), the file is read again, `change` is applied to its
    /// document, and the result is written through a temporary file and a rename, so a crash
    /// never leaves half a file. `change` replaces the caller's own records and nothing else
    /// (`Document::set_automap`, `Document::set_godot`); what it returns is returned.
    ///
    /// A change that leaves the document as it was writes nothing, so the file's modification
    /// time stays meaningful and a missing file stays missing. A file that cannot be read or
    /// parsed is an error before `change` runs, and nothing is written over it. A read-only file
    /// or folder is [`Error::Write`], with [`Error::refused`] saying which.
    pub fn edit<T>(
        path: &Path,
        lock_folder: &Path,
        change: impl FnOnce(&mut Document) -> T,
    ) -> Result<T, Error> {
        let _lock = WriteLock::take(path, lock_folder)?;
        let before = ProjectFile::load(path)?;
        let mut document = before.clone();
        let result = change(&mut document);
        if document != before {
            let text = document.to_json().map_err(|source| Error::Serialize {
                path: path.to_path_buf(),
                source,
            })?;
            write_file(path, text.as_bytes(), WriteMode::Atomic)
                .map_err(|source| Error::Write { source })?;
        }
        Ok(result)
    }

    /// Moves a map's entry from `from` to `to` (Save As, a move made in the editor), replacing
    /// any entry `to` had, and returns whether there was one to move. Within one file it is one
    /// [`ProjectFile::edit`]. Across files the entry is inserted into `to`'s file first and then
    /// removed from `from`'s, each under its own lock, so a failure in between leaves the entry
    /// in both files rather than in neither; it is removed only while it is still what was
    /// copied.
    pub fn move_entry(
        from: &EntryPlace,
        to: &EntryPlace,
        lock_folder: &Path,
    ) -> Result<bool, Error> {
        if same_file(&from.file, &to.file) {
            return ProjectFile::edit(&from.file, lock_folder, |document| {
                document.move_entry(&from.key, &to.key)
            });
        }
        let Some(entry) = ProjectFile::load(&from.file)?.entry(&from.key).cloned() else {
            return Ok(false);
        };
        ProjectFile::edit(&to.file, lock_folder, |document| {
            document.insert_entry(&to.key, entry.clone());
        })?;
        ProjectFile::edit(&from.file, lock_folder, |document| {
            if document.entry(&from.key) == Some(&entry) {
                document.remove_entry(&from.key);
            }
        })?;
        Ok(true)
    }
}

/// The folder of the file at `path`, made absolute and cleaned, with no link followed.
fn folder_of(path: &Path) -> Result<PathBuf, Error> {
    let path = absolute_path(path).map_err(|source| Error::Resolve { source })?;
    // Only a root has no parent; it is its own folder.
    Ok(path
        .parent()
        .map_or_else(|| path.clone(), Path::to_path_buf))
}

/// Whether two paths lead to the same file or folder: equal as written, or once links are
/// followed.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (fs::canonicalize(a), fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

/// An exclusive advisory lock on one project file, held until dropped: a lock file in the lock
/// folder, named from a hash of the project file's canonical path (so every program reaching the
/// file, through whatever links, takes the same lock) and locked with the system's file locks,
/// which the system releases when a program dies.
struct WriteLock {
    _file: File,
}

impl WriteLock {
    /// Waits for the lock of the project file at `path` and takes it.
    fn take(path: &Path, lock_folder: &Path) -> Result<WriteLock, Error> {
        let lock = lock_folder.join(lock_name(path)?);
        let failed = |source: std::io::Error| Error::Lock {
            path: path.to_path_buf(),
            lock: lock.clone(),
            source,
        };
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock)
            .map_err(failed)?;
        file.lock().map_err(failed)?;
        Ok(WriteLock { _file: file })
    }
}

/// The name of the lock file of the project file at `path`.
fn lock_name(path: &Path) -> Result<String, Error> {
    let identity = identity(path)?;
    let hash = fnv1a_64(identity.as_os_str().as_encoded_bytes());
    Ok(format!("project-{hash:016x}.lock"))
}

/// The path of a project file that every program reaching it agrees on: canonical, with links
/// followed; for a file not made yet, its folder's canonical path and its name.
fn identity(path: &Path) -> Result<PathBuf, Error> {
    if let Ok(canonical) = fs::canonicalize(path) {
        return Ok(canonical);
    }
    let absolute = absolute_path(path).map_err(|source| Error::Resolve { source })?;
    Ok(match (absolute.parent(), absolute.file_name()) {
        (Some(folder), Some(name)) => {
            fs::canonicalize(folder).map_or_else(|_| absolute.clone(), |folder| folder.join(name))
        }
        _ => absolute,
    })
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use super::*;

    #[test]
    fn a_file_has_one_lock_however_it_is_reached() {
        let scratch = Scratch::new("bt-project-lock-name").unwrap();
        let file = scratch.join("game/better-tiled.json");
        fs::create_dir_all(scratch.join("game")).unwrap();
        let before = lock_name(&file).unwrap();
        assert!(
            before.starts_with("project-") && before.ends_with(".lock") && before.len() == 29,
            "{before}"
        );
        assert_eq!(
            lock_name(&scratch.join("game/./x/../better-tiled.json")).unwrap(),
            before
        );
        fs::write(&file, "{}").unwrap();
        assert_eq!(lock_name(&file).unwrap(), before, "made since");
        assert_ne!(
            lock_name(&scratch.join("other/better-tiled.json")).unwrap(),
            before
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(scratch.join("game"), scratch.join("link")).unwrap();
            assert_eq!(
                lock_name(&scratch.join("link/better-tiled.json")).unwrap(),
                before,
                "through a link"
            );
            assert!(same_file(&scratch.join("link/better-tiled.json"), &file));
        }
        assert!(!same_file(&file, &scratch.join("game/other.json")));
    }

    #[test]
    fn a_lock_folder_that_is_missing_is_an_error_naming_both_files() {
        let scratch = Scratch::new("bt-project-lock-missing").unwrap();
        let file = scratch.join("better-tiled.json");
        let error = ProjectFile::edit(&file, &scratch.join("missing"), |_| ()).unwrap_err();
        assert!(matches!(error, Error::Lock { .. }), "{error}");
        let message = error.to_string();
        assert!(
            message.contains("better-tiled.json") && message.contains("missing"),
            "{message}"
        );
        assert_eq!(error.refused(), None);
        assert!(!file.exists());
    }
}
