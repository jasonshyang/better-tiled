//! The text of painted rows: a row of cells as space-separated tokens with run lengths, readable
//! in a diff and editable by hand.
//!
//! ```text
//! value   := "." | tileset ":" tile [ "/" flags ] ; tileset is an index into `tilesets`, tile an id
//! flags   := one or more of h v d x               ; horizontal, vertical, diagonal, hexagonal 120°
//! token   := value [ "*" count ]                  ; count repeats the value
//! row     := token ( " " token )*                 ; trailing empty cells are left off
//! ```
//!
//! [`encode`] writes the shortest form: equal neighbours as one token with a count, flags in the
//! order `hvdx`, runs longer than a million split. [`decode`] reads that and any other spelling of
//! the same cells (flags in any order, a count of one, trailing empty cells, more than one space).

use bt_core::{Flip, TileId};

/// The largest count a token may have.
const MAX_COUNT: usize = 1_000_000;

/// The flags in the order they are written, with their letters.
const FLAGS: [FlagLetter; 4] = [
    FlagLetter {
        flip: Flip::H,
        letter: 'h',
    },
    FlagLetter {
        flip: Flip::V,
        letter: 'v',
    },
    FlagLetter {
        flip: Flip::D,
        letter: 'd',
    },
    FlagLetter {
        flip: Flip::HEX120,
        letter: 'x',
    },
];

/// A flip flag and the letter that writes it.
struct FlagLetter {
    flip: Flip,
    letter: char,
}

/// One cell of a painted row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RowCell {
    /// No tile: `.`.
    #[default]
    Empty,
    /// A tile: `tileset:tile`, then `/` and its flags when it has any.
    Tile {
        /// The tileset, as an index into the painted entry's `tilesets`.
        tileset: usize,
        /// The tile's id in its tileset.
        tile: TileId,
        /// How the tile is turned and mirrored.
        flip: Flip,
    },
}

/// Why a row's text could not be read. Each names the token at fault.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RowError {
    /// A token whose count is above a million.
    #[error("`{token}`: a count above a million")]
    CountTooLarge {
        /// The token.
        token: String,
    },
    /// A token with a flag that is not one of `h`, `v`, `d` and `x`.
    #[error("`{token}`: `{flag}` is not a flag (h, v, d or x)")]
    UnknownFlag {
        /// The token.
        token: String,
        /// The flag.
        flag: char,
    },
    /// A token that is not `.` or `tileset:tile`, with `/flags` and `*count` after it: a part
    /// missing or not a number, a flag given twice, a count of zero, flags on an empty cell.
    #[error("`{token}` is not a cell (`.` or tileset:tile, then /flags and *count)")]
    Malformed {
        /// The token.
        token: String,
    },
}

impl RowError {
    /// The token at fault, as written.
    pub fn token(&self) -> &str {
        match self {
            RowError::CountTooLarge { token }
            | RowError::UnknownFlag { token, .. }
            | RowError::Malformed { token } => token,
        }
    }
}

/// The text of a row of cells, the inverse of [`decode`]: equal neighbours as one token with a
/// count, and the empty cells at the end left off.
pub fn encode(cells: &[RowCell]) -> String {
    let painted = cells
        .iter()
        .rposition(|cell| *cell != RowCell::Empty)
        .map_or(0, |last| last + 1);
    let mut tokens: Vec<String> = Vec::new();
    let mut cells = cells.iter().take(painted).peekable();
    while let Some(&cell) = cells.next() {
        let mut run = 1;
        while cells.next_if_eq(&&cell).is_some() {
            run += 1;
        }
        let value = value_text(cell);
        while run > 0 {
            let count = run.min(MAX_COUNT);
            tokens.push(if count == 1 {
                value.clone()
            } else {
                format!("{value}*{count}")
            });
            run -= count;
        }
    }
    tokens.join(" ")
}

/// The cells of a row's text, the inverse of [`encode`]. A count above a million, an unknown
/// flag or a malformed token is an error naming the token.
pub fn decode(text: &str) -> Result<Vec<RowCell>, RowError> {
    let mut cells = Vec::new();
    for token in text.split_ascii_whitespace() {
        let (value, count) = match token.split_once('*') {
            Some((value, count)) => (value, count_of(token, count)?),
            None => (token, 1),
        };
        let cell = cell_of(token, value)?;
        cells.extend(std::iter::repeat_n(cell, count));
    }
    Ok(cells)
}

/// How a cell is written, without its count.
fn value_text(cell: RowCell) -> String {
    match cell {
        RowCell::Empty => ".".to_owned(),
        RowCell::Tile {
            tileset,
            tile,
            flip,
        } => {
            let flags: String = FLAGS
                .iter()
                .filter(|flag| flip.contains(flag.flip))
                .map(|flag| flag.letter)
                .collect();
            if flags.is_empty() {
                format!("{tileset}:{tile}")
            } else {
                format!("{tileset}:{tile}/{flags}")
            }
        }
    }
}

/// The count written after `*` in `token`.
fn count_of(token: &str, text: &str) -> Result<usize, RowError> {
    if !is_number(text) {
        return Err(malformed(token));
    }
    match text.parse::<usize>() {
        Ok(0) => Err(malformed(token)),
        Ok(count) if count <= MAX_COUNT => Ok(count),
        // A number too long for `usize` is above a million too.
        _ => Err(RowError::CountTooLarge {
            token: token.to_owned(),
        }),
    }
}

/// The cell `value` of `token` stands for.
fn cell_of(token: &str, value: &str) -> Result<RowCell, RowError> {
    if value == "." {
        return Ok(RowCell::Empty);
    }
    let (tileset, rest) = value.split_once(':').ok_or_else(|| malformed(token))?;
    let (tile, flags) = match rest.split_once('/') {
        Some((tile, flags)) => (tile, Some(flags)),
        None => (rest, None),
    };
    let tileset = number(token, tileset)?;
    let tile = TileId(number(token, tile)?);
    let flip = match flags {
        Some(flags) => flip_of(token, flags)?,
        None => Flip::empty(),
    };
    Ok(RowCell::Tile {
        tileset,
        tile,
        flip,
    })
}

/// The flags of `token`: one or more letters, each once.
fn flip_of(token: &str, flags: &str) -> Result<Flip, RowError> {
    if flags.is_empty() {
        return Err(malformed(token));
    }
    let mut flip = Flip::empty();
    for letter in flags.chars() {
        let flag = FLAGS
            .iter()
            .find(|flag| flag.letter == letter)
            .ok_or_else(|| RowError::UnknownFlag {
                token: token.to_owned(),
                flag: letter,
            })?;
        if flip.contains(flag.flip) {
            return Err(malformed(token));
        }
        flip |= flag.flip;
    }
    Ok(flip)
}

/// A whole number of `token`, written in decimal digits only.
fn number<T: std::str::FromStr>(token: &str, text: &str) -> Result<T, RowError> {
    if !is_number(text) {
        return Err(malformed(token));
    }
    text.parse().map_err(|_| malformed(token))
}

/// Whether `text` is one or more decimal digits (no sign, no spaces).
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn malformed(token: &str) -> RowError {
    RowError::Malformed {
        token: token.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use bt_test_data::Fuzz;
    use bt_utils::rng::Rng;

    use super::*;

    fn tile(tileset: usize, tile: u32, flip: Flip) -> RowCell {
        RowCell::Tile {
            tileset,
            tile: TileId(tile),
            flip,
        }
    }

    const EMPTY: RowCell = RowCell::Empty;

    #[test]
    fn rows_are_written_with_runs_flags_and_no_trailing_empty_cells() {
        let plain = tile(0, 5, Flip::empty());
        assert_eq!(encode(&[]), "");
        assert_eq!(encode(&[EMPTY, EMPTY]), "", "nothing painted");
        assert_eq!(encode(&[plain, plain, plain]), "0:5*3");
        assert_eq!(
            encode(&[
                EMPTY,
                tile(0, 1, Flip::H | Flip::V),
                tile(1, 7, Flip::empty())
            ]),
            ". 0:1/hv 1:7"
        );
        assert_eq!(
            encode(&[EMPTY, EMPTY, EMPTY, EMPTY, tile(0, 2, Flip::empty()), EMPTY]),
            ".*4 0:2"
        );
        assert_eq!(
            encode(&[tile(12, 40000, Flip::all()), tile(12, 40000, Flip::D)]),
            "12:40000/hvdx 12:40000/d"
        );
        assert_eq!(
            encode(&[tile(0, 1, Flip::HEX120 | Flip::H)]),
            "0:1/hx",
            "flags in the order hvdx"
        );
    }

    #[test]
    fn rows_are_read_back_whatever_their_spelling() {
        let plain = tile(0, 5, Flip::empty());
        assert_eq!(decode("").unwrap(), []);
        assert_eq!(decode("0:5*3").unwrap(), [plain, plain, plain]);
        assert_eq!(
            decode(". 0:1/hv 1:7").unwrap(),
            [
                EMPTY,
                tile(0, 1, Flip::H | Flip::V),
                tile(1, 7, Flip::empty())
            ]
        );
        assert_eq!(
            decode("  0:1/vh   .*2 0:5*1 . ").unwrap(),
            [tile(0, 1, Flip::H | Flip::V), EMPTY, EMPTY, plain, EMPTY],
            "flags in any order, a count of one, spaces and trailing empty cells"
        );
        assert_eq!(
            decode("007:0005").unwrap(),
            [tile(7, 5, Flip::empty())],
            "leading zeros"
        );
    }

    #[test]
    fn runs_longer_than_a_million_are_split() {
        let cell = tile(1, 2, Flip::V);
        let cells = vec![cell; 2_500_000];
        let text = encode(&cells);
        assert_eq!(text, "1:2/v*1000000 1:2/v*1000000 1:2/v*500000");
        assert_eq!(decode(&text).unwrap(), cells);
        assert_eq!(decode("1:2/v*1000000").unwrap().len(), 1_000_000);
    }

    #[test]
    fn every_malformed_token_is_refused_with_its_text() {
        let malformed = |token: &str| RowError::Malformed {
            token: token.to_owned(),
        };
        let cases = [
            (
                "0:1*1000001",
                RowError::CountTooLarge {
                    token: "0:1*1000001".to_owned(),
                },
            ),
            (
                ".*99999999999999999999999",
                RowError::CountTooLarge {
                    token: ".*99999999999999999999999".to_owned(),
                },
            ),
            (
                "0:1/q",
                RowError::UnknownFlag {
                    token: "0:1/q".to_owned(),
                    flag: 'q',
                },
            ),
            (
                "0:1/hH",
                RowError::UnknownFlag {
                    token: "0:1/hH".to_owned(),
                    flag: 'H',
                },
            ),
            ("0:1/hh", malformed("0:1/hh")),
            ("0:1/", malformed("0:1/")),
            ("./h", malformed("./h")),
            (".*0", malformed(".*0")),
            ("0:1*", malformed("0:1*")),
            ("0:1*x", malformed("0:1*x")),
            ("0:1*+2", malformed("0:1*+2")),
            ("0:1*2*3", malformed("0:1*2*3")),
            ("*3", malformed("*3")),
            ("..", malformed("..")),
            ("x", malformed("x")),
            ("5", malformed("5")),
            (":5", malformed(":5")),
            ("0:", malformed("0:")),
            ("-1:5", malformed("-1:5")),
            ("0:-5", malformed("0:-5")),
            ("+0:5", malformed("+0:5")),
            ("0:1:2", malformed("0:1:2")),
            ("0:4294967296", malformed("0:4294967296")),
            ("0:1.5", malformed("0:1.5")),
        ];
        for (token, expected) in cases {
            let text = format!("0:1 {token} .");
            assert_eq!(decode(&text), Err(expected.clone()), "{text}");
            assert_eq!(expected.token(), token);
            assert!(expected.to_string().contains(token), "{expected}");
        }
        assert_eq!(
            RowError::UnknownFlag {
                token: "0:1/q".to_owned(),
                flag: 'q'
            }
            .to_string(),
            "`0:1/q`: `q` is not a flag (h, v, d or x)"
        );
    }

    /// A random row: gaps, runs of one cell, every flag, a few tilesets.
    fn random_row(rng: &mut Rng) -> Vec<RowCell> {
        let mut below = |n: u64| rng.below(n).unwrap_or(0);
        let length = below(80);
        let mut cells = Vec::new();
        while (cells.len() as u64) < length {
            let cell = if below(3) == 0 {
                EMPTY
            } else {
                let tileset = below(4) as usize;
                let id = below(70) as u32;
                let flip = Flip::from_bits_truncate(below(16) as u8);
                tile(tileset, id, flip)
            };
            let run = if below(4) == 0 { 1 + below(30) } else { 1 };
            cells.extend(std::iter::repeat_n(cell, run as usize));
        }
        cells
    }

    #[test]
    fn encoding_and_decoding_are_inverse_on_random_rows() {
        let fuzz = Fuzz::from_env(0x0B7_0005, 500);
        for case in 0..fuzz.cases {
            let mut rng = Rng::new(fuzz.case_seed(case));
            let cells = random_row(&mut rng);
            let text = encode(&cells);
            let painted = cells
                .iter()
                .rposition(|cell| *cell != EMPTY)
                .map_or(0, |last| last + 1);
            assert_eq!(
                decode(&text).as_deref(),
                Ok(&cells[..painted]),
                "{} case {case}: {text:?} from {cells:?}",
                fuzz.reproduce()
            );
            let tokens: Vec<&str> = text.split(' ').filter(|t| !t.is_empty()).collect();
            let value = |token: &str| token.split('*').next().unwrap_or_default().to_owned();
            assert!(
                tokens.last().is_none_or(|last| value(last) != ".")
                    && tokens.iter().all(|token| !token.ends_with("*1"))
                    && tokens
                        .windows(2)
                        .all(|pair| value(pair[0]) != value(pair[1]))
                    && !text.contains("  "),
                "{} case {case}: not the shortest form: {text:?}",
                fuzz.reproduce()
            );
            assert_eq!(
                encode(&decode(&text).unwrap()),
                text,
                "{} case {case}",
                fuzz.reproduce()
            );
        }
    }
}
