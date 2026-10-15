//! Path helpers: lexical cleaning, absolute and relative paths, file stems and labels, the
//! upward search that finds a project file above a map, and the user's private runtime folder
//! with the check that keeps a folder the user's alone.

use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use crate::error::NotPrivate;
use crate::error::{Error, NoDataFolder};

/// The folders whose presence marks the top of a repository.
const REPOSITORY_MARKERS: [&str; 3] = [".git", ".hg", ".svn"];

/// `path` with `.` removed and `..` folded into the folder before it, without touching the file
/// system (Qt's `QDir::cleanPath`). `..` above the root stays at the root; `..` at the start of a
/// relative path is kept. An empty result is `.`.
pub fn clean_path(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => out.push(component),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        PathBuf::from(".")
    } else {
        out.iter().collect()
    }
}

/// `path` made absolute against the working folder and cleaned, without following links.
pub fn absolute_path(path: &Path) -> Result<PathBuf, Error> {
    std::path::absolute(path)
        .map(|absolute| clean_path(&absolute))
        .map_err(|source| Error::Resolve {
            path: path.to_path_buf(),
            source,
        })
}

/// `target` relative to the folder `base_dir`, with `/` separators and `..` where needed, as
/// files refer to each other. `None` when there is no such path: one is absolute and the other
/// is not, or they are on different Windows drives.
pub fn relative_path(base_dir: &Path, target: &Path) -> Option<String> {
    if base_dir.is_absolute() != target.is_absolute() {
        return None;
    }
    let base = clean_path(base_dir);
    let target = clean_path(target);
    let base: Vec<Component> = base
        .components()
        .filter(|c| *c != Component::CurDir)
        .collect();
    let target: Vec<Component> = target
        .components()
        .filter(|c| *c != Component::CurDir)
        .collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for component in base.iter().skip(common) {
        match component {
            Component::Normal(_) => parts.push("..".to_owned()),
            _ => return None,
        }
    }
    for component in target.iter().skip(common) {
        match component {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Resolves an existing file's symbolic links, or cleans a path for a file not created yet.
/// This is for comparing filesystem identities; project-file references use `clean_path`.
pub fn canonical_or_clean(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| clean_path(path))
}

/// A file reference relative to `base_dir`, or its absolute path on another drive.
/// Uses `/` separators on every platform.
pub fn reference_path(base_dir: &Path, target: &Path) -> String {
    relative_path(base_dir, target).unwrap_or_else(|| target.to_string_lossy().replace('\\', "/"))
}

/// The file name of a reference (`/` or `\` separated) without its last extension (Qt's
/// `QFileInfo::completeBaseName`): `"../Tiles/ground.v2.tsx"` gives `"ground.v2"`. A name that
/// starts with its only dot keeps it.
pub fn file_stem(reference: &str) -> &str {
    let name = reference.rsplit(['/', '\\']).next().unwrap_or(reference);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// The name to show for a file: its file name, or the whole path when it has none.
pub fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The folder of better-tiled's own files, shared by the editor and `bt`:
/// `~/Library/Application Support/better-tiled` on macOS, `%APPDATA%\better-tiled` on Windows,
/// `$XDG_DATA_HOME/better-tiled` (or `~/.local/share/better-tiled`) elsewhere. An error names the
/// variable that is not set.
pub fn data_dir() -> Result<PathBuf, NoDataFolder> {
    let var = |name: &'static str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or(NoDataFolder { variable: name })
    };
    if cfg!(target_os = "macos") {
        return Ok(var("HOME")?.join("Library/Application Support/better-tiled"));
    }
    if cfg!(target_os = "windows") {
        return Ok(var("APPDATA")?.join("better-tiled"));
    }
    if let Ok(xdg) = var("XDG_DATA_HOME") {
        return Ok(xdg.join("better-tiled"));
    }
    Ok(var("HOME")?.join(".local/share/better-tiled"))
}

/// Searches `start` and the folders above it, nearest first, for the first one `found` returns
/// something for. The search stops after the top of a repository (a folder with `.git`, `.hg` or
/// `.svn`) and before `home`, the user's home folder: a file found higher up belongs to something
/// else, and a clone of the repository would not have it.
pub fn search_up<T>(
    start: &Path,
    home: Option<&Path>,
    mut found: impl FnMut(&Path) -> Option<T>,
) -> Option<T> {
    for dir in start.ancestors() {
        if home.is_some_and(|home| home == dir) {
            return None;
        }
        if let Some(thing) = found(dir) {
            return Some(thing);
        }
        if REPOSITORY_MARKERS
            .iter()
            .any(|marker| dir.join(marker).exists())
        {
            return None;
        }
    }
    None
}

/// The user's runtime folder: where the live link's sockets and the project files' write locks
/// are kept. It is private to the user and the same for every program of theirs, whatever
/// `$HOME` is.
///
/// On Unix it is `/tmp/better-tiled-<uid>`, short so that a socket path in it stays well below
/// the hundred-odd bytes a socket path may have. It is made with mode `0700` when missing and
/// checked on every call: it must be a folder (not a symbolic link) that the user owns and no
/// one else can read, write or enter. A wider mode is narrowed to `0700`; any other failure is
/// [`Error::NotPrivate`], and the folder is not used.
///
/// On Windows, which has no live link, it is `better-tiled` in the user's temporary folder
/// (`%TEMP%`, inside the user's own profile, so other users cannot reach it), made when missing.
pub fn runtime_dir() -> Result<PathBuf, Error> {
    #[cfg(unix)]
    {
        let uid = rustix::process::getuid().as_raw();
        let dir = runtime_dir_of(uid);
        make_private_dir_of(&dir, uid)?;
        Ok(dir)
    }
    #[cfg(not(unix))]
    {
        let dir = std::env::temp_dir().join("better-tiled");
        std::fs::create_dir_all(&dir).map_err(|source| Error::Folder {
            path: dir.clone(),
            source,
        })?;
        Ok(dir)
    }
}

/// The runtime folder of the user `uid` on Unix.
#[cfg(unix)]
fn runtime_dir_of(uid: u32) -> PathBuf {
    PathBuf::from(format!("/tmp/better-tiled-{uid}"))
}

/// Makes `dir` a folder of the current user alone, the check [`runtime_dir`] makes of its own
/// folder, for a folder chosen otherwise (the live link's `BT_LIVE_DIR`): creates it with mode
/// `0700` when it is missing, refuses a symbolic link, anything that is not a folder and a folder
/// someone else owns ([`Error::NotPrivate`], saying which), and narrows a mode open to others to
/// `0700`. Unix only.
#[cfg(unix)]
pub fn make_private_dir(dir: &Path) -> Result<(), Error> {
    make_private_dir_of(dir, rustix::process::getuid().as_raw())
}

/// Makes `dir` a folder of the user `uid` alone: creates it with mode `0700` when it is missing,
/// refuses a symbolic link, anything that is not a folder and a folder someone else owns, and
/// narrows a mode open to others to `0700`. In `/tmp`, whose sticky bit lets only an entry's
/// owner rename or remove it, a folder that passed cannot be swapped for another afterwards.
#[cfg(unix)]
fn make_private_dir_of(dir: &Path, uid: u32) -> Result<(), Error> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let folder = |source: std::io::Error| Error::Folder {
        path: dir.to_path_buf(),
        source,
    };
    let not_private = |reason: NotPrivate| Error::NotPrivate {
        path: dir.to_path_buf(),
        reason,
    };
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(source) => return Err(folder(source)),
    }
    let metadata = std::fs::symlink_metadata(dir).map_err(folder)?;
    if metadata.file_type().is_symlink() {
        return Err(not_private(NotPrivate::SymbolicLink));
    }
    if !metadata.is_dir() {
        return Err(not_private(NotPrivate::NotAFolder));
    }
    if metadata.uid() != uid {
        return Err(not_private(NotPrivate::OwnedByOther {
            owner: metadata.uid(),
        }));
    }
    if metadata.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).map_err(folder)?;
        let mode = std::fs::symlink_metadata(dir).map_err(folder)?.mode() & 0o7777;
        if mode & 0o077 != 0 {
            return Err(not_private(NotPrivate::OpenToOthers { mode }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_test_data::Scratch;

    #[test]
    fn cleaning_folds_dots_lexically() {
        let clean = |p: &str| clean_path(Path::new(p));
        assert_eq!(
            clean("/maps/./world/../tiles/a.tsx"),
            PathBuf::from("/maps/tiles/a.tsx")
        );
        assert_eq!(clean("/.."), PathBuf::from("/"));
        assert_eq!(clean("../a/../../b"), PathBuf::from("../../b"));
        assert_eq!(clean("a/.."), PathBuf::from("."));
        assert_eq!(clean(""), PathBuf::from("."));
    }

    #[test]
    fn relative_paths_climb_with_dots() {
        let relative = |a: &str, b: &str| relative_path(Path::new(a), Path::new(b));
        assert_eq!(relative("/a/b", "/a/b/c.tsx").as_deref(), Some("c.tsx"));
        assert_eq!(
            relative("/a/b/maps", "/a/b/tilesets/c.tsx").as_deref(),
            Some("../tilesets/c.tsx")
        );
        assert_eq!(relative("/a/b/", "/a/b").as_deref(), Some(""));
        assert_eq!(relative("/a/./b", "/a/x/../b/c").as_deref(), Some("c"));
        assert_eq!(relative("/a", "b.tsx"), None);
        assert_eq!(
            relative("maps", "tiles/a.tsx").as_deref(),
            Some("../tiles/a.tsx")
        );
    }

    #[test]
    fn stems_and_names() {
        assert_eq!(
            file_stem("../Tilesets/Terrain - Highlands.tsx"),
            "Terrain - Highlands"
        );
        assert_eq!(file_stem(":/automap-tiles.tsx"), "automap-tiles");
        assert_eq!(file_stem("C:\\art\\ground.v2.png"), "ground.v2");
        assert_eq!(file_stem(".hidden"), ".hidden");
        assert_eq!(file_stem("noext"), "noext");
        assert_eq!(display_name(Path::new("/maps/level 1.tmx")), "level 1.tmx");
        assert_eq!(display_name(Path::new("/")), "/");
    }

    #[test]
    fn absolute_paths_are_clean() {
        let absolute = absolute_path(Path::new("a/./b/../c.tmx")).unwrap();
        assert!(
            absolute.is_absolute() && absolute.ends_with("a/c.tmx"),
            "{}",
            absolute.display()
        );
    }

    #[test]
    fn the_upward_search_stops_at_the_repository_top_and_below_home() {
        let scratch = Scratch::new("bt-utils-search-up").unwrap();
        let home = scratch.join("home");
        let repository = home.join("games/new");
        fs_tree(&[&repository.join(".git"), &repository.join("maps/caves")]);
        std::fs::write(home.join("marker"), "").unwrap();
        std::fs::write(home.join("games/marker"), "").unwrap();
        let marked = |dir: &Path| dir.join("marker").is_file().then(|| dir.to_path_buf());
        let start = repository.join("maps/caves");
        assert_eq!(
            search_up(&start, Some(&home), marked),
            None,
            "nothing above the repository"
        );
        std::fs::write(repository.join("marker"), "").unwrap();
        assert_eq!(
            search_up(&start, Some(&home), marked),
            Some(repository.clone()),
            "the repository's own"
        );

        std::fs::remove_dir_all(repository.join(".git")).unwrap();
        std::fs::remove_file(repository.join("marker")).unwrap();
        assert_eq!(
            search_up(&start, Some(&home), marked),
            Some(home.join("games")),
            "no repository: up to home"
        );
        std::fs::remove_file(home.join("games/marker")).unwrap();
        assert_eq!(
            search_up(&start, Some(&home), marked),
            None,
            "home itself is not searched"
        );
        let mut visited = Vec::new();
        let _: Option<()> = search_up(&start, Some(&home), |dir| {
            visited.push(dir.to_path_buf());
            None
        });
        assert_eq!(
            visited,
            [
                start.clone(),
                repository.join("maps"),
                repository.clone(),
                home.join("games")
            ]
        );
    }

    fn fs_tree(dirs: &[&Path]) {
        for dir in dirs {
            std::fs::create_dir_all(dir).unwrap();
        }
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    #[cfg(unix)]
    fn owner(path: &Path) -> u32 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).unwrap().uid()
    }

    #[cfg(unix)]
    #[test]
    fn the_runtime_folder_is_named_after_the_user_in_tmp() {
        assert_eq!(runtime_dir_of(501), PathBuf::from("/tmp/better-tiled-501"));
    }

    #[cfg(unix)]
    #[test]
    fn a_private_folder_is_made_with_mode_0700_and_a_wider_mode_is_narrowed() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("bt-utils-private-dir").unwrap();
        let uid = owner(scratch.path());
        let dir = scratch.join("runtime");
        make_private_dir_of(&dir, uid).unwrap();
        assert_eq!(mode(&dir), 0o700, "made private");
        std::fs::write(dir.join("editor.sock"), "").unwrap();
        make_private_dir_of(&dir, uid).unwrap();
        assert!(
            dir.join("editor.sock").is_file(),
            "an existing folder is kept"
        );
        for wider in [0o755, 0o711, 0o701, 0o770, 0o777] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(wider)).unwrap();
            make_private_dir_of(&dir, uid).unwrap();
            assert_eq!(mode(&dir), 0o700, "{wider:o} narrowed");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_current_user_makes_private_folders_of_their_own() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("bt-utils-private-dir-own").unwrap();
        let dir = scratch.join("live");
        make_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700, "made private");
        assert_eq!(owner(&dir), owner(scratch.path()));
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        make_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700, "narrowed");
        let file = scratch.join("file");
        std::fs::write(&file, "").unwrap();
        assert!(matches!(
            make_private_dir(&file),
            Err(Error::NotPrivate {
                reason: NotPrivate::NotAFolder,
                ..
            })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn links_files_and_other_users_folders_are_never_used() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("bt-utils-private-dir-refused").unwrap();
        let uid = owner(scratch.path());
        let real = scratch.join("real");
        make_private_dir_of(&real, uid).unwrap();
        let refusal = |dir: &Path, uid: u32| match make_private_dir_of(dir, uid) {
            Err(Error::NotPrivate { reason, .. }) => reason,
            other => panic!("{} was not refused: {other:?}", dir.display()),
        };

        let link = scratch.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(refusal(&link, uid), NotPrivate::SymbolicLink);
        let dangling = scratch.join("dangling");
        std::os::unix::fs::symlink(scratch.join("nowhere"), &dangling).unwrap();
        assert_eq!(refusal(&dangling, uid), NotPrivate::SymbolicLink);
        assert!(!scratch.join("nowhere").exists(), "nothing made through it");

        let file = scratch.join("file");
        std::fs::write(&file, "").unwrap();
        assert_eq!(refusal(&file, uid), NotPrivate::NotAFolder);

        // Another user's folder is refused as it is, mode and all.
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
        let someone_else = uid.wrapping_add(1);
        assert_eq!(
            refusal(&real, someone_else),
            NotPrivate::OwnedByOther { owner: uid }
        );
        assert_eq!(mode(&real), 0o755, "left alone");
        let error = make_private_dir_of(&real, someone_else).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("{} is not used: it belongs to user {uid}", real.display())
        );
        assert_eq!(error.refused(), None);
    }
}
