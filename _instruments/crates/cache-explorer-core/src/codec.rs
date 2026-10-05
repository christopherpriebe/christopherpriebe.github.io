//! The codec of a share string: a document's compact JSON deflated and
//! written in base64url without padding, and the same text read back within
//! the limits a link and a file may have.
//!
//! The codec knows nothing of what the JSON holds. A caller passes the
//! [`Limits`] and words the errors itself: a size error is data here, the
//! size, and the sentence about it is the caller's.

use base64::engine::general_purpose::{URL_SAFE_NO_PAD, URL_SAFE_NO_PAD_INDIFFERENT};
use base64::Engine as _;
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::{decompress_to_vec_with_limit, TINFLStatus};

/// The deflate level of a share string: the shortest links, and always the
/// same string for the same document.
const DEFLATE_LEVEL: u8 = 9;

/// The most a link and a file may hold, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most a share string may be long.
    pub text_bytes: u32,
    /// The most JSON a share string may unpack to, and a file may hold.
    pub json_bytes: u32,
}

/// What is larger than a link or a file may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oversize {
    /// The share string is longer than [`Limits::text_bytes`].
    Text {
        /// Its length, in bytes.
        bytes: u32,
    },
    /// The JSON is larger than [`Limits::json_bytes`].
    Json {
        /// Its size in bytes: a file's; none for a share string, whose
        /// inflating stops at the limit.
        bytes: Option<u32>,
    },
}

/// Why a share string cannot be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    /// The text is not base64url.
    Base64,
    /// The bytes are not a deflate stream.
    Deflate,
    /// The inflated bytes are not UTF-8 text.
    Utf8,
    /// The text, or what it inflates to, is larger than may be opened.
    Oversize(Oversize),
}

/// `json` as a share string: deflated, then base64url without padding.
pub fn encode(json: &str) -> String {
    URL_SAFE_NO_PAD.encode(compress_to_vec(json.as_bytes(), DEFLATE_LEVEL))
}

/// `json` as a share string, if a link can open it: the JSON within
/// [`Limits::json_bytes`], the most a link unpacks to, and the string within
/// [`Limits::text_bytes`].
///
/// # Errors
///
/// [`Oversize::Json`] with the JSON's size, or [`Oversize::Text`] with the
/// string's length: the first of the two a link could not open.
pub fn link(json: &str, limits: Limits) -> Result<String, Oversize> {
    if exceeds(json.len(), limits.json_bytes) {
        return Err(Oversize::Json {
            bytes: Some(count(json.len())),
        });
    }
    let share = encode(json);
    if exceeds(share.len(), limits.text_bytes) {
        return Err(Oversize::Text {
            bytes: count(share.len()),
        });
    }
    Ok(share)
}

/// The JSON text a share string holds: bounded first, then decoded from
/// base64url with or without padding and inflated, the inflating stopped at
/// [`Limits::json_bytes`] so that a short link cannot unpack into unbounded
/// memory.
///
/// # Errors
///
/// The [`CodecError`] of the first step that fails.
pub fn decode(share: &str, limits: Limits) -> Result<String, CodecError> {
    if exceeds(share.len(), limits.text_bytes) {
        return Err(CodecError::Oversize(Oversize::Text {
            bytes: count(share.len()),
        }));
    }
    let deflated = URL_SAFE_NO_PAD_INDIFFERENT
        .decode(share)
        .map_err(|_| CodecError::Base64)?;
    let limit = usize::try_from(limits.json_bytes).unwrap_or(usize::MAX);
    let json = decompress_to_vec_with_limit(&deflated, limit).map_err(|error| {
        if error.status == TINFLStatus::HasMoreOutput {
            CodecError::Oversize(Oversize::Json { bytes: None })
        } else {
            CodecError::Deflate
        }
    })?;
    String::from_utf8(json).map_err(|_| CodecError::Utf8)
}

/// Checks a saved file's size against [`Limits::json_bytes`].
///
/// # Errors
///
/// [`Oversize::Json`] with the file's size.
pub fn check_file(json: &str, limits: Limits) -> Result<(), Oversize> {
    if exceeds(json.len(), limits.json_bytes) {
        return Err(Oversize::Json {
            bytes: Some(count(json.len())),
        });
    }
    Ok(())
}

/// Whether `len` is more than `limit`.
fn exceeds(len: usize, limit: u32) -> bool {
    u64::try_from(len).unwrap_or(u64::MAX) > u64::from(limit)
}

/// `n` as a size in bytes, saturating.
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    const LIMITS: Limits = Limits {
        text_bytes: 64,
        json_bytes: 256,
    };

    /// The share string of `bytes` deflated, whatever they hold.
    fn share_of(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(compress_to_vec(bytes, DEFLATE_LEVEL))
    }

    /// `len` letters that deflate poorly.
    fn scattered_letters(len: usize) -> String {
        let mut state = 12_345_u32;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                char::from(b'a' + u8::try_from((state >> 16) % 26).unwrap())
            })
            .collect()
    }

    #[test]
    fn a_share_string_decodes_to_the_json_it_encodes() {
        let json = r#"{"schema_version":1,"ops":[]}"#;
        assert_eq!(decode(&encode(json), LIMITS), Ok(json.to_owned()));
    }

    #[test]
    fn a_share_string_is_unpadded_base64url_and_the_same_every_time() {
        let json = r#"{"ops":[{"type":"step"},{"type":"step"},{"type":"step"}]}"#;
        let share = encode(json);
        assert_eq!(share, encode(json));
        assert!(share
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn decoding_accepts_padding() {
        let share = encode("{}");
        let padded = format!("{share}{}", "=".repeat((4 - share.len() % 4) % 4));
        assert_eq!(decode(&padded, LIMITS), Ok("{}".to_owned()));
    }

    #[test]
    fn text_that_is_not_base64url_is_refused() {
        assert_eq!(decode("not base64!", LIMITS), Err(CodecError::Base64));
    }

    #[test]
    fn bytes_that_are_not_deflated_are_refused() {
        let share = URL_SAFE_NO_PAD.encode(b"plain text, not a deflate stream");
        assert_eq!(decode(&share, LIMITS), Err(CodecError::Deflate));
    }

    #[test]
    fn deflated_bytes_that_are_not_utf8_are_refused() {
        assert_eq!(
            decode(&share_of(&[0xFF, 0xFE]), LIMITS),
            Err(CodecError::Utf8)
        );
    }

    #[test]
    fn a_share_string_over_the_text_limit_is_refused_before_it_is_read() {
        let long = "A".repeat(65);
        assert_eq!(
            decode(&long, LIMITS),
            Err(CodecError::Oversize(Oversize::Text { bytes: 65 }))
        );
    }

    #[test]
    fn a_share_string_that_inflates_past_the_json_limit_is_refused() {
        let json = "x".repeat(257);
        let share = encode(&json);
        assert!(share.len() <= 64, "the string must itself be short");
        assert_eq!(
            decode(&share, LIMITS),
            Err(CodecError::Oversize(Oversize::Json { bytes: None }))
        );
    }

    #[test]
    fn json_at_the_json_limit_links_and_one_byte_more_does_not() {
        let at = "x".repeat(256);
        assert_eq!(link(&at, LIMITS), Ok(encode(&at)));
        let over = "x".repeat(257);
        assert_eq!(
            link(&over, LIMITS),
            Err(Oversize::Json { bytes: Some(257) })
        );
    }

    #[test]
    fn json_whose_share_string_is_too_long_does_not_link() {
        let json = scattered_letters(200);
        let share = encode(&json);
        assert!(share.len() > 64, "the string must be over the text limit");
        assert_eq!(
            link(&json, LIMITS),
            Err(Oversize::Text {
                bytes: u32::try_from(share.len()).unwrap()
            })
        );
    }

    #[test]
    fn a_file_is_checked_against_the_json_limit_alone() {
        assert_eq!(check_file(&"x".repeat(256), LIMITS), Ok(()));
        assert_eq!(
            check_file(&"x".repeat(257), LIMITS),
            Err(Oversize::Json { bytes: Some(257) })
        );
    }
}
