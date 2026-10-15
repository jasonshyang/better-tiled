//! Godot's text resource syntax (`.tscn`, format 3) for the values the exporter writes, as Godot
//! 4.7 writes them itself.

use std::fmt::Write;

use bt_core::Color;
use glam::{DVec2, IVec2};

/// A number where Godot expects a float: whole numbers keep a `.0` so that they stay floats.
pub fn float(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// A coordinate inside a vector (always a float there, so no `.0` needed).
pub fn coord(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

pub fn vector2(v: DVec2) -> String {
    format!("Vector2({}, {})", coord(v.x), coord(v.y))
}

pub fn vector2i(v: IVec2) -> String {
    format!("Vector2i({}, {})", v.x, v.y)
}

pub fn rect2(min: DVec2, size: DVec2) -> String {
    format!(
        "Rect2({}, {}, {}, {})",
        coord(min.x),
        coord(min.y),
        coord(size.x),
        coord(size.y)
    )
}

pub fn color(c: Color) -> String {
    let f = |v: u8| coord((v as f64 / 255.0 * 1e6).round() / 1e6);
    format!("Color({}, {}, {}, {})", f(c.r), f(c.g), f(c.b), f(c.a))
}

pub fn points(points: &[DVec2]) -> String {
    let mut out = String::from("PackedVector2Array(");
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}, {}", coord(p.x), coord(p.y));
    }
    out.push(')');
    out
}

pub fn bytes(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 4 + 20);
    out.push_str("PackedByteArray(");
    for (i, b) in data.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{b}");
    }
    out.push(')');
    out
}

/// A string literal.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A node name: Godot doesn't allow `. : @ / " %` in names.
pub fn node_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if ".:@/\"%".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    name.trim().to_owned()
}

/// A metadata key: ASCII letters, digits and underscores, not starting with a digit, since
/// Godot's `set_meta` refuses any other on load. Latin letters lose their accents (`état` →
/// `etat`) rather than become `_`, which at the start would make the key editor-only metadata,
/// hidden in Godot's inspector; any other character becomes `_`.
pub fn identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            c if c.is_ascii_alphanumeric() || c == '_' => out.push(c),
            c => out.push_str(unaccented(c).unwrap_or("_")),
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// A Latin letter with an accent (Latin-1 and Latin Extended-A) as ASCII: its letter, or the
/// letters a ligature stands for.
fn unaccented(c: char) -> Option<&'static str> {
    // One letter per code point from U+00C0 and from U+0100; `_` for none (or a ligature).
    const LATIN_1: &str = "AAAAAA_CEEEEIIIIDNOOOOO_OUUUUY__aaaaaa_ceeeeiiiidnooooo_ouuuuy_y";
    const EXTENDED_A: &str = "AaAaAaCcCcCcCcDdDdEeEeEeEeEeGgGgGgGgHhHhIiIiIiIiIi__JjKkkLlLlLlLlLlNnNnNnnNnOoOoOo__RrRrRrSsSsSsSsTtTtTtUuUuUuUuUuUuWwYyYZzZzZzs";
    let ligature = match c {
        'Æ' => "AE",
        'æ' => "ae",
        'Þ' => "TH",
        'þ' => "th",
        'ß' => "ss",
        'Ĳ' => "IJ",
        'ĳ' => "ij",
        'Œ' => "OE",
        'œ' => "oe",
        _ => "",
    };
    if !ligature.is_empty() {
        return Some(ligature);
    }
    let (table, i) = match c as usize {
        i @ 0xC0..=0xFF => (LATIN_1, i - 0xC0),
        i @ 0x100..=0x17F => (EXTENDED_A, i - 0x100),
        _ => return None,
    };
    Some(&table[i..=i]).filter(|l| *l != "_")
}

/// A property's name as a `.tscn` writes it before ` = `: quoted where it has characters Godot
/// would otherwise not read as part of it (spaces, `=`, `"`, `;`, brackets, and anything
/// beyond ASCII), as Godot's `String::property_name_encode` does.
pub fn property_name(name: &str) -> String {
    if name
        .chars()
        .any(|c| "=\";[]".contains(c) || !('!'..='~').contains(&c))
    {
        string(name)
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_written_like_godot_writes_them() {
        assert_eq!(
            (float(2.0), float(0.5), float(-1.25)),
            ("2.0".to_owned(), "0.5".to_owned(), "-1.25".to_owned())
        );
        assert_eq!(vector2(DVec2::new(732.0, 220.5)), "Vector2(732, 220.5)");
        assert_eq!(vector2i(IVec2::new(-2, 3)), "Vector2i(-2, 3)");
        assert_eq!(
            color(Color::rgba(255, 0, 0, 128)),
            "Color(1, 0, 0, 0.501961)"
        );
        assert_eq!(
            points(&[DVec2::ZERO, DVec2::new(10.0, 0.5)]),
            "PackedVector2Array(0, 0, 10, 0.5)"
        );
        assert_eq!(bytes(&[0, 16, 255]), "PackedByteArray(0, 16, 255)");
        assert_eq!(string("say \"hi\"\n"), r#""say \"hi\"\n""#);
        assert_eq!(node_name(" a.b/c:d "), "a_b_c_d");
        assert_eq!(
            (identifier("max hp"), identifier("2nd")),
            ("max_hp".to_owned(), "_2nd".to_owned())
        );
        assert_eq!(
            (
                property_name("gold"),
                property_name("état"),
                property_name("max hp")
            ),
            (
                "gold".to_owned(),
                "\"état\"".to_owned(),
                "\"max hp\"".to_owned()
            )
        );
    }

    /// Godot refuses metadata keys beyond ASCII ("Invalid metadata identifier"), so accented
    /// Latin letters lose their accents and anything else becomes `_`.
    #[test]
    fn metadata_keys_are_ascii_identifiers() {
        let keys: Vec<String> = ["état", "größe", "Ærø", "Łódź", "Œuvre", "体力", "", "ok_1"]
            .map(identifier)
            .into();
        assert_eq!(
            keys,
            [
                "etat", "grosse", "AEro", "Lodz", "OEuvre", "__", "_", "ok_1"
            ]
        );
        // Every letter of both blocks comes out ASCII, the signs (× ÷) as `_`.
        for c in ('\u{C0}'..='\u{17F}').filter(|c| c.is_alphabetic()) {
            let key = identifier(&format!("a{c}"));
            assert!(
                key.len() > 1 && key.bytes().all(|b| b.is_ascii_alphabetic()),
                "{c} → {key}"
            );
        }
        assert_eq!(
            (identifier("a×b"), identifier("a÷b")),
            ("a_b".to_owned(), "a_b".to_owned())
        );
    }
}
