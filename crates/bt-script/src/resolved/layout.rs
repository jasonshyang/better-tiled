//! How the resolved JSON is laid out: indented like Tiled's own JSON, one space a level, except
//! that an array of numbers or strings stays on one line, so a tile layer's row, a point or a
//! rectangle reads as one line however long. Whole numbers are written without a fraction.

use std::io::Write;

use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter};

/// `value` as the resolved JSON lays it out, ending with a line break.
pub(crate) fn json_text<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, Layout::default());
    value.serialize(&mut serializer)?;
    bytes.push(b'\n');
    // The serializer writes UTF-8 only.
    String::from_utf8(bytes).map_err(|error| {
        serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })
}

/// How the values of an array are laid out, decided by its first value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArrayLayout {
    /// No value seen yet.
    Undecided,
    /// Numbers or strings: on the array's line.
    OnOneLine,
    /// Arrays or objects: one a line.
    OneALine,
}

/// An array or object being written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    /// An array, laid out as its first value decides.
    Array(ArrayLayout),
    /// An object, which has fields when the flag says so.
    Object {
        /// Whether a field was written.
        has_fields: bool,
    },
}

/// The formatter: the arrays and objects open, and whether a value of the innermost array is
/// about to be written.
#[derive(Debug, Default)]
struct Layout {
    /// The arrays and objects open, the innermost last.
    open: Vec<Open>,
    /// A value of the innermost array is next, and nothing was written before it yet.
    value_starts: bool,
    /// That value is the array's first.
    first_value: bool,
}

impl Layout {
    /// Writes what goes before a value of the innermost array: nothing or a space on one line, a
    /// line break and the indent on lines of their own.
    fn write_before_value<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        scalar: bool,
    ) -> std::io::Result<()> {
        if !std::mem::take(&mut self.value_starts) {
            return Ok(());
        }
        let depth = self.open.len();
        let Some(Open::Array(layout)) = self.open.last_mut() else {
            return Ok(());
        };
        if *layout == ArrayLayout::Undecided {
            *layout = if scalar {
                ArrayLayout::OnOneLine
            } else {
                ArrayLayout::OneALine
            };
        }
        match *layout {
            ArrayLayout::OnOneLine if self.first_value => Ok(()),
            ArrayLayout::OnOneLine => writer.write_all(b" "),
            ArrayLayout::Undecided | ArrayLayout::OneALine => new_line(writer, depth),
        }
    }
}

/// A line break and the indent of `depth`.
fn new_line<W: ?Sized + Write>(writer: &mut W, depth: usize) -> std::io::Result<()> {
    writer.write_all(b"\n")?;
    for _ in 0..depth {
        writer.write_all(b" ")?;
    }
    Ok(())
}

impl Formatter for Layout {
    fn write_null<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        writer.write_all(b"null")
    }

    fn write_bool<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        value: bool,
    ) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        CompactFormatter.write_bool(writer, value)
    }

    fn write_i32<W: ?Sized + Write>(&mut self, writer: &mut W, value: i32) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        CompactFormatter.write_i32(writer, value)
    }

    fn write_i64<W: ?Sized + Write>(&mut self, writer: &mut W, value: i64) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        CompactFormatter.write_i64(writer, value)
    }

    fn write_u32<W: ?Sized + Write>(&mut self, writer: &mut W, value: u32) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        CompactFormatter.write_u32(writer, value)
    }

    fn write_u64<W: ?Sized + Write>(&mut self, writer: &mut W, value: u64) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        CompactFormatter.write_u64(writer, value)
    }

    fn write_f64<W: ?Sized + Write>(&mut self, writer: &mut W, value: f64) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        // Whole numbers that an i64 holds exactly are written without a fraction.
        if value.fract() == 0.0 && value.abs() < 1e15 {
            CompactFormatter.write_i64(writer, value as i64)
        } else {
            CompactFormatter.write_f64(writer, value)
        }
    }

    fn begin_string<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.write_before_value(writer, true)?;
        CompactFormatter.begin_string(writer)
    }

    fn begin_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.write_before_value(writer, false)?;
        self.open.push(Open::Array(ArrayLayout::Undecided));
        writer.write_all(b"[")
    }

    fn end_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        if let Some(Open::Array(ArrayLayout::OneALine)) = self.open.pop() {
            new_line(writer, self.open.len())?;
        }
        writer.write_all(b"]")
    }

    fn begin_array_value<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if !first {
            writer.write_all(b",")?;
        }
        self.value_starts = true;
        self.first_value = first;
        Ok(())
    }

    fn end_array_value<W: ?Sized + Write>(&mut self, _writer: &mut W) -> std::io::Result<()> {
        Ok(())
    }

    fn begin_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.write_before_value(writer, false)?;
        self.open.push(Open::Object { has_fields: false });
        writer.write_all(b"{")
    }

    fn end_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        if let Some(Open::Object { has_fields: true }) = self.open.pop() {
            new_line(writer, self.open.len())?;
        }
        writer.write_all(b"}")
    }

    fn begin_object_key<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if !first {
            writer.write_all(b",")?;
        }
        if let Some(Open::Object { has_fields }) = self.open.last_mut() {
            *has_fields = true;
        }
        new_line(writer, self.open.len())
    }

    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Serialize)]
    struct Sample {
        name: &'static str,
        point: [f64; 2],
        rows: Vec<Vec<u32>>,
        empty: Vec<u32>,
        nothing: BTreeMap<u32, u32>,
        keyed: BTreeMap<u32, &'static str>,
        half: f64,
    }

    #[test]
    fn rows_of_numbers_stay_on_a_line_and_whole_numbers_have_no_fraction() {
        let sample = Sample {
            name: "a \"map\"",
            point: [16.0, -2.5],
            rows: vec![vec![1, 2, 0], vec![0, 0, 3]],
            empty: Vec::new(),
            nothing: BTreeMap::new(),
            keyed: BTreeMap::from([(2, "b"), (10, "j")]),
            half: 0.5,
        };
        let text = json_text(&sample).unwrap();
        assert_eq!(
            text,
            "{\n \"name\": \"a \\\"map\\\"\",\n \"point\": [16, -2.5],\n \"rows\": [\n  [1, 2, 0],\n  \
             [0, 0, 3]\n ],\n \"empty\": [],\n \"nothing\": {},\n \"keyed\": {\n  \"2\": \"b\",\n  \
             \"10\": \"j\"\n },\n \"half\": 0.5\n}\n"
        );
        let read: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(read["keyed"]["10"], "j");
        assert_eq!(read["point"][0], 16.0);
    }
}
