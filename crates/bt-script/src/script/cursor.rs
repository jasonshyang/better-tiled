//! A cursor over a line's words with the small readers every operation's parser uses. Mistakes
//! are said as what was expected, in words the guide uses.

use bt_utils::IRect;
use glam::{DVec2, IVec2};

use super::values::{is_cell, is_point, is_size, parse_cell, parse_point, parse_region};

/// The words of a line and how far they have been read.
pub(crate) struct Cursor<'w> {
    words: &'w [String],
    at: usize,
}

impl<'w> Cursor<'w> {
    /// A cursor at the first of `words`.
    pub(crate) fn new(words: &'w [String]) -> Cursor<'w> {
        Cursor { words, at: 0 }
    }

    /// The next word, not taken.
    pub(crate) fn peek(&self) -> Option<&'w str> {
        self.words.get(self.at).map(String::as_str)
    }

    /// The word `ahead` words after the next one, not taken.
    pub(crate) fn peek_ahead(&self, ahead: usize) -> Option<&'w str> {
        self.words.get(self.at + ahead).map(String::as_str)
    }

    /// Takes the next word.
    pub(crate) fn take(&mut self) -> Option<&'w str> {
        let word = self.peek();
        if word.is_some() {
            self.at += 1;
        }
        word
    }

    /// Takes the next word, which must be there.
    pub(crate) fn take_word(&mut self, what: &str) -> Result<&'w str, String> {
        self.take().ok_or_else(|| format!("expected {what}"))
    }

    /// Takes `word`, which must be next.
    pub(crate) fn take_keyword(&mut self, word: &str) -> Result<(), String> {
        match self.take() {
            Some(found) if found == word => Ok(()),
            Some(found) => Err(format!("expected “{word}”, got “{found}”")),
            None => Err(format!("expected “{word}”")),
        }
    }

    /// Takes `word` when it is next.
    pub(crate) fn take_if(&mut self, word: &str) -> bool {
        if self.peek() == Some(word) {
            self.at += 1;
            return true;
        }
        false
    }

    /// Takes the next word when it is one of `words`.
    pub(crate) fn take_if_any(&mut self, words: &[&str]) -> Option<&'w str> {
        let word = self.peek().filter(|w| words.contains(w))?;
        self.at += 1;
        Some(word)
    }

    /// Takes a number.
    pub(crate) fn take_number(&mut self, what: &str) -> Result<f64, String> {
        let word = self.take_word(what)?;
        word.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("expected {what}, got “{word}”"))
    }

    /// Takes a whole number of zero or more.
    pub(crate) fn take_whole(&mut self, what: &str) -> Result<u64, String> {
        let word = self.take_word(what)?;
        word.parse::<u64>()
            .map_err(|_| format!("expected {what}, got “{word}”"))
    }

    /// Takes a point `X,Y`.
    pub(crate) fn take_point(&mut self, what: &str) -> Result<DVec2, String> {
        parse_point(self.take_word(what)?)
    }

    /// Takes a cell `X,Y`.
    pub(crate) fn take_cell(&mut self, what: &str) -> Result<IVec2, String> {
        parse_cell(self.take_word(what)?)
    }

    /// Takes the points that follow, at least `min` of them.
    pub(crate) fn take_points(&mut self, min: usize, what: &str) -> Result<Vec<DVec2>, String> {
        let mut points = Vec::new();
        while let Some(word) = self.peek().filter(|w| is_point(w)) {
            points.push(parse_point(word)?);
            self.at += 1;
        }
        if points.len() < min {
            return Err(format!("{what} needs at least {min} points X,Y"));
        }
        Ok(points)
    }

    /// Takes the cells that follow, at least one.
    pub(crate) fn take_cells(&mut self, what: &str) -> Result<Vec<IVec2>, String> {
        let mut cells = Vec::new();
        while let Some(word) = self.peek().filter(|w| is_cell(w)) {
            cells.push(parse_cell(word)?);
            self.at += 1;
        }
        if cells.is_empty() {
            return Err(format!("{what} needs a cell X,Y"));
        }
        Ok(cells)
    }

    /// Takes a region: `X,Y WxH` as two words, or `X1,Y1..X2,Y2`, or `X,Y,WxH`.
    pub(crate) fn take_region(&mut self) -> Result<IRect, String> {
        let first = self.take_word("a region X,Y WxH")?;
        if !first.contains("..")
            && let Some(size) = self.peek().filter(|w| is_size(w))
        {
            self.at += 1;
            return parse_region(&format!("{first} {size}"));
        }
        parse_region(first)
    }

    /// Takes a list written with commas, in one word or several (`a,b`, `a, b`, `a ,b`).
    pub(crate) fn take_list(&mut self, what: &str) -> Result<Vec<String>, String> {
        let mut items = Vec::new();
        loop {
            let word = self.take_word(what)?;
            items.extend(
                word.split(',')
                    .filter(|item| !item.is_empty())
                    .map(str::to_owned),
            );
            if word.ends_with(',') || self.peek().is_some_and(|next| next.starts_with(',')) {
                continue;
            }
            break;
        }
        if items.is_empty() {
            return Err(format!("expected {what}"));
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn lists_span_words_joined_by_commas() {
        let words = words("pine, oak ,birch rest");
        let mut cursor = Cursor::new(&words);
        assert_eq!(
            cursor.take_list("a sprite").unwrap(),
            ["pine", "oak", "birch"]
        );
        assert_eq!(cursor.peek(), Some("rest"));
    }

    #[test]
    fn regions_take_their_size_word() {
        let words = words("4,2 10x6 on grass");
        let mut cursor = Cursor::new(&words);
        assert_eq!(
            cursor.take_region().unwrap(),
            IRect::from_min_size(IVec2::new(4, 2), IVec2::new(10, 6))
        );
        assert_eq!(cursor.peek(), Some("on"));
        let words = self::words("4,2 on");
        let mut cursor = Cursor::new(&words);
        assert_eq!(cursor.take_region().unwrap().size(), IVec2::ONE);
    }

    #[test]
    fn mistakes_say_what_was_expected() {
        let words = words("seed x");
        let mut cursor = Cursor::new(&words);
        assert!(cursor.take_keyword("seed").is_ok());
        assert_eq!(
            cursor.take_whole("a seed").unwrap_err(),
            "expected a seed, got “x”"
        );
        assert_eq!(cursor.take_word("more").unwrap_err(), "expected more");
    }
}
