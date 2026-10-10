//! Tile layer data payloads: CSV and base64, uncompressed or compressed with zlib, gzip or zstd.
//! Decoding is lenient and bounded; encoding gives Tiled's bytes.

use std::io::Read;

use base64::Engine;
use bt_core::LayerDataFormat;

use crate::deflate::{self, Wrapper};

/// The gids decoded from a payload, and what was wrong with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DecodedGids {
    /// The gids that could be read, at most as many as asked for.
    pub(crate) gids: Vec<u32>,
    /// The first problem found.
    pub(crate) problem: Option<Corruption>,
}

/// A problem with layer data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Corruption {
    /// What is wrong.
    pub(crate) message: String,
    /// The index of the first cell it affects, row by row.
    pub(crate) at: usize,
}

/// Decodes comma-separated gids. As in Tiled, whitespace is ignored anywhere and an empty field
/// is 0; unlike Tiled, a trailing comma is accepted.
pub(crate) fn decode_csv(text: &str, expected: usize) -> DecodedGids {
    let mut gids = Vec::with_capacity(expected.min(text.len() / 2 + 1));
    let mut current: u32 = 0;
    let mut in_field = false;
    let mut problem = None;
    for c in text.chars() {
        match c {
            '0'..='9' => {
                let digit = u32::from(c) - u32::from('0');
                current = current.wrapping_mul(10).wrapping_add(digit);
                in_field = true;
            }
            ',' => {
                gids.push(current);
                current = 0;
                in_field = false;
            }
            c if c.is_whitespace() => {}
            c => {
                problem.get_or_insert_with(|| Corruption {
                    message: format!("unexpected character “{c}” in CSV layer data"),
                    at: gids.len(),
                });
                in_field = true;
            }
        }
    }
    if in_field {
        gids.push(current);
    }
    checked_length(DecodedGids { gids, problem }, expected)
}

/// Base64 that accepts missing padding and stray trailing bits, as Qt's decoder does.
const LENIENT_BASE64: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Decodes base64 bytes, ignoring whitespace; `None` when they are not base64.
pub(crate) fn decode_base64_bytes(text: &str) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    LENIENT_BASE64.decode(cleaned).ok()
}

/// Base64 as Qt's `toBase64` writes it: the standard alphabet with padding.
pub(crate) fn encode_base64_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decodes base64 layer data (little-endian `u32` gids), decompressed as `format` says. zlib
/// and gzip are told apart by their header whatever the format says, as Tiled's
/// `inflateInit2(15 + 32)` does; decompression stops past the size `expected` cells take.
pub(crate) fn decode_base64(text: &str, format: LayerDataFormat, expected: usize) -> DecodedGids {
    let Some(bytes) = decode_base64_bytes(text) else {
        return DecodedGids {
            gids: Vec::new(),
            problem: Some(Corruption {
                message: "the layer data is not valid base64".to_owned(),
                at: 0,
            }),
        };
    };
    let limit = expected.saturating_mul(4);
    let Inflated {
        mut bytes,
        mut problem,
    } = match format {
        LayerDataFormat::Base64Zlib | LayerDataFormat::Base64Gzip => {
            if bytes.starts_with(&[0x1f, 0x8b]) {
                read_limited(
                    flate2::read::GzDecoder::new(bytes.as_slice()),
                    limit,
                    "gzip",
                )
            } else {
                read_limited(
                    flate2::read::ZlibDecoder::new(bytes.as_slice()),
                    limit,
                    "zlib",
                )
            }
        }
        LayerDataFormat::Base64Zstd => {
            match ruzstd::decoding::StreamingDecoder::new(bytes.as_slice()) {
                Ok(decoder) => read_limited(decoder, limit, "zstd"),
                Err(error) => Inflated {
                    bytes: Vec::new(),
                    problem: Some(Corruption {
                        message: format!("the zstd layer data is corrupt ({error})"),
                        at: 0,
                    }),
                },
            }
        }
        LayerDataFormat::Xml | LayerDataFormat::Csv | LayerDataFormat::Base64 => Inflated {
            bytes,
            problem: None,
        },
    };
    if bytes.len() > limit {
        bytes.truncate(limit);
        problem.get_or_insert_with(|| Corruption {
            message: format!(
                "the layer data holds more than {expected} cells; the rest was ignored"
            ),
            at: expected,
        });
    }
    if bytes.len() % 4 != 0 {
        problem.get_or_insert_with(|| Corruption {
            message: format!("the layer data's {} bytes are not whole gids", bytes.len()),
            at: bytes.len() / 4,
        });
    }
    let gids = bytes
        .chunks_exact(4)
        .map(|chunk| match chunk {
            [a, b, c, d] => u32::from_le_bytes([*a, *b, *c, *d]),
            _ => 0,
        })
        .collect();
    checked_length(DecodedGids { gids, problem }, expected)
}

/// Decompressed bytes, and what went wrong.
struct Inflated {
    bytes: Vec<u8>,
    problem: Option<Corruption>,
}

/// Decompresses at most `limit + 1` bytes, so that a corrupt stream cannot allocate without
/// bound.
fn read_limited(reader: impl Read, limit: usize, what: &str) -> Inflated {
    let mut bytes = Vec::with_capacity(limit.min(64 << 20));
    let bound = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let error = reader.take(bound).read_to_end(&mut bytes).err();
    let problem = error.map(|error| Corruption {
        message: format!("the {what} layer data is corrupt ({error})"),
        at: bytes.len() / 4,
    });
    Inflated { bytes, problem }
}

/// The gids cut to `expected`, with a problem when there were more or fewer (unless a problem
/// found earlier explains missing cells).
pub(crate) fn checked_length(mut decoded: DecodedGids, expected: usize) -> DecodedGids {
    let got = decoded.gids.len();
    if got == expected || (decoded.problem.is_some() && got < expected) {
        decoded.gids.truncate(expected);
        return decoded;
    }
    let message = if got < expected {
        format!("the layer data holds {got} of its {expected} cells")
    } else {
        format!("the layer data holds {got} cells, not {expected}; the rest was ignored")
    };
    decoded.gids.truncate(expected);
    decoded.problem.get_or_insert(Corruption {
        message,
        at: got.min(expected),
    });
    decoded
}

/// CSV as Tiled writes it in XML: a line break, then each row's values, each followed by a
/// comma except the very last, each row ending in a line break. Minimized output has no line
/// breaks.
pub(crate) fn encode_csv(gids: &[u32], width: usize, minimize: bool) -> String {
    let mut text = String::with_capacity(gids.len() * 4 + 2);
    if !minimize {
        text.push('\n');
    }
    let width = width.max(1);
    for (i, gid) in gids.iter().enumerate() {
        text.push_str(&gid.to_string());
        if i + 1 < gids.len() {
            text.push(',');
        }
        if (i + 1) % width == 0 && !minimize {
            text.push('\n');
        }
    }
    text
}

/// Base64 layer data: little-endian `u32` gids, compressed as `format` says at `level`
/// (`None` for the default). zstd is written at its fastest level whatever `level` says: no
/// pure-Rust encoder offers zlib-style levels.
pub(crate) fn encode_base64(gids: &[u32], format: LayerDataFormat, level: Option<i32>) -> String {
    let bytes: Vec<u8> = gids.iter().flat_map(|gid| gid.to_le_bytes()).collect();
    let level = level.unwrap_or(-1);
    let compressed = match format {
        LayerDataFormat::Base64Zlib => deflate::compress(&bytes, level, Wrapper::Zlib),
        LayerDataFormat::Base64Gzip => deflate::compress(&bytes, level, Wrapper::Gzip),
        LayerDataFormat::Base64Zstd => ruzstd::encoding::compress_to_vec(
            bytes.as_slice(),
            ruzstd::encoding::CompressionLevel::Fastest,
        ),
        LayerDataFormat::Xml | LayerDataFormat::Csv | LayerDataFormat::Base64 => bytes,
    };
    encode_base64_bytes(&compressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_BASE64: [LayerDataFormat; 4] = [
        LayerDataFormat::Base64,
        LayerDataFormat::Base64Zlib,
        LayerDataFormat::Base64Gzip,
        LayerDataFormat::Base64Zstd,
    ];

    #[test]
    fn csv_is_read_leniently() {
        let decoded = decode_csv("\n1,2,3,\n4,0,2147483649\n", 6);
        assert_eq!(decoded.gids, [1, 2, 3, 4, 0, 0x8000_0001]);
        assert_eq!(decoded.problem, None);
        let decoded = decode_csv("5,,7,", 3);
        assert_eq!(
            (decoded.gids.as_slice(), decoded.problem),
            ([5, 0, 7].as_slice(), None)
        );
        let short = decode_csv("1,2", 3).problem.unwrap();
        assert_eq!((short.at, short.message.contains("2 of its 3")), (2, true));
        let long = decode_csv("1,2,3,4", 3);
        assert_eq!(long.gids.len(), 3);
        assert!(long.problem.unwrap().message.contains("not 3"));
        let bad = decode_csv("1,x2,3", 3);
        assert_eq!(bad.gids, [1, 2, 3]);
        assert_eq!(bad.problem.unwrap().at, 1);
    }

    #[test]
    fn every_base64_form_round_trips() {
        let gids = [1, 0, 0x8000_0005, 0x6000_0000 | 300, 7, 0x1000_0009];
        for format in ALL_BASE64 {
            let text = format!("\n   {}\n  ", encode_base64(&gids, format, None));
            let decoded = decode_base64(&text, format, gids.len());
            assert_eq!(decoded.gids, gids, "{format:?}");
            assert_eq!(decoded.problem, None, "{format:?}");
        }
        // A gzip member labelled zlib still decodes, as in Tiled.
        let gzip = encode_base64(&gids, LayerDataFormat::Base64Gzip, Some(9));
        assert_eq!(
            decode_base64(&gzip, LayerDataFormat::Base64Zlib, 6).gids,
            gids
        );
    }

    #[test]
    fn broken_base64_data_keeps_what_it_can() {
        assert!(
            decode_base64("!!!!", LayerDataFormat::Base64, 1)
                .problem
                .unwrap()
                .message
                .contains("base64")
        );
        let short = decode_base64(
            &encode_base64(&[1, 2], LayerDataFormat::Base64Zlib, None),
            LayerDataFormat::Base64Zlib,
            3,
        );
        assert_eq!(short.gids, [1, 2]);
        assert_eq!(short.problem.unwrap().at, 2);
        let long = decode_base64(
            &encode_base64(&[1, 2, 3, 4], LayerDataFormat::Base64Zstd, None),
            LayerDataFormat::Base64Zstd,
            2,
        );
        assert_eq!(long.gids, [1, 2]);
        let raw = encode_base64(&[1], LayerDataFormat::Base64, None);
        assert!(
            decode_base64(&raw, LayerDataFormat::Base64Zlib, 1)
                .problem
                .unwrap()
                .message
                .contains("zlib")
        );
    }

    #[test]
    fn csv_is_written_in_rows() {
        assert_eq!(encode_csv(&[1, 2, 3, 4], 2, false), "\n1,2,\n3,4\n");
        assert_eq!(encode_csv(&[1, 2, 3, 4], 2, true), "1,2,3,4");
        assert_eq!(encode_csv(&[], 2, false), "\n");
    }

    /// Random and mutated payloads: no panic, never more cells than asked for.
    #[test]
    fn garbage_never_panics() {
        let mut rng = bt_utils::rng::Rng::new(0x2545_f491_4f6c_dd1d);
        let mut below = |n: u64| usize::try_from(rng.below(n).unwrap()).unwrap();
        let valid = encode_base64(&[5; 64], LayerDataFormat::Base64Zlib, None);
        for _ in 0..500 {
            let len = below(80);
            let text: String = (0..len)
                .map(|_| char::from(b"AZaz09+/=, \n1x"[below(14)]))
                .collect();
            let mut mutated = valid.clone().into_bytes();
            let at = below(mutated.len() as u64);
            mutated[at] = b"ABab+/"[below(6)];
            let mutated = String::from_utf8(mutated).unwrap();
            for format in ALL_BASE64 {
                assert!(decode_base64(&text, format, 16).gids.len() <= 16);
                assert!(decode_base64(&mutated, format, 64).gids.len() <= 64);
            }
            assert!(decode_csv(&text, 16).gids.len() <= 16);
        }
    }
}
