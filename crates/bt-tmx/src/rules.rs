//! AutoMapping rule lists (`rules.txt`): rule maps, map name filters and nested lists.
//!
//! One entry a line, trimmed: empty lines and lines starting with `#` or `//` are ignored;
//! `[pattern]` restricts the entries after it to maps whose file name matches the
//! case-insensitive wildcard, and `[*]` lifts it; a path relative to the list names a rule map,
//! or, ending in `.txt`, a list read in place with the current filter, whose own filter changes
//! stay inside it.

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::loader::absolute;
use crate::paths::resolve_reference;
use crate::text::read_text;

/// A rule map a rule list names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleMapEntry {
    /// The rule map.
    pub path: PathBuf,
    /// The wildcard the maps it applies to match; `None` for all.
    pub filter: Option<String>,
}

impl RuleMapEntry {
    /// Whether the rule map applies to a map with this file name (`town1.tmx`; `""` for a map
    /// not saved yet).
    pub fn applies_to(&self, map_file_name: &str) -> bool {
        self.filter
            .as_deref()
            .is_none_or(|pattern| wildcard_match(pattern, map_file_name))
    }
}

/// Reads a rule list and the lists it includes; include cycles are skipped. Fails when a list
/// cannot be read; rule maps are not checked here.
pub fn parse_rules_file(path: &Path) -> Result<Vec<RuleMapEntry>, Error> {
    let mut entries = Vec::new();
    read_list(&absolute(path)?, None, &mut Vec::new(), &mut entries)?;
    Ok(entries)
}

fn read_list(
    path: &Path,
    mut filter: Option<String>,
    reading: &mut Vec<PathBuf>,
    entries: &mut Vec<RuleMapEntry>,
) -> Result<(), Error> {
    let identity = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if reading.contains(&identity) {
        return Ok(());
    }
    let text = read_text(path)?;
    let dir = path.parent().unwrap_or(Path::new(""));
    reading.push(identity);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if let Some(pattern) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let pattern = pattern.trim();
            filter = (pattern != "*").then(|| pattern.to_owned());
            continue;
        }
        let target = resolve_reference(dir, line);
        if target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
        {
            read_list(&target, filter.clone(), reading, entries)?;
        } else {
            entries.push(RuleMapEntry {
                path: target,
                filter: filter.clone(),
            });
        }
    }
    reading.pop();
    Ok(())
}

/// Case-insensitive wildcard matching, anchored at both ends: `*` any run, `?` any character,
/// `[…]` a set with ranges, negated by a leading `!` or `^`.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    matches_from(&pattern, &name)
}

fn matches_from(pattern: &[char], name: &[char]) -> bool {
    let Some((&first, rest)) = pattern.split_first() else {
        return name.is_empty();
    };
    let after_one = name.get(1..).unwrap_or(&[]);
    match first {
        '*' => (0..=name.len()).any(|skip| matches_from(rest, name.get(skip..).unwrap_or(&[]))),
        '?' => !name.is_empty() && matches_from(rest, after_one),
        '[' => match CharSet::parse(rest) {
            Some(parsed) => name
                .first()
                .is_some_and(|&c| parsed.set.matches(c) && matches_from(parsed.rest, after_one)),
            None => name.first() == Some(&'[') && matches_from(rest, after_one),
        },
        c => name.first() == Some(&c) && matches_from(rest, after_one),
    }
}

/// A `[…]` set.
struct CharSet {
    negated: bool,
    ranges: Vec<CharRange>,
}

/// A range of a set; one character is a range of one.
struct CharRange {
    first: char,
    last: char,
}

/// A parsed set and the pattern after it.
struct ParsedSet<'p> {
    set: CharSet,
    rest: &'p [char],
}

impl CharSet {
    /// Parses the inside of `[…]` (after the `[`); `None` without a closing `]`.
    fn parse(pattern: &[char]) -> Option<ParsedSet<'_>> {
        let (negated, mut rest) = match pattern.split_first() {
            Some(('!' | '^', rest)) => (true, rest),
            _ => (false, pattern),
        };
        let mut ranges = Vec::new();
        let mut first_item = true;
        loop {
            let (&c, after) = rest.split_first()?;
            if c == ']' && !first_item {
                return Some(ParsedSet {
                    set: CharSet { negated, ranges },
                    rest: after,
                });
            }
            first_item = false;
            match after {
                ['-', last, tail @ ..] if *last != ']' => {
                    ranges.push(CharRange {
                        first: c,
                        last: *last,
                    });
                    rest = tail;
                }
                _ => {
                    ranges.push(CharRange { first: c, last: c });
                    rest = after;
                }
            }
        }
    }

    fn matches(&self, c: char) -> bool {
        self.ranges.iter().any(|r| r.first <= c && c <= r.last) != self.negated
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::Scratch;

    use super::*;

    #[test]
    fn wildcards_match_whole_names() {
        assert!(wildcard_match("*.tmx", "Town1.TMX"));
        assert!(wildcard_match("town?.tmx", "town1.tmx"));
        assert!(!wildcard_match("town?.tmx", "town12.tmx"));
        assert!(wildcard_match("town[0-4].tmx", "town3.tmx"));
        assert!(!wildcard_match("town[!0-4].tmx", "town3.tmx"));
        assert!(wildcard_match("[]].tmx", "].tmx"));
        assert!(wildcard_match("a[b", "a[b"));
        assert!(!wildcard_match("town", "town1"));
    }

    #[test]
    fn lists_nest_with_their_filters() {
        let scratch = Scratch::new("bt-tmx-rules").unwrap();
        std::fs::write(
            scratch.join("rules.txt"),
            "# comment\n// comment\nbase.tmx\n[town*]\nsub/rules.txt\ntown.tmx\n[*]\nall.tmx\nrules.txt\n",
        )
        .unwrap();
        std::fs::create_dir_all(scratch.join("sub")).unwrap();
        std::fs::write(
            scratch.join("sub/rules.txt"),
            "  inner.tmx  \n[cave*]\ncave.tmx\n",
        )
        .unwrap();
        let entries = parse_rules_file(&scratch.join("rules.txt")).unwrap();
        let shown: Vec<(String, Option<&str>)> = entries
            .iter()
            .map(|e| {
                (
                    e.path
                        .strip_prefix(scratch.path())
                        .unwrap()
                        .display()
                        .to_string(),
                    e.filter.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("base.tmx".to_owned(), None),
                ("sub/inner.tmx".to_owned(), Some("town*")),
                ("sub/cave.tmx".to_owned(), Some("cave*")),
                ("town.tmx".to_owned(), Some("town*")),
                ("all.tmx".to_owned(), None),
            ]
        );
        assert!(entries[1].applies_to("Town2.tmx") && !entries[1].applies_to("cave.tmx"));
        assert!(parse_rules_file(&scratch.join("missing.txt")).is_err());
    }
}
