//! What is wrong or odd in rule maps, rule lists and runs, typed so the editor can group the
//! problems and `bt` can print them as problem lines. Nothing is skipped quietly: every rule map
//! that is not used, every value that cannot be read and every output a lock kept out has one.

use std::fmt;

/// A problem: its kind, and a sentence saying what and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// What kind of problem it is.
    pub kind: ProblemKind,
    /// What happened, naming the layer, property, object or file concerned.
    pub text: String,
}

impl Problem {
    /// A problem of `kind` described by `text`.
    pub fn new(kind: ProblemKind, text: impl Into<String>) -> Problem {
        Problem {
            kind,
            text: text.into(),
        }
    }

    /// How serious it is.
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.severity(), self.text)
    }
}

/// How serious a problem is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    /// The rule map is not used.
    Error,
    /// Something is ignored or done otherwise than the rule map seems to ask.
    Warning,
    /// Worth knowing, not wrong: tiles that make no rule, a rule that can never match.
    Note,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        })
    }
}

/// The kinds of problem.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProblemKind {
    // Errors: the rule map is not used, as in Tiled.
    /// A layer name has no underscore, so it names no target layer (Tiled: "Did you forget an
    /// underscore?"). A `//` prefix makes a layer a note.
    LayerNameWithoutUnderscore,
    /// The rule map has no `input` or `inputnot` layer.
    NoInputLayers,
    /// The rule map has no `output` layer.
    NoOutputLayers,
    /// A `regions`, `regions_input` or `regions_output` layer occurs twice.
    RegionsLayerTwice,
    /// A `regions` layer is not a tile layer.
    RegionsLayerNotTiles,
    /// The `rule_options` layer is not an object layer.
    RuleOptionsNotObjects,
    /// An `input` or `inputnot` layer is not a tile layer.
    InputLayerNotTiles,

    // Warnings.
    /// A property that means nothing to AutoMap; Tiled ignores it too.
    UnknownProperty,
    /// An option whose value cannot be read as what the option needs; the option keeps its
    /// default.
    UnreadableOption,
    /// An option value Tiled reads in a way that may surprise: text read as `true`, a modulus
    /// of 0 read as 1.
    OddOptionValue,
    /// A layer whose name means nothing to AutoMap, such as `regionsX` or `foo_bar`.
    UnknownLayer,
    /// An object on the `rule_options` layer that is not a plain rectangle; it sets nothing.
    RuleOptionsNotRectangle,
    /// A rotated rectangle on the `rule_options` layer; it sets nothing.
    RuleOptionsRotated,
    /// Tiles of input or output layers outside the legacy `regions` layers, which belong to no
    /// rule.
    TilesOutsideRegions,
    /// A tile whose `MatchType` is none of Tiled's names (they are case-sensitive); it counts
    /// as an ordinary tile.
    UnknownMatchType,
    /// An output object whose cells cannot be worked out (a position or size too large or not
    /// a number); it is never placed.
    OutputObjectOutOfRange,
    /// A rule map listed in a rule list cannot be read; its previous rules are kept on a
    /// reload.
    UnreadableRuleMap,
    /// What the file reader reported about a rule map or a file it refers to (a missing
    /// tileset, say).
    FileWarning,
    /// A lock kept a rule's output out of a layer (`IgnoreLock` lets it in).
    LockedOutput,
    /// A run needed more tilesets than a map can hold; the tiles of those it could not add
    /// were not placed.
    TooManyTilesets,
    /// A run's layer names bind a layer that is gone, of the wrong kind, or bound twice; the
    /// run does nothing.
    BindingRefused,
    /// A tileset the rules use is a copy of one of the map's (the same image through another
    /// file), so rules naming its tiles never match the map's tiles.
    TilesetCopy,
    /// A rule map matched nothing in a run, and it reads a layer the map does not have (which
    /// no earlier rule map writes), so that layer read as empty.
    IdleMissingLayer,

    // Notes.
    /// A rules file includes one being read (an include cycle); the include is skipped.
    IncludeCycle,
    /// A rule map matched nothing in a run, and a layer it reads held no tile when it ran.
    IdleEmptyLayer,
    /// A rule map matched nothing in a run though its input layers held tiles.
    IdleNoMatch,
    /// Input tiles that touch no output tiles, so they make no rule.
    OrphanInput,
    /// Output tiles or objects that touch no input tiles, so they make no rule.
    OrphanOutput,
    /// A rule map without rules.
    NoRules,
    /// A rule that can never match: every one of its input sets contradicts itself or has no
    /// conditions, or it has no output to apply.
    RuleNeverMatches,
    /// A run asked for a region was run on the whole map, with the reason.
    FullRunFallback,
    /// What was painted on a layer the rules write could not be rebuilt from the project file
    /// (the layer changed elsewhere, or is gone): all of the layer counts as painted.
    PaintedNotRestored,
}

impl ProblemKind {
    /// How serious problems of this kind are.
    pub fn severity(self) -> Severity {
        use ProblemKind::*;
        match self {
            LayerNameWithoutUnderscore
            | NoInputLayers
            | NoOutputLayers
            | RegionsLayerTwice
            | RegionsLayerNotTiles
            | RuleOptionsNotObjects
            | InputLayerNotTiles => Severity::Error,
            UnknownProperty
            | UnreadableOption
            | OddOptionValue
            | UnknownLayer
            | RuleOptionsNotRectangle
            | RuleOptionsRotated
            | TilesOutsideRegions
            | UnknownMatchType
            | OutputObjectOutOfRange
            | UnreadableRuleMap
            | FileWarning
            | LockedOutput
            | TooManyTilesets
            | BindingRefused
            | TilesetCopy
            | IdleMissingLayer => Severity::Warning,
            IncludeCycle | IdleEmptyLayer | IdleNoMatch | OrphanInput | OrphanOutput | NoRules
            | RuleNeverMatches | FullRunFallback | PaintedNotRestored => Severity::Note,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_read_with_their_severity() {
        let problem = Problem::new(
            ProblemKind::LayerNameWithoutUnderscore,
            "layer “walls” has no underscore",
        );
        assert_eq!(problem.severity(), Severity::Error);
        assert_eq!(
            problem.to_string(),
            "error: layer “walls” has no underscore"
        );
        assert_eq!(ProblemKind::OrphanInput.severity(), Severity::Note);
        assert_eq!(ProblemKind::LockedOutput.severity(), Severity::Warning);
    }
}
