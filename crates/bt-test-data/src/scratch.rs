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
}
