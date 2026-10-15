//! Options controlling the regeneration contract.

/// How an export goes (the regeneration contract, docs/design/12-godot-export.md).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Replace generated files that changed since the export wrote them (or that it didn't
    /// write), keeping each as a `.bak` first (`--replace`).
    pub replace: bool,
    /// Write nothing; the report says what an export would write (`--check`).
    pub check: bool,
    /// Give the scene a uid of its own, derived from its path salted, when the one it would get
    /// is another file's (`--new-uid`).
    pub new_uid: bool,
}

impl Options {
    /// Starts with safe regeneration and writing enabled.
    pub fn builder() -> Self {
        Self::default()
    }
    /// Allows replacing modified generated files while retaining backups.
    pub fn replace(mut self, yes: bool) -> Self {
        self.replace = yes;
        self
    }
    /// Plans and checks without writing any output.
    pub fn check(mut self, yes: bool) -> Self {
        self.check = yes;
        self
    }
    /// Derives a new uid when the path-derived uid is already taken.
    pub fn new_uid(mut self, yes: bool) -> Self {
        self.new_uid = yes;
        self
    }
}
