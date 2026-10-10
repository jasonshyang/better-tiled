//! The name a duplicate takes (Tiled's `Editor::nameOfDuplicate`,
//! `reference/tiled/src/tiled/editor.cpp`): objects, layers and terrain sets share it.

/// The name of a copy of something called `name`: "Copy" appended, or the number after an
/// existing "Copy" counted up ("a" → "a Copy" → "a Copy 2" → "a Copy 3"). An empty name stays
/// empty. Tiled's rule is the pattern `(.*)\s*Copy\s*(\d+)?$`.
pub(crate) fn name_of_duplicate(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    let without_number = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let number = name.strip_prefix(without_number).unwrap_or_default();
    let Some(before) = without_number
        .trim_end_matches(|c: char| c.is_ascii_whitespace())
        .strip_suffix("Copy")
    else {
        return format!("{name} Copy");
    };
    // Tiled counts on from the number it read, and starts again at 1 from one too large to
    // read; no number is the first copy, so the next is the second.
    let next = if number.is_empty() {
        2
    } else {
        number.parse::<u64>().map_or(1, |n| n.saturating_add(1))
    };
    match before.trim() {
        "" => format!("Copy {next}"),
        stem => format!("{stem} Copy {next}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_count_up_as_tileds_do() {
        assert_eq!(name_of_duplicate(""), "");
        assert_eq!(name_of_duplicate("tree"), "tree Copy");
        assert_eq!(name_of_duplicate("tree Copy"), "tree Copy 2");
        assert_eq!(name_of_duplicate("tree Copy 2"), "tree Copy 3");
        assert_eq!(name_of_duplicate("tree Copy 007"), "tree Copy 8");
        assert_eq!(name_of_duplicate("tree Copy9"), "tree Copy 10");
        assert_eq!(
            name_of_duplicate("treeCopy "),
            "tree Copy 2",
            "spacing around Copy is loose"
        );
        assert_eq!(name_of_duplicate("Copy"), "Copy 2");
        assert_eq!(
            name_of_duplicate("Copy 1 2"),
            "Copy 1 2 Copy",
            "only a number right after Copy counts"
        );
        assert_eq!(name_of_duplicate("Copycat"), "Copycat Copy");
        assert_eq!(
            name_of_duplicate("tree Copy 99999999999999999999999"),
            "tree Copy 1"
        );
    }
}
