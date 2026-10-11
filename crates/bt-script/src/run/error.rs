//! Why a script stopped (`10-agents.md`, §8): the line, the operation as written and what is
//! wrong. Nothing a failed script did is kept: the caller's map is as it was.

use bt_edit::EditError;
use bt_terrain::TerrainError;

use crate::error::Error;

/// The most characters of an operation an error quotes.
const OPERATION_SHOWN: usize = 60;

/// A script that stopped at a line: where, what the line says, and what is wrong.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{}", self.text())]
pub struct RunError {
    /// The line, from 1.
    pub line: usize,
    /// The operation as written.
    pub operation: String,
    /// Whether the line is one of the `--with` file's rather than the script's.
    pub in_definitions: bool,
    /// What is wrong (boxed: an error travels up through every operation).
    pub failure: Box<RunFailure>,
}

impl RunError {
    /// "line 3 (paint lava all): unknown terrain “lava” …".
    fn text(&self) -> String {
        let mut operation: String = self.operation.chars().take(OPERATION_SHOWN).collect();
        if self.operation.chars().count() > OPERATION_SHOWN {
            operation.push('…');
        }
        let place = if self.in_definitions {
            format!("--with file, line {}", self.line)
        } else {
            format!("line {}", self.line)
        };
        format!("{place} ({operation}): {}", self.failure)
    }
}

/// What is wrong with an operation.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RunFailure {
    /// A name, a shape or a foot.
    #[error(transparent)]
    Name(#[from] Error),
    /// The operation would change a locked layer.
    #[error("{}", locked_text(.layer, .group.as_deref()))]
    Locked {
        /// The layer.
        layer: String,
        /// The locked group it is in, when the lock is the group's.
        group: Option<String>,
    },
    /// No rule of §8.2 says which layer, and the script did not.
    #[error("{}", say_which_text(.what, *.objects, .layers))]
    SayWhichLayer {
        /// What goes on the layer: a terrain's name, `the tiles`, `the sprite`, `the spawn`.
        what: String,
        /// Whether it needs an object layer rather than a tile layer.
        objects: bool,
        /// The map's layers of that kind, by name.
        layers: Vec<String>,
    },
    /// An object of a map with AutoMap layers that the rules placed, which only changes when
    /// what the rules read does.
    #[error(
        "object #{id} was placed by the map's AutoMap rules, not painted: bt edit changes what was \
         painted (change what the rules read, or stop them: automap layers off)"
    )]
    RuleMade {
        /// The object.
        id: u32,
    },
    /// Rules that could not be read.
    #[error("the rules can't be read: {0}")]
    Rules(String),
    /// A map a script names that could not be read.
    #[error("{path} can't be read: {reason}")]
    ReadMap {
        /// The map, as the script resolved it.
        path: String,
        /// Why.
        reason: String,
    },
    /// An editing operation refused.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// A terrain fill refused.
    #[error(transparent)]
    Terrain(#[from] TerrainError),
    /// Anything else, said as it is.
    #[error("{0}")]
    Refused(String),
}

impl RunFailure {
    /// A refusal in its own words.
    pub(crate) fn refused(text: impl Into<String>) -> RunFailure {
        RunFailure::Refused(text.into())
    }
}

impl From<bt_core::Error> for RunFailure {
    fn from(error: bt_core::Error) -> RunFailure {
        RunFailure::Edit(EditError::Model(error))
    }
}

/// "ice is locked (unlock it first: layer unlock ice)".
fn locked_text(layer: &str, group: Option<&str>) -> String {
    let quoted = crate::words::quoted_word;
    match group {
        Some(group) => format!(
            "{layer} is in the locked group {group} (unlock it first: layer unlock {})",
            quoted(group)
        ),
        None => format!(
            "{layer} is locked (unlock it first: layer unlock {})",
            quoted(layer)
        ),
    }
}

/// "say which layer to put water on: add `layer L` (tile layers: ground, ice)".
fn say_which_text(what: &str, objects: bool, layers: &[String]) -> String {
    let kind = if objects { "object" } else { "tile" };
    let known = if layers.is_empty() {
        format!("the map has no {kind} layers yet: add one with layer add NAME {kind}s")
    } else {
        format!("{kind} layers: {}", layers.join(", "))
    };
    format!("say which layer to put {what} on: add `layer L` ({known})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_names_its_line_its_operation_and_what_is_wrong() {
        let error = RunError {
            line: 3,
            operation: "paint ice 1,1 layer ice".into(),
            in_definitions: false,
            failure: Box::new(RunFailure::Locked {
                layer: "ice".into(),
                group: None,
            }),
        };
        assert_eq!(
            error.to_string(),
            "line 3 (paint ice 1,1 layer ice): ice is locked (unlock it first: layer unlock ice)"
        );
        let long = RunError {
            line: 1,
            operation: format!("paint water poly {}", "1,1 ".repeat(30)),
            in_definitions: true,
            failure: Box::new(RunFailure::SayWhichLayer {
                what: "water".into(),
                objects: false,
                layers: vec!["ground".into(), "ice".into()],
            }),
        };
        let text = long.to_string();
        assert!(
            text.starts_with("--with file, line 1 (paint water poly 1,1"),
            "{text}"
        );
        assert!(
            text.ends_with(
                "…): say which layer to put water on: add `layer L` (tile layers: ground, ice)"
            ),
            "{text}"
        );
        let none = RunFailure::SayWhichLayer {
            what: "the spawn".into(),
            objects: true,
            layers: Vec::new(),
        };
        assert_eq!(
            none.to_string(),
            "say which layer to put the spawn on: add `layer L` (the map has no object layers yet: \
             add one with layer add NAME objects)"
        );
    }
}
