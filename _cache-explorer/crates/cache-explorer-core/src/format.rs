//! Every rule by which a number becomes text the student reads. Messages,
//! labels and views call these functions; nothing else in the crate turns a
//! number into text.

/// A data value, register value or count: plain digits below 10,000 (`53`,
/// `4096`), comma-grouped from there on (`10,000`).
pub(crate) fn data(n: impl Into<u64>) -> String {
    let n = n.into();
    if n < 10_000 {
        n.to_string()
    } else {
        grouped(n)
    }
}

/// A configuration value or a limit, as a sentence or a label states it:
/// comma-grouped from 1,000 on (`1,000`, `9,007,199,254,740,991`).
pub(crate) fn setting(n: impl Into<u64>) -> String {
    grouped(n.into())
}

/// `0x` and upper-case hex digits, zero-padded to at least `digits` digits.
pub(crate) fn hex(value: u32, digits: usize) -> String {
    format!("0x{value:0digits$X}")
}

/// "a" or "an", whichever reads right before `n` spoken aloud: "an 8-line
/// cache", "a 16-line cache".
pub(crate) fn a_or_an(n: impl Into<u64>) -> &'static str {
    if reads_with_vowel(n.into()) {
        "an"
    } else {
        "a"
    }
}

/// [`a_or_an`] at the start of a sentence.
pub(crate) fn capital_a_or_an(n: impl Into<u64>) -> &'static str {
    if reads_with_vowel(n.into()) {
        "An"
    } else {
        "A"
    }
}

/// Whether `n` spoken aloud starts with a vowel sound: when its leading group
/// of up to three digits is eight, eleven, eighteen, eighty-something or
/// eight hundred and something.
fn reads_with_vowel(n: u64) -> bool {
    let mut lead = n;
    while lead >= 1_000 {
        lead /= 1_000;
    }
    let mut first_digit = lead;
    while first_digit >= 10 {
        first_digit /= 10;
    }
    lead == 11 || lead == 18 || first_digit == 8
}

/// Decimal digits with a comma before every group of three.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn data_is_grouped_from_ten_thousand() {
        assert_eq!(data(0_u64), "0");
        assert_eq!(data(53_u32), "53");
        assert_eq!(data(4_096_u32), "4096");
        assert_eq!(data(9_999_u32), "9999");
        assert_eq!(data(10_000_u32), "10,000");
        assert_eq!(data(100_000_u32), "100,000");
        assert_eq!(data(u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn settings_are_grouped_from_one_thousand() {
        assert_eq!(setting(1_u8), "1");
        assert_eq!(setting(999_u32), "999");
        assert_eq!(setting(1_000_u32), "1,000");
        assert_eq!(setting(8_192_u32), "8,192");
        assert_eq!(setting((1_u64 << 53) - 1), "9,007,199,254,740,991");
    }

    #[test]
    fn hex_is_upper_case_and_padded() {
        assert_eq!(hex(0xA4, 3), "0x0A4");
        assert_eq!(hex(0xA4, 2), "0xA4");
        assert_eq!(hex(0x5, 2), "0x05");
        assert_eq!(hex(0xABCD, 2), "0xABCD");
        assert_eq!(hex(u32::MAX, 8), "0xFFFFFFFF");
    }

    #[test]
    fn articles_follow_the_spoken_number() {
        for n in [8_u64, 11, 18, 80, 88, 800, 8_192, 11_000, 18_500, 8_388_608] {
            assert_eq!(a_or_an(n), "an", "{n}");
            assert_eq!(capital_a_or_an(n), "An", "{n}");
        }
        for n in [
            0_u64, 1, 2, 4, 16, 32, 64, 110, 128, 180, 1_024, 1_100, 16_384, 1_048_576,
        ] {
            assert_eq!(a_or_an(n), "a", "{n}");
            assert_eq!(capital_a_or_an(n), "A", "{n}");
        }
    }
}
