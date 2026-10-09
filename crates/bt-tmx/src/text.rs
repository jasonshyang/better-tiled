//! Reading text files in the encodings Tiled's files come in, and their line endings.

use std::path::Path;

use bt_core::LineEnding;

use crate::error::Error;

/// Reads a text file: UTF-8 with or without a byte order mark, or UTF-16 with one.
pub(crate) fn read_text(path: &Path) -> Result<String, Error> {
    let bytes = std::fs::read(path).map_err(|source| Error::from_io(path, source))?;
    decode_text(bytes).ok_or_else(|| Error::format(path, "the file is not UTF-8 or UTF-16 text"))
}

/// The text of a file's bytes; `None` when they are not UTF-8 or UTF-16 with a byte order mark.
pub(crate) fn decode_text(bytes: Vec<u8>) -> Option<String> {
    let utf16 = |rest: &[u8], big_endian: bool| {
        if !rest.len().is_multiple_of(2) {
            return None;
        }
        let units = rest.chunks_exact(2).map(|pair| match pair {
            [a, b] if big_endian => u16::from_be_bytes([*a, *b]),
            [a, b] => u16::from_le_bytes([*a, *b]),
            _ => 0,
        });
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .ok()
    };
    match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        _ => {
            let text = String::from_utf8(bytes).ok()?;
            Some(match text.strip_prefix('\u{feff}') {
                Some(rest) => rest.to_owned(),
                None => text,
            })
        }
    }
}

/// The line ending of a text's first line; a text without line breaks has Unix ones.
pub(crate) fn line_ending(text: &str) -> LineEnding {
    match text.find('\n') {
        Some(end) if text.get(..end).is_some_and(|line| line.ends_with('\r')) => LineEnding::CrLf,
        _ => LineEnding::Lf,
    }
}

/// `text`, written with `\n` line breaks, with the line ending asked for.
pub(crate) fn with_line_ending(text: String, line_ending: LineEnding) -> String {
    match line_ending {
        LineEnding::Lf => text,
        LineEnding::CrLf => text.replace('\n', "\r\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_order_marks_and_utf16_are_read() {
        assert_eq!(
            decode_text(b"\xEF\xBB\xBF<a/>".to_vec()).as_deref(),
            Some("<a/>")
        );
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("<a/>".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(le).as_deref(), Some("<a/>"));
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("é".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode_text(be).as_deref(), Some("é"));
        assert_eq!(decode_text(vec![0xC3, 0x28]), None);
        assert_eq!(decode_text(vec![0xFF, 0xFE, 0x41]), None);
    }

    #[test]
    fn the_first_line_decides_the_line_ending() {
        assert_eq!(line_ending("<a>\r\n<b/>\n"), LineEnding::CrLf);
        assert_eq!(line_ending("<a>\n<b/>\r\n"), LineEnding::Lf);
        assert_eq!(line_ending("<a/>"), LineEnding::Lf);
        assert_eq!(
            with_line_ending("a\nb\n".into(), LineEnding::CrLf),
            "a\r\nb\r\n"
        );
    }
}
