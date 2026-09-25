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

/// A data value with its sign: [`data`] of `magnitude`, after a minus sign
/// (U+2212) when `negative`, as in `−3` and `−32,768`. Zero has no sign.
pub(crate) fn signed_data(negative: bool, magnitude: u64) -> String {
    if negative && magnitude > 0 {
        format!("\u{2212}{}", data(magnitude))
    } else {
        data(magnitude)
    }
}

/// The lower bound of a signed range in a message: [`setting`] of
/// `magnitude` after a minus sign (U+2212), as in `−128 to 255`.
pub(crate) fn negative_setting(magnitude: u64) -> String {
    format!("\u{2212}{}", setting(magnitude))
}

/// `0x` and upper-case hex digits, zero-padded to at least `digits` digits.
pub(crate) fn hex(value: u32, digits: usize) -> String {
    format!("0x{value:0digits$X}")
}

/// An address or a line address as the student reads it: hex padded to
/// ⌈`address_bits` / 4⌉ digits, so `0x0A4` with 12-bit addresses and `0x00A4`
/// with 16-bit ones. A value too wide for the width keeps all its digits.
pub(crate) fn address(value: u32, address_bits: u8) -> String {
    hex(value, usize::from(address_bits).div_ceil(4))
}

/// A tag as the student reads it: hex padded to ⌈`tag_bits` / 4⌉ digits, and to
/// at least two, so `0x05` for a 5-bit tag and `0x001` for a 9-bit one.
pub(crate) fn tag(value: u32, tag_bits: u8) -> String {
    hex(value, usize::from(tag_bits).div_ceil(4).max(2))
}

/// The low `width` bits of `value` as binary digits, most significant first,
/// as the address strip shows them: exactly `width` characters, none for 0.
pub(crate) fn bits(value: u32, width: u8) -> String {
    (0..u32::from(width))
        .rev()
        .map(|bit| {
            if value.checked_shr(bit).unwrap_or(0) & 1 == 1 {
                '1'
            } else {
                '0'
            }
        })
        .collect()
}

/// An event's label, `{entry}.{seq}` as in `5.3`. Plain digits, never
/// grouped: a label names an event rather than counting anything, and a
/// comma would read as a third number.
pub(crate) fn label(entry: u32, seq: u32) -> String {
    format!("{entry}.{seq}")
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

/// The noun that reads right after `n`: `one` for 1 and `many` otherwise, as
/// in "1 way" and "8 ways".
pub(crate) fn plural(n: impl Into<u64>, one: &'static str, many: &'static str) -> &'static str {
    if n.into() == 1 {
        one
    } else {
        many
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
    fn signed_values_take_a_minus_sign() {
        assert_eq!(signed_data(false, 300), "300");
        assert_eq!(signed_data(true, 3), "\u{2212}3");
        assert_eq!(signed_data(true, 4_096), "\u{2212}4096");
        assert_eq!(signed_data(true, 32_768), "\u{2212}32,768");
        assert_eq!(signed_data(true, 0), "0");
        assert_eq!(negative_setting(128), "\u{2212}128");
        assert_eq!(negative_setting(2_048), "\u{2212}2,048");
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
    fn addresses_are_padded_to_the_address_width() {
        assert_eq!(address(0xA4, 8), "0xA4");
        assert_eq!(address(0xA4, 12), "0x0A4");
        assert_eq!(address(0xA4, 13), "0x00A4");
        assert_eq!(address(0xA4, 16), "0x00A4");
        assert_eq!(address(0xA4, 32), "0x000000A4");
        assert_eq!(address(0x1000, 12), "0x1000");
    }

    #[test]
    fn tags_are_padded_to_at_least_two_digits() {
        assert_eq!(tag(0x1, 1), "0x01");
        assert_eq!(tag(0x5, 5), "0x05");
        assert_eq!(tag(0xA, 8), "0x0A");
        assert_eq!(tag(0x1, 9), "0x001");
        assert_eq!(tag(0x1, 25), "0x0000001");
    }

    #[test]
    fn bits_are_exactly_as_wide_as_asked() {
        assert_eq!(bits(0xA4, 12), "000010100100");
        assert_eq!(bits(0b10, 3), "010");
        assert_eq!(bits(1, 1), "1");
        assert_eq!(bits(0, 0), "");
        assert_eq!(bits(u32::MAX, 32), "1".repeat(32));
    }

    #[test]
    fn plurals_are_singular_only_for_one() {
        assert_eq!(plural(1_u32, "way", "ways"), "way");
        for n in [0_u32, 2, 8, 1_000] {
            assert_eq!(plural(n, "way", "ways"), "ways", "{n}");
        }
    }

    #[test]
    fn labels_are_entry_dot_seq_in_plain_digits() {
        assert_eq!(label(5, 3), "5.3");
        assert_eq!(label(12_345, 10), "12345.10");
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
