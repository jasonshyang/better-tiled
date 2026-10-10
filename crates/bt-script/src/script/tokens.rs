//! A script line as words (`10-agents.md`, §7): words with spaces go in quotes, `#` starts a
//! comment except in `#12` (an object id), and `--do "a; b"` holds several lines separated by
//! semicolons outside quotes.

/// The words of a line: `"quoted words"` are one word (also after `key=`), `\` in quotes takes
/// the next character as it is, and a `#` that does not start an object id (`#12`) starts a
/// comment. A quote left open is an error.
pub(crate) fn words_of(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else {
            break;
        };
        if first == '#' {
            let mut probe = chars.clone();
            probe.next();
            if !probe.peek().is_some_and(char::is_ascii_digit) {
                break;
            }
        }
        let mut word = String::new();
        let mut quoted = false;
        while let Some(&c) = chars.peek() {
            if c == '"' {
                chars.next();
                quoted = true;
                loop {
                    match chars.next() {
                        None => return Err("a quote is not closed".to_owned()),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped) => word.push(escaped),
                            None => return Err("a quote is not closed".to_owned()),
                        },
                        Some(c) => word.push(c),
                    }
                }
                continue;
            }
            if c.is_whitespace() {
                break;
            }
            word.push(c);
            chars.next();
        }
        if !word.is_empty() || quoted {
            words.push(word);
        }
    }
    Ok(words)
}

/// The comment a line holds, when the line is nothing but a comment: `# text` gives `text`.
pub(crate) fn comment_of(line: &str) -> Option<&str> {
    let text = line.trim().strip_prefix('#')?;
    if text.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(text.trim())
}

/// The lines of `bt edit --do "a; b"`: split at semicolons outside quotes.
pub fn split_do(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut quoted = false;
    let mut escaped = false;
    for c in text.chars() {
        if quoted {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            out.push(c);
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                out.push(c);
            }
            ';' => out.push('\n'),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_quotes_and_comments() {
        assert_eq!(
            words_of(r#"object spawn at 1,2 name "big boss" kind="slick ice" # a comment"#)
                .unwrap(),
            [
                "object",
                "spawn",
                "at",
                "1,2",
                "name",
                "big boss",
                "kind=slick ice"
            ]
        );
        assert_eq!(
            words_of("delete #12 #13").unwrap(),
            ["delete", "#12", "#13"]
        );
        assert_eq!(words_of("# just a comment").unwrap(), Vec::<String>::new());
        assert_eq!(words_of(r#"name "a \"b\"""#).unwrap(), ["name", "a \"b\""]);
        assert_eq!(words_of(r#"name """#).unwrap(), ["name", ""]);
        assert!(words_of("name \"open").is_err());
    }

    #[test]
    fn a_line_that_is_only_a_comment_gives_its_text() {
        assert_eq!(comment_of("  # widen the lake "), Some("widen the lake"));
        assert_eq!(comment_of("#12"), None);
        assert_eq!(comment_of("paint water all"), None);
    }

    #[test]
    fn semicolons_split_lines_outside_quotes() {
        assert_eq!(
            split_do(r#"paint water all; object sign at 1,1 text="a; b""#),
            "paint water all\n object sign at 1,1 text=\"a; b\""
        );
    }
}
