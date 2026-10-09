//! Text helpers shared by every message and name lookup: counts with their nouns, lists in prose,
//! the lowercase hyphenated names people and agents type, the one fuzzy match all name lookups
//! use, and natural ordering.

use std::cmp::Ordering;

/// `n` with the noun that fits it: `plural(1, "cell", "cells")` is `"1 cell"`.
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The items in prose: `"a"`, `"a and b"`, `"a, b and c"`; empty for none.
pub fn and_list<S: AsRef<str>>(items: &[S]) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_owned(),
        [init @ .., last] => {
            let init: Vec<&str> = init.iter().map(AsRef::as_ref).collect();
            format!("{} and {}", init.join(", "), last.as_ref())
        }
    }
}

/// The items quoted, in prose: `“a”, “b” and “c”`.
pub fn quoted_list<S: AsRef<str>>(items: &[S]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|item| format!("“{}”", item.as_ref()))
        .collect();
    and_list(&quoted)
}

/// The name people and agents type for `text`: lowercase letters and digits, with every other
/// run of characters one `-`, and none at the ends. `"Pine Tree_big"` gives `"pine-tree-big"`.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

/// Up to three of `candidates` closest to `query`, closest first: those containing it (or
/// contained in it) first, then those within an edit distance of a third of its length (at least
/// two). Candidates are compared after their last `/`, so a qualified name `set/water` matches
/// `water`. The query is compared as its [`slug`].
pub fn fuzzy_match<'c>(query: &str, candidates: &[&'c str]) -> Vec<&'c str> {
    const SHOWN: usize = 3;
    let wanted = slug(query);
    if wanted.is_empty() {
        return Vec::new();
    }
    let allowed = (wanted.chars().count() / 3).max(2);
    let mut scored: Vec<Scored<'c>> = candidates
        .iter()
        .filter_map(|&candidate| {
            let tail = candidate.rsplit('/').next().unwrap_or(candidate);
            if tail.contains(&wanted) || wanted.contains(tail) {
                return Some(Scored {
                    distance: 0,
                    candidate,
                });
            }
            let distance = edit_distance(&wanted, tail);
            (distance <= allowed).then_some(Scored {
                distance,
                candidate,
            })
        })
        .collect();
    scored.sort_by_key(|s| (s.distance, s.candidate.len()));
    let mut out: Vec<&'c str> = Vec::new();
    for s in scored {
        if !out.contains(&s.candidate) {
            out.push(s.candidate);
        }
        if out.len() == SHOWN {
            break;
        }
    }
    out
}

/// A candidate with its distance from the query.
struct Scored<'c> {
    distance: usize,
    candidate: &'c str,
}

/// The Levenshtein distance between two strings, in characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = Vec::with_capacity(b.len() + 1);
        current.push(i + 1);
        for (j, &cb) in b.iter().enumerate() {
            let substitution = previous.get(j).map(|d| d + usize::from(ca != cb));
            let deletion = previous.get(j + 1).map(|d| d + 1);
            let insertion = current.last().map(|d| d + 1);
            let best = [substitution, deletion, insertion]
                .into_iter()
                .flatten()
                .min();
            current.push(best.unwrap_or(i + 1));
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(0)
}

/// Orders names as people count: runs of digits by their value (`d3` before `d20`), letters
/// without regard to case, and, between equal values, fewer leading zeros first.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x, y) = (digits(&mut a), digits(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx
                    .len()
                    .cmp(&ty.len())
                    .then_with(|| tx.cmp(ty))
                    .then_with(|| x.len().cmp(&y.len()));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The run of digits at the start of `chars`, taken from it.
fn digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut out = String::new();
    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_lists_read_as_prose() {
        assert_eq!(plural(1, "cell", "cells"), "1 cell");
        assert_eq!(plural(0, "cell", "cells"), "0 cells");
        assert_eq!(and_list::<&str>(&[]), "");
        assert_eq!(and_list(&["a"]), "a");
        assert_eq!(and_list(&["a", "b"]), "a and b");
        assert_eq!(and_list(&["a", "b", "c"]), "a, b and c");
        assert_eq!(quoted_list(&["walls", "ground"]), "“walls” and “ground”");
    }

    #[test]
    fn slugs_are_lowercase_and_hyphenated() {
        assert_eq!(slug("platformV2-3tiles tall"), "platformv2-3tiles-tall");
        assert_eq!(slug("  Terrain - Highlands "), "terrain-highlands");
        assert_eq!(slug("pine-tree2-no leaves_0"), "pine-tree2-no-leaves-0");
        assert_eq!(slug("État Ü"), "état-ü");
        assert_eq!(slug("---"), "");
    }

    #[test]
    fn fuzzy_matches_prefer_containment_then_distance() {
        let names = [
            "frozen-lake",
            "snow-ground",
            "terrain1",
            "terrain2",
            "set/water",
        ];
        assert_eq!(fuzzy_match("frozen lak", &names), ["frozen-lake"]);
        assert_eq!(fuzzy_match("lake", &names), ["frozen-lake"]);
        assert_eq!(fuzzy_match("terain1", &names).first(), Some(&"terrain1"));
        assert_eq!(fuzzy_match("Water", &names), ["set/water"]);
        assert!(fuzzy_match("xyz", &names).is_empty());
        assert!(fuzzy_match("--", &names).is_empty());
        assert_eq!(
            fuzzy_match("terrain", &["terrain1", "terrain2", "terrain3", "terrain4"]).len(),
            3
        );
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn natural_order_counts_numbers() {
        let mut names = vec!["d20", "D3", "d100", "a", "d03", "b2c10", "b2c9"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["a", "b2c9", "b2c10", "D3", "d03", "d20", "d100"]);
    }
}
