//! What keeps rules from being loaded at all.

use std::path::PathBuf;

/// A failure to load rules: a rules file that cannot be read, or a rule map it lists that does
/// not exist (both stop Tiled from running any rule). A rule map that exists but cannot be read
/// is not an error: the rule set says so with a problem and goes on without it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The rules file (or one it includes) cannot be read.
    #[error("the rules file {} cannot be read: {source}", path.display())]
    RulesFile {
        /// The rules file.
        path: PathBuf,
        /// Why.
        #[source]
        source: bt_tmx::Error,
    },
    /// A rule map the rules file (or one it includes) lists does not exist.
    #[error(
        "the rule map {} does not exist (listed through {})",
        path.display(),
        rules_file.display()
    )]
    MissingRuleMap {
        /// The rule map.
        path: PathBuf,
        /// The rules file the run loaded.
        rules_file: PathBuf,
    },
}
