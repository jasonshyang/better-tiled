//! Qt-compatible argument grouping for Tiled external commands, without invoking a shell.
//! See <https://doc.qt.io/qt-6/qprocess.html#splitCommand>.

/// Separates whitespace-delimited arguments. Double quotes group whitespace, and a run of
/// three quotes inserts a literal quote. Backslashes and single quotes are ordinary characters.
/// An unfinished quoted group is an error rather than a silently changed command.
pub fn split(text: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            let mut count = 1;
            while chars.peek() == Some(&'"') {
                chars.next();
                count += 1;
            }
            token.extend(std::iter::repeat_n('"', count / 3));
            if count % 3 == 1 {
                quoted = !quoted;
            }
        } else if ch.is_whitespace() && !quoted {
            if !token.is_empty() {
                args.push(std::mem::take(&mut token));
            }
        } else {
            token.push(ch);
        }
    }
    if quoted {
        return Err("An argument has an unclosed double quote".into());
    }
    if !token.is_empty() {
        args.push(token);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qt_groups_paths_and_literal_quotes_without_shell_expansion() {
        assert_eq!(
            split(r#"dir "Epic 12""" Singles" C:\maps 'a b' $HOME > out"#).unwrap(),
            [
                "dir",
                "Epic 12\" Singles",
                "C:\\maps",
                "'a",
                "b'",
                "$HOME",
                ">",
                "out"
            ]
        );
        assert!(split("\"unfinished").is_err());
    }
}
