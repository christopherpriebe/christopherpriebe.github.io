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
pub(crate) fn hex(value: impl Into<u64>, digits: usize) -> String {
    let value = value.into();
    format!("0x{value:0digits$X}")
}

/// An address or a line address as the student reads it: hex padded to
/// ⌈`address_bits` / 4⌉ digits, so `0x0A4` with 12-bit addresses and `0x00A4`
/// with 16-bit ones. A value too wide for the width keeps all its digits, so
/// an address the student wrote beyond it is quoted as written: `0x1000`.
pub(crate) fn address(value: impl Into<u64>, address_bits: u8) -> String {
    hex(value, usize::from(address_bits).div_ceil(4))
}

/// The bytes of a line, first to last, as a fill names them: two
/// [`address`]es joined by an en dash, `0x2A0–0x2AF` for a 16 B line with
/// 12-bit addresses.
pub(crate) fn line_range(line: u32, line_bytes: u32, address_bits: u8) -> String {
    let last = line.saturating_add(line_bytes.saturating_sub(1));
    format!(
        "{}\u{2013}{}",
        address(line, address_bits),
        address(last, address_bits)
    )
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

/// A number of cycles, as a stall and the bus's cycle chip give it: `20
/// cycles`, `1 cycle`, `2,000 cycles`. Grouped like a setting, since every
/// such figure is a multiple of the configured miss penalty.
pub(crate) fn cycles(n: impl Into<u64>) -> String {
    let n = n.into();
    format!("{} {}", setting(n), plural(n, "cycle", "cycles"))
}

/// A fixed-point number, `units` of 10^−`places`, written with exactly
/// `places` decimals and its whole part grouped like a setting: `0.40` for 40
/// hundredths, `7.6` for 76 tenths, `2,000.5` for 20,005 tenths.
pub(crate) fn decimal(units: u64, places: u32) -> String {
    let (whole, fraction) = match 10_u64.checked_pow(places) {
        Some(scale) => (units / scale, units % scale),
        // More places than a u64 has digits: every digit is a decimal.
        None => (0, units),
    };
    let width = usize::try_from(places).unwrap_or(0);
    if width == 0 {
        setting(whole)
    } else {
        format!("{}.{fraction:0width$}", setting(whole))
    }
}

/// `numerator / denominator` rounded half up to `places` decimals, as a
/// whole number of 10^−`places` units for [`decimal`]: 33 for 1 / 3 to two
/// places, and 13 for 1 / 8, since 0.125 rounds up to 0.13. `None` when the
/// denominator is 0, or the result is too large for a `u64`.
pub(crate) fn half_up(numerator: u64, denominator: u64, places: u32) -> Option<u64> {
    // In u128, where no u64 count times a scale of a u64 can overflow.
    let scaled = u128::from(numerator).checked_mul(10_u128.checked_pow(places)?)?;
    let twice = u128::from(denominator).checked_mul(2)?;
    let rounded = scaled
        .checked_mul(2)?
        .checked_add(u128::from(denominator))?
        .checked_div(twice)?;
    u64::try_from(rounded).ok()
}

/// A rate as a whole percent, rounded half up: `67%` for 2 in 3, and `13%`
/// for 1 in 8, since 12.5 rounds up. `None` when the denominator is 0.
pub(crate) fn percent(numerator: u64, denominator: u64) -> Option<String> {
    // A ratio in hundredths is a percent.
    half_up(numerator, denominator, 2).map(|percent| format!("{}%", data(percent)))
}

/// What the place of a figure shows while there is nothing to compute it
/// from, such as the hit rate before the run's first access: an em dash.
pub(crate) const NO_FIGURE: &str = "\u{2014}";

/// An event's label, `{entry}.{seq}` as in `5.3`. Plain digits, never
/// grouped: a label names an event rather than counting anything, and a
/// comma would read as a third number.
pub(crate) fn label(entry: u32, seq: u32) -> String {
    format!("{entry}.{seq}")
}

/// An entry's number as the CPU status names it, `#5`: plain digits, never
/// grouped, since like a label it names an entry rather than counting.
pub(crate) fn entry_number(entry: u32) -> String {
    format!("#{entry}")
}

/// A source line or column number, as a parse error or a fault names its
/// place: `Line 3, column 5`. Plain digits, never grouped, since like an
/// entry number it names a place rather than counting.
pub(crate) fn position(n: u32) -> String {
    n.to_string()
}

/// A number the student reads back as source or types back into a field:
/// plain digits, never grouped, so it reads as the assembly language writes
/// it (`st [0x0A4], #10000`) and a timing field's text parses as typed
/// (`1000`).
pub(crate) fn plain(n: impl Into<u64>) -> String {
    n.into().to_string()
}

/// The head of a word's column in the cache and memory tables: `+` and the
/// word's byte offset within its line in upper-case hex, as in `+0`, `+4`,
/// `+8`, `+C` for 4-byte words.
pub(crate) fn word_head(offset: u32) -> String {
    format!("+{offset:X}")
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
        assert_eq!(hex(0xA4_u32, 3), "0x0A4");
        assert_eq!(hex(0xA4_u32, 2), "0xA4");
        assert_eq!(hex(0x5_u32, 2), "0x05");
        assert_eq!(hex(0xABCD_u32, 2), "0xABCD");
        assert_eq!(hex(u32::MAX, 8), "0xFFFFFFFF");
    }

    #[test]
    fn addresses_are_padded_to_the_address_width() {
        assert_eq!(address(0xA4_u32, 8), "0xA4");
        assert_eq!(address(0xA4_u32, 12), "0x0A4");
        assert_eq!(address(0xA4_u32, 13), "0x00A4");
        assert_eq!(address(0xA4_u32, 16), "0x00A4");
        assert_eq!(address(0xA4_u32, 32), "0x000000A4");
        assert_eq!(address(0x1000_u32, 12), "0x1000");
        assert_eq!(address(0x1_0000_0000_u64, 32), "0x100000000");
        assert_eq!(address(u64::MAX, 8), "0xFFFFFFFFFFFFFFFF");
    }

    #[test]
    fn line_ranges_run_from_the_first_byte_to_the_last() {
        assert_eq!(line_range(0x2A0, 16, 12), "0x2A0\u{2013}0x2AF");
        assert_eq!(line_range(0x2A0, 16, 16), "0x02A0\u{2013}0x02AF");
        assert_eq!(line_range(0xA0, 32, 8), "0xA0\u{2013}0xBF");
        assert_eq!(line_range(0xA4, 4, 12), "0x0A4\u{2013}0x0A7");
        assert_eq!(
            line_range(0xFFFF_FF80, 128, 32),
            "0xFFFFFF80\u{2013}0xFFFFFFFF"
        );
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
    fn cycles_are_grouped_like_settings_and_singular_for_one() {
        assert_eq!(cycles(1_u32), "1 cycle");
        assert_eq!(cycles(20_u32), "20 cycles");
        assert_eq!(cycles(999_u32), "999 cycles");
        assert_eq!(cycles(2_000_u32), "2,000 cycles");
        assert_eq!(cycles(0_u64), "0 cycles");
    }

    #[test]
    fn decimals_keep_every_place_and_group_the_whole_part() {
        assert_eq!(decimal(40, 2), "0.40");
        assert_eq!(decimal(33, 2), "0.33");
        assert_eq!(decimal(100, 2), "1.00");
        assert_eq!(decimal(7, 2), "0.07");
        assert_eq!(decimal(76, 1), "7.6");
        assert_eq!(decimal(90, 1), "9.0");
        assert_eq!(decimal(0, 1), "0.0");
        assert_eq!(decimal(20_005, 1), "2,000.5");
        assert_eq!(decimal(99_999, 1), "9,999.9");
        assert_eq!(decimal(1_234, 0), "1,234");
        assert_eq!(decimal(u64::MAX, 1), "1,844,674,407,370,955,161.5");
        assert_eq!(decimal(5, 20), "0.00000000000000000005");
    }

    #[test]
    fn half_up_rounds_halves_away_from_zero() {
        assert_eq!(half_up(1, 3, 2), Some(33));
        assert_eq!(half_up(2, 3, 2), Some(67));
        assert_eq!(half_up(4, 10, 2), Some(40));
        assert_eq!(half_up(1, 8, 2), Some(13));
        assert_eq!(half_up(3, 8, 2), Some(38));
        assert_eq!(half_up(1, 16, 2), Some(6));
        assert_eq!(half_up(175, 100, 1), Some(18));
        assert_eq!(half_up(174, 100, 1), Some(17));
        assert_eq!(half_up(23, 3, 1), Some(77));
        assert_eq!(half_up(0, 3, 2), Some(0));
        assert_eq!(half_up(3, 3, 2), Some(100));
        assert_eq!(half_up(7, 2, 0), Some(4));
        assert_eq!(half_up(u64::MAX, 1, 0), Some(u64::MAX));
    }

    #[test]
    fn half_up_has_no_result_without_a_denominator_or_room() {
        assert_eq!(half_up(1, 0, 2), None);
        assert_eq!(half_up(0, 0, 1), None);
        assert_eq!(half_up(u64::MAX, 1, 1), None);
        assert_eq!(half_up(1, 1, 40), None);
    }

    #[test]
    fn percents_are_whole_and_round_half_up() {
        assert_eq!(percent(2, 3).as_deref(), Some("67%"));
        assert_eq!(percent(1, 3).as_deref(), Some("33%"));
        // 12.5 % and 37.5 % round up.
        assert_eq!(percent(1, 8).as_deref(), Some("13%"));
        assert_eq!(percent(3, 8).as_deref(), Some("38%"));
        assert_eq!(percent(0, 5).as_deref(), Some("0%"));
        assert_eq!(percent(5, 5).as_deref(), Some("100%"));
        assert_eq!(percent(1, 201).as_deref(), Some("0%"));
        assert_eq!(percent(1, 200).as_deref(), Some("1%"));
        assert_eq!(percent(1, 0), None);
    }

    #[test]
    fn labels_are_entry_dot_seq_in_plain_digits() {
        assert_eq!(label(5, 3), "5.3");
        assert_eq!(label(12_345, 10), "12345.10");
    }

    #[test]
    fn line_and_column_numbers_are_plain_digits() {
        assert_eq!(position(3), "3");
        assert_eq!(position(16_385), "16385");
    }

    #[test]
    fn source_and_field_numbers_are_plain_digits() {
        assert_eq!(plain(7_u32), "7");
        assert_eq!(plain(1_000_u32), "1000");
        assert_eq!(plain(u64::MAX), "18446744073709551615");
    }

    #[test]
    fn word_heads_give_the_byte_offset_in_hex() {
        let heads: Vec<String> = [0, 4, 8, 12].into_iter().map(word_head).collect();
        assert_eq!(heads, ["+0", "+4", "+8", "+C"]);
        assert_eq!(word_head(0x78), "+78");
    }

    #[test]
    fn entry_numbers_are_plain_digits_after_a_number_sign() {
        assert_eq!(entry_number(5), "#5");
        assert_eq!(entry_number(12_345), "#12345");
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
