//! Path helpers: lexical cleaning, absolute and relative paths, file stems and labels, and the
//! upward search that finds a project file above a map.

use std::path::{Component, Path, PathBuf};

use crate::error::Error;

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
}
