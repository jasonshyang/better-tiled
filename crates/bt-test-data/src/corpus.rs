//! Where the git-ignored corpora are: in this checkout, or, from a git worktree, in the main
//! checkout, so that worktrees share one copy.

use std::path::{Path, PathBuf};

use crate::skip::missing;

/// The root of this checkout of the workspace.
pub fn workspace_root() -> PathBuf {
    // The manifest directory is `<root>/crates/bt-test-data`.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// `relative` (such as `"reference/tiled/examples"`) in this checkout, or in the main checkout
/// when this is a git worktree. When it is in neither, the calling test fails, unless missing
/// data is skipped ([`crate::SKIP_VAR`]): then this returns `None` and the test should return.
pub fn test_data(relative: &str) -> Option<PathBuf> {
    let root = workspace_root();
    let found = std::iter::once(root.clone())
        .chain(main_checkout(&root))
        .map(|checkout| checkout.join(relative))
        .find(|path| path.exists());
    if found.is_none() {
        let hint = if relative.starts_with("reference/") {
            "run scripts/fetch-reference.sh".to_owned()
        } else {
            format!("it is git-ignored: copy or link it into {}", shown(&root))
        };
        missing(relative, &hint);
    }
    found
}

/// The main checkout when `root` is a git worktree, whose `.git` is a file pointing into
/// `<main checkout>/.git/worktrees/<name>`.
fn main_checkout(root: &Path) -> Option<PathBuf> {
    let link = std::fs::read_to_string(root.join(".git")).ok()?;
    let git_dir = root.join(link.strip_prefix("gitdir:")?.trim());
    let dot_git = git_dir.parent()?.parent()?;
    if dot_git.file_name()? != ".git" {
        return None;
    }
    dot_git.parent().map(Path::to_path_buf)
}

/// A path as people should read it: canonical when it exists.
fn shown(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scratch;

    #[test]
    fn a_worktree_finds_its_main_checkout() {
        let scratch = Scratch::new("bt-test-data-worktree").unwrap();
        let main = scratch.join("main");
        let worktree = main.join(".claude/worktrees/agent");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(main.join(".git/worktrees/agent")).unwrap();
        let link = format!("gitdir: {}\n", main.join(".git/worktrees/agent").display());
        std::fs::write(worktree.join(".git"), link).unwrap();
        assert_eq!(main_checkout(&worktree), Some(main.clone()));
        assert_eq!(
            main_checkout(&main),
            None,
            "the main checkout has a .git folder"
        );
    }

    #[test]
    fn files_of_this_checkout_are_found() {
        let found = test_data("Cargo.toml").unwrap();
        assert_eq!(
            std::fs::canonicalize(found).unwrap(),
            std::fs::canonicalize(workspace_root().join("Cargo.toml")).unwrap()
        );
    }

    #[test]
    fn missing_data_fails_the_test_and_names_it() {
        if crate::skipping() {
            return;
        }
        let failure = std::panic::catch_unwind(|| test_data("reference/not-there")).unwrap_err();
        let message = failure.downcast::<String>().unwrap();
        assert!(
            message.contains("reference/not-there") && message.contains("fetch-reference"),
            "{message}"
        );
    }
}
