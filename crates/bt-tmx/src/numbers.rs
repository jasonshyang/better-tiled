//! Numbers as Tiled's files hold them: read leniently, written the way Qt formats them.

/// An integer read the way a forgiving reader should: surrounding whitespace, a leading `+` and
/// float notation (`32.0`, `1e+06`) are accepted; a real is rounded.
pub(crate) fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(value) = text.parse::<i64>() {
        return Some(value);
    }
    let real = text.parse::<f64>().ok()?;
    integer_of_real(real)
}

/// A real rounded to an integer, when it is finite and fits.
pub(crate) fn integer_of_real(real: f64) -> Option<i64> {
    // The bounds keep the rounded value inside i64.
    (real.is_finite() && real.abs() < 9.0e18).then(|| real.round() as i64)
}

/// A real, in decimal or exponent notation, with surrounding whitespace allowed.
pub(crate) fn parse_real(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok()
}

/// A boolean: `true`/`false` in any case, or an integer (non-zero is true).
pub(crate) fn parse_bool(text: &str) -> Option<bool> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        parse_int(text).map(|value| value != 0)
    }
}

/// A global tile id: unsigned, or negative as tools with signed 32-bit integers write the
/// flags.
pub(crate) fn parse_gid(text: &str) -> Option<u32> {
    let text = text.trim();
    if let Ok(value) = text.parse::<u32>() {
        return Some(value);
    }
    gid_of_integer(parse_int(text)?)
}

/// A global tile id from an integer, unsigned or the signed 32-bit form of the same bits.
pub(crate) fn gid_of_integer(value: i64) -> Option<u32> {
    u32::try_from(value)
        .ok()
        .or_else(|| i32::try_from(value).ok().map(i32::cast_unsigned))
}

/// `QString::number(value)`: `%g` with six significant digits, how Tiled writes coordinates,
/// sizes, opacities and other reals in XML (`146.97`, `1.23457e+06`).
pub(crate) fn real_g6(value: f64) -> String {
    format_g(value, 6)
}

/// `QString::number(value, 'g', 15)`: how Tiled's JSON writer writes reals.
pub(crate) fn real_g15(value: f64) -> String {
    format_g(value, 15)
}

/// C's `%.{precision}g` as Qt writes it: exponents with a sign and at least two digits,
/// trailing zeros dropped.
fn format_g(value: f64, precision: usize) -> String {
    if let Some(special) = special_real(value) {
        return special;
    }
    let precision = precision.max(1);
    let scientific = format!("{:.*e}", precision - 1, value);
    let Scientific { mantissa, exponent } = Scientific::of(&scientific);
    let fits = i32::try_from(precision).unwrap_or(i32::MAX);
    if exponent < -4 || exponent >= fits {
        exponent_form(trim_fraction(mantissa), exponent)
    } else {
        // Non-negative: the exponent is below the precision here.
        let decimals = usize::try_from(fits - 1 - exponent).unwrap_or(0);
        trim_fraction(&format!("{value:.decimals$}")).to_owned()
    }
}

/// `QString::number(value, 'g', QLocale::FloatingPointShortest)` (`QVariant::toString`): the
/// shortest text that reads back exactly, in exponent form when that is shorter. Tiled writes
/// `float` custom properties this way, and Qt's JSON document writes every real this way.
pub(crate) fn real_shortest(value: f64) -> String {
    if let Some(special) = special_real(value) {
        return special;
    }
    let scientific = format!("{value:e}");
    let Scientific { mantissa, exponent } = Scientific::of(&scientific);
    let digits = i32::try_from(mantissa.bytes().filter(u8::is_ascii_digit).count()).unwrap_or(0);
    let point = exponent + 1;
    // Qt's rule: "e+" and two exponent digits cost four characters.
    const BIAS: i32 = 4;
    let decimal = if point <= 0 {
        1 - point <= BIAS
    } else {
        point <= digits + BIAS
    };
    if decimal {
        format!("{value}")
    } else {
        exponent_form(mantissa, exponent)
    }
}

/// The text of values without digits: Qt's `nan`, `inf`, `-inf`, and the zeros.
fn special_real(value: f64) -> Option<String> {
    if value.is_nan() {
        Some("nan".to_owned())
    } else if value.is_infinite() {
        Some(if value > 0.0 { "inf" } else { "-inf" }.to_owned())
    } else if value == 0.0 {
        Some(if value.is_sign_negative() { "-0" } else { "0" }.to_owned())
    } else {
        None
    }
}

/// A number in Rust's scientific form (`1.5e-7`), split.
struct Scientific<'s> {
    mantissa: &'s str,
    exponent: i32,
}

impl Scientific<'_> {
    fn of(scientific: &str) -> Scientific<'_> {
        match scientific.split_once('e') {
            Some((mantissa, exponent)) => Scientific {
                mantissa,
                exponent: exponent.parse().unwrap_or(0),
            },
            None => Scientific {
                mantissa: scientific,
                exponent: 0,
            },
        }
    }
}

/// `d.ddde+XX`: Qt's exponent notation.
fn exponent_form(mantissa: &str, exponent: i32) -> String {
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.unsigned_abs())
}

/// A decimal without trailing zeros after its point, and without a bare point.
fn trim_fraction(number: &str) -> &str {
    if number.contains('.') {
        number.trim_end_matches('0').trim_end_matches('.')
    } else {
        number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_read_leniently() {
        assert_eq!(parse_int(" 42 "), Some(42));
        assert_eq!(parse_int("+7"), Some(7));
        assert_eq!(parse_int("32.0"), Some(32));
        assert_eq!(parse_int("1e+06"), Some(1_000_000));
        assert_eq!(parse_int("abc"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_real("1.23457e+06"), Some(1_234_570.0));
        assert_eq!(parse_real(" -10.4469"), Some(-10.4469));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
        assert_eq!(parse_gid("2147487940"), Some(2_147_487_940));
        assert_eq!(parse_gid("-2147479356"), Some(2_147_487_940));
        assert_eq!(parse_gid("5000000000"), None);
    }

    #[test]
    fn reals_are_written_like_qt() {
        let six = [
            (0.0, "0"),
            (1.0, "1"),
            (-96.6667, "-96.6667"),
            (1824.67, "1824.67"),
            (f64::from(0.39f32), "0.39"),
            (146.97, "146.97"),
            (693.727, "693.727"),
            (1_234_567.0, "1.23457e+06"),
            (100_000.0, "100000"),
            (999_999.5, "1e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e100, "1e+100"),
            (2.0 / 3.0, "0.666667"),
        ];
        for (value, expected) in six {
            assert_eq!(real_g6(value), expected, "{value}");
        }
        let fifteen = [
            (262.666666666667, "262.666666666667"),
            (0.490000009536743, "0.490000009536743"),
            (2.0 / 3.0, "0.666666666666667"),
            (1e14, "100000000000000"),
            (1e15, "1e+15"),
            (1.5e-5, "1.5e-05"),
            (-256.0, "-256"),
        ];
        for (value, expected) in fifteen {
            assert_eq!(real_g15(value), expected, "{value}");
        }
        let shortest = [
            (1.0, "1"),
            (0.1, "0.1"),
            (2.0 / 3.0, "0.6666666666666666"),
            (1e6, "1e+06"),
            (12345.0, "12345"),
            (1_234_500.0, "1234500"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5e-7, "-2.5e-07"),
            (f64::NAN, "nan"),
            (-0.0, "-0"),
        ];
        for (value, expected) in shortest {
            assert_eq!(real_shortest(value), expected, "{value}");
        }
    }
}
