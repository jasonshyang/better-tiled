//! What a script runs on (`10-agents.md`, §8; `11-live-link.md`, §9.2): the map as painted and
//! where its file is, the script's seed, what the editor has selected, the map's AutoMap layers,
//! the definitions of a `--with` file, rules the caller has read for the map already, and whether
//! the report shows the changed area as text. `bt` builds it from a file, the editor from the
//! base of an external edit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bt_automap::RuleSet;
use bt_automap::layers::Setup;
use bt_core::Map;

use crate::context::Selection;
use crate::script::Script;
use crate::view::ViewBy;

/// The seed a script draws from unless the caller gives one.
pub const DEFAULT_SEED: u64 = 1;

/// What a script runs on. The map is never changed: the script runs on a copy.
#[derive(Clone, Debug)]
pub struct RunInput {
    pub(crate) map: Arc<Map>,
    pub(crate) path: PathBuf,
    pub(crate) seed: u64,
    pub(crate) selection: Option<Selection>,
    pub(crate) automap: Option<AutoMapInput>,
    pub(crate) definitions: Option<Script>,
    pub(crate) rules_read: Option<RulesRead>,
    pub(crate) view: Option<ChangedView>,
}

/// A map's AutoMap layers going into a script.
#[derive(Clone, Debug)]
pub(crate) struct AutoMapInput {
    /// Their setup.
    pub(crate) setup: Setup,
    /// The rules they run, read from the setup's rules file.
    pub(crate) rules: Arc<RuleSet>,
    /// The map as it shows now: what the rules made of what was painted.
    pub(crate) shown: Arc<Map>,
}

/// The changed area as text in the report (`--view`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChangedView {
    /// What a character stands for; the view's own choice when none.
    pub(crate) by: Option<ViewBy>,
    /// Each row's runs after it.
    pub(crate) runs: bool,
}

/// Rules the caller has read for the map, and the file they came from.
#[derive(Clone, Debug)]
pub(crate) struct RulesRead {
    pub(crate) path: PathBuf,
    pub(crate) rules: Arc<RuleSet>,
}

impl RunInput {
    /// A script on `map`, the map as painted, whose file is `path`: maps a script names and the
    /// `rules.txt` beside the map are found from its folder. The seed is [`DEFAULT_SEED`] and
    /// the map is not open in the editor until the builder says otherwise.
    pub fn builder(map: Arc<Map>, path: impl Into<PathBuf>) -> RunInputBuilder {
        RunInputBuilder {
            input: RunInput {
                map,
                path: path.into(),
                seed: DEFAULT_SEED,
                selection: None,
                automap: None,
                definitions: None,
                rules_read: None,
                view: None,
            },
        }
    }

    /// The map as painted.
    pub fn map(&self) -> &Arc<Map> {
        &self.map
    }

    /// The map's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The script's seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

/// Sets up a [`RunInput`].
#[derive(Clone, Debug)]
pub struct RunInputBuilder {
    input: RunInput,
}

impl RunInputBuilder {
    /// The seed every operation's randomness is drawn from, with the operation's ordinal
    /// (`--seed N`).
    pub fn seed(mut self, seed: u64) -> RunInputBuilder {
        self.input.seed = seed;
        self
    }

    /// The map is open in the editor with this selected: `selection` and `selected` stand for
    /// it.
    pub fn selection(mut self, selection: Selection) -> RunInputBuilder {
        self.input.selection = Some(selection);
        self
    }

    /// The map has AutoMap layers: their `setup`, the `rules` read from its rules file, and the
    /// map as it shows now, `shown` (what the rules make of what was painted, as its file holds
    /// it). The script edits what was painted and the result is what the rules make of it.
    pub fn automap_layers(
        mut self,
        setup: Setup,
        rules: Arc<RuleSet>,
        shown: Arc<Map>,
    ) -> RunInputBuilder {
        self.input.automap = Some(AutoMapInput {
            setup,
            rules,
            shown,
        });
        self
    }

    /// The definitions of a `--with` file, run before the script.
    pub fn definitions(mut self, definitions: Script) -> RunInputBuilder {
        self.input.definitions = Some(definitions);
        self
    }

    /// Rules the caller has read for the map from the file `path` (the editor has them): the
    /// `automap` step uses them when it would read that file, instead of reading it again.
    pub fn rules_read(mut self, path: impl Into<PathBuf>, rules: Arc<RuleSet>) -> RunInputBuilder {
        self.input.rules_read = Some(RulesRead {
            path: path.into(),
            rules,
        });
        self
    }

    /// The report shows the changed area as text (`--view`): `by` what a character stands for
    /// (the view's own choice when none), with each row's runs when `runs`.
    pub fn view(mut self, by: Option<ViewBy>, runs: bool) -> RunInputBuilder {
        self.input.view = Some(ChangedView { by, runs });
        self
    }

    /// The input.
    pub fn build(self) -> RunInput {
        self.input
    }
}
