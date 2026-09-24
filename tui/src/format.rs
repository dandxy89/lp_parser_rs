//! Shared number, duration and count formatting for the panes, so a value
//! reads the same wherever it appears.

use std::time::Duration;

/// Significant figures [`fmt_num`] keeps.
const SIGNIFICANT: i32 = 6;

/// Format a value for display: six significant figures in plain decimals
/// between `1e-4` and `1e6`, scientific notation outside that range, with
/// trailing zeros dropped (`2.5`, not `2.500000`), negative zero shown as `0`,
/// and infinities as `∞`.
pub fn fmt_num(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "\u{221e}".to_owned() } else { "-\u{221e}".to_owned() };
    }
    // Both zeros, and anything that rounds to one, print as a plain 0.
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs();
    if (1e-4..1e6).contains(&magnitude) {
        #[allow(clippy::cast_possible_truncation)] // log10 of a value in [1e-4, 1e6) is in [-4, 6)
        let exponent = magnitude.log10().floor() as i32;
        #[allow(clippy::cast_sign_loss)] // clamped non-negative
        let decimals = (SIGNIFICANT - 1 - exponent).clamp(0, 12) as usize;
        let text = format!("{value:.decimals$}");
        let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
        // A value that rounded to zero at this precision keeps its sign otherwise.
        return if text == "-0" { "0".to_owned() } else { text.to_owned() };
    }
    #[allow(clippy::cast_sign_loss)] // a positive constant
    let text = format!("{value:.digits$e}", digits = (SIGNIFICANT - 1) as usize);
    // Drop the mantissa's trailing zeros: `1.00000e7` reads as `1e7`.
    match text.split_once('e') {
        Some((mantissa, exponent)) if mantissa.contains('.') => {
            format!("{}e{exponent}", mantissa.trim_end_matches('0').trim_end_matches('.'))
        }
        _ => text,
    }
}

/// Format a duration at a precision that suits its size: `0.4ms`, `38ms`,
/// `1.24s`, `2m 05s`.
pub fn fmt_duration(duration: Duration) -> String {
    let seconds = duration.as_secs_f64();
    let millis = seconds * 1000.0;
    if millis < 10.0 {
        format!("{millis:.1}ms")
    } else if millis < 1000.0 {
        format!("{millis:.0}ms")
    } else if seconds < 60.0 {
        format!("{seconds:.2}s")
    } else {
        let whole = duration.as_secs();
        format!("{}m {:02}s", whole / 60, whole % 60)
    }
}

/// `count` and the noun that agrees with it: `1 row`, `3 rows`, `0 passes`.
pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_num_keeps_six_significant_figures() {
        assert_eq!(fmt_num(41.199_263), "41.1993");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(3.0), "3");
        assert_eq!(fmt_num(-0.000_123_456_78), "-0.000123457");
        assert_eq!(fmt_num(123_456.7), "123457");
    }

    #[test]
    fn fmt_num_goes_scientific_outside_the_plain_range() {
        assert_eq!(fmt_num(1e7), "1e7");
        assert_eq!(fmt_num(1_234_567.0), "1.23457e6");
        assert_eq!(fmt_num(0.000_012_5), "1.25e-5");
        assert_eq!(fmt_num(-2.5e-9), "-2.5e-9");
    }

    #[test]
    fn fmt_num_normalises_zero_and_infinity() {
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(0.0), "0");
        assert_eq!(fmt_num(f64::INFINITY), "\u{221e}");
        assert_eq!(fmt_num(f64::NEG_INFINITY), "-\u{221e}");
        assert_eq!(fmt_num(f64::NAN), "NaN");
    }

    #[test]
    fn fmt_duration_scales_its_unit() {
        assert_eq!(fmt_duration(Duration::ZERO), "0.0ms");
        assert_eq!(fmt_duration(Duration::from_micros(400)), "0.4ms");
        assert_eq!(fmt_duration(Duration::from_millis(38)), "38ms");
        assert_eq!(fmt_duration(Duration::from_millis(1240)), "1.24s");
        assert_eq!(fmt_duration(Duration::from_secs(125)), "2m 05s");
    }

    #[test]
    fn plural_agrees_with_the_count() {
        assert_eq!(plural(1, "pass", "passes"), "1 pass");
        assert_eq!(plural(3, "row", "rows"), "3 rows");
        assert_eq!(plural(0, "row", "rows"), "0 rows");
    }
}
