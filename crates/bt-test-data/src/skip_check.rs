//! Which recorded skips are allowed: continuous integration may skip only what it can never have
//! (the purchased packs), never what it should have fetched.

/// The lines of a skip log whose missing thing matches none of `allowed`. A directory prefix
/// ending in `/` also permits the directory itself to be absent.
pub fn disallowed_skips<'log>(log: &'log str, allowed: &[&str]) -> Vec<&'log str> {
    log.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            !allowed
                .iter()
                .any(|prefix| line.starts_with(prefix) || *line == prefix.trim_end_matches('/'))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_allowed_skips_pass() {
        let log = "assets\nassets/Epic RPG World - Desert V1.6\nreference/tiled/examples\n\nassets/x.tmx\n";
        assert_eq!(
            disallowed_skips(log, &["assets/"]),
            ["reference/tiled/examples"]
        );
        assert!(disallowed_skips(log, &["assets/", "reference/"]).is_empty());
        assert_eq!(
            disallowed_skips("assets-other\n", &["assets/"]),
            ["assets-other"]
        );
        assert_eq!(disallowed_skips("Godot 4\n", &["assets/"]), ["Godot 4"]);
        assert!(disallowed_skips("", &[]).is_empty());
    }
}
