//! A folder of its own for each test.

use std::path::{Path, PathBuf};

/// An empty folder for one test, in the system's temporary folder, removed when dropped. Each test
/// gives a name of its own; the process id keeps parallel runs apart.
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    /// A new, empty folder named after `name` and this process (a leftover of an earlier run of
    /// the same process id is removed first).
    pub fn new(name: &str) -> std::io::Result<Scratch> {
        let dir = std::env::temp_dir()
            .join("better-tiled-tests")
            .join(format!("{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        // Canonical, so that paths compare equal however the system's temporary folder is reached.
        Ok(Scratch {
            dir: std::fs::canonicalize(&dir)?,
        })
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// A path inside the folder.
    pub fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.dir.join(relative)
    }

    /// A copy of the folder `from` named `name` inside this one, for a test that writes files: the
    /// files a test may write (Tiled's maps, tilesets, templates, worlds and projects) are copied,
    /// and the rest (images) are hard-linked when the file system allows, so a large pack is
    /// mirrored at once and its own files are never written.
    pub fn mirror(&self, from: &Path, name: &str) -> std::io::Result<PathBuf> {
        let to = self.dir.join(name);
        mirror_folder(from, &to)?;
        Ok(to)
    }
}

/// The extensions of the files [`Scratch::mirror`] copies rather than links.
const WRITABLE: [&str; 9] = [
    "tmx",
    "tsx",
    "tx",
    "tmj",
    "tsj",
    "tj",
    "json",
    "world",
    "tiled-project",
];

fn mirror_folder(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            mirror_folder(&source, &target)?;
            continue;
        }
        let writable = source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| WRITABLE.contains(&e.to_ascii_lowercase().as_str()));
        if writable || std::fs::hard_link(&source, &target).is_err() {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A folder that can't be removed is left for the system to clean; it fails no test.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scratch_folder_starts_empty_and_goes_away() {
        let path;
        {
            let scratch = Scratch::new("bt-test-data-scratch").unwrap();
            path = scratch.path().to_path_buf();
            std::fs::write(scratch.join("a.txt"), "a").unwrap();
            assert!(scratch.join("a.txt").is_file());
        }
        assert!(!path.exists());
        let again = Scratch::new("bt-test-data-scratch").unwrap();
        assert_eq!(std::fs::read_dir(again.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_mirror_copies_what_tests_write_and_links_the_rest() {
        let scratch = Scratch::new("bt-test-data-mirror").unwrap();
        let pack = scratch.join("pack");
        std::fs::create_dir_all(pack.join("maps")).unwrap();
        std::fs::write(pack.join("maps/a.tmx"), "<map/>").unwrap();
        std::fs::write(pack.join("tiles.png"), "png").unwrap();
        let copy = scratch.mirror(&pack, "copy").unwrap();
        // Writing the mirrored map leaves the original alone.
        std::fs::write(copy.join("maps/a.tmx"), "changed").unwrap();
        assert_eq!(
            std::fs::read_to_string(pack.join("maps/a.tmx")).unwrap(),
            "<map/>"
        );
        assert_eq!(
            std::fs::read_to_string(copy.join("tiles.png")).unwrap(),
            "png"
        );
    }
}
