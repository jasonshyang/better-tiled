//! Paths into custom properties: a property's name, then members of class values and items of
//! lists (Tiled's `PropertyPath`, `reference/tiled/src/libtiled/properties.h`).

use std::fmt;

/// One step of a path into custom properties. A path starts with the property's name as a
/// member step, as Tiled's paths do.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    /// A property (the first step), or a member of a class value.
    Member(String),
    /// An item of a list, counted from 0.
    Item(usize),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Member(name) => f.write_str(name),
            Step::Item(index) => write!(f, "[{index}]"),
        }
    }
}

/// A path as Tiled names it in its undo commands: `loot.gold`, `zones[2].name`.
pub(crate) fn path_text(path: &[Step]) -> String {
    let mut text = String::new();
    for step in path {
        if matches!(step, Step::Member(_)) && !text.is_empty() {
            text.push('.');
        }
        text.push_str(&step.to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_read_as_tiled_writes_them() {
        let path = [
            Step::Member("zones".into()),
            Step::Item(2),
            Step::Member("name".into()),
        ];
        assert_eq!(path_text(&path), "zones[2].name");
        assert_eq!(path_text(&[Step::Member("hp".into())]), "hp");
        assert_eq!(path_text(&[]), "");
    }
}
