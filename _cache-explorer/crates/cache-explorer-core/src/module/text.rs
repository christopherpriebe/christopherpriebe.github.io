//! Runs of styled text: the form in which a message that names a field or
//! quotes a value reaches the page, so the page sets the code in mono
//! without parsing anything.

use serde::Serialize;

/// One run of text in one style.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct TextSegment {
    /// The text, as the page shows it.
    pub text: String,
    /// How the page sets it.
    pub style: TextStyle,
}

/// How a run of text is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum TextStyle {
    /// Running text.
    Plain,
    /// A field, a value or a name, set in mono.
    Code,
    /// Stressed text.
    Emphasis,
}

/// Segments being written one run at a time. A run in the style of the one
/// before it joins it, and an empty run is dropped, so the page receives
/// each stretch of one style as a single segment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Segments(Vec<TextSegment>);

impl Segments {
    /// No segments yet.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// These segments, then `text` as running text.
    #[must_use]
    pub(crate) fn plain(mut self, text: &str) -> Self {
        self.push(TextStyle::Plain, text);
        self
    }

    /// These segments, then `text` in mono.
    #[must_use]
    pub(crate) fn code(mut self, text: &str) -> Self {
        self.push(TextStyle::Code, text);
        self
    }

    /// These segments, then `segments`, joined as [`Segments::push`] joins
    /// them.
    #[must_use]
    pub(crate) fn then(mut self, segments: Vec<TextSegment>) -> Self {
        for segment in segments {
            self.push(segment.style, &segment.text);
        }
        self
    }

    /// Adds `text` in `style`: to the last segment when it has that style,
    /// and as a new segment otherwise; nothing when `text` is empty.
    pub(crate) fn push(&mut self, style: TextStyle, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.0.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => self.0.push(TextSegment {
                text: text.to_owned(),
                style,
            }),
        }
    }

    /// The segments.
    pub(crate) fn into_vec(self) -> Vec<TextSegment> {
        self.0
    }
}

/// `text` with every stretch it quotes between backticks in mono, the
/// backticks dropped, as serde and the core's messages quote a field or a
/// name: ``unknown field `ways` `` becomes `unknown field ` and `ways` in
/// mono. A backtick opens a stretch only where it follows no letter, digit,
/// `_` or backtick, and closes one only where none follows it, so a name
/// quoted as it came keeps a backtick of its own: ``unknown field `a`b` ``
/// sets ``a`b`` in mono. A backtick that opens no stretch, or opens one
/// that nothing closes, stays as it is.
pub(crate) fn code_spans(text: &str) -> Vec<TextSegment> {
    let mut segments = Segments::new();
    // Where the running text starts, and where the open stretch of code
    // starts, after its backtick, if one is open.
    let mut plain = 0;
    let mut open: Option<usize> = None;
    let mut before: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c == '`' {
            let after = chars.peek().map(|&(_, next)| next);
            match open {
                None if !in_word(before) => {
                    segments.push(TextStyle::Plain, text.get(plain..at).unwrap_or_default());
                    open = Some(at.saturating_add(1));
                }
                Some(start) if !in_word(after) => {
                    segments.push(TextStyle::Code, text.get(start..at).unwrap_or_default());
                    open = None;
                    plain = at.saturating_add(1);
                }
                _ => {}
            }
        }
        before = Some(c);
    }
    // A stretch that nothing closes is running text, its backtick included.
    let rest = open.map_or(plain, |start| start.saturating_sub(1));
    segments.push(TextStyle::Plain, text.get(rest..).unwrap_or_default());
    segments.into_vec()
}

/// Whether `c`, beside a backtick, is part of a word, so that the backtick
/// cannot open or close a stretch of code there: a letter, a digit, `_` or
/// another backtick. The edge of the text, `None`, is not.
fn in_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '`')
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    fn plain(text: &str) -> TextSegment {
        TextSegment {
            text: text.to_owned(),
            style: TextStyle::Plain,
        }
    }

    fn code(text: &str) -> TextSegment {
        TextSegment {
            text: text.to_owned(),
            style: TextStyle::Code,
        }
    }

    #[test]
    fn runs_of_one_style_join_and_empty_runs_are_dropped() {
        let segments = Segments::new()
            .code("associativity: 3")
            .plain("")
            .plain(" is not a power of two.")
            .plain(" The 6 steps after it were not replayed.")
            .code("")
            .into_vec();
        assert_eq!(
            segments,
            [
                code("associativity: 3"),
                plain(" is not a power of two. The 6 steps after it were not replayed.")
            ]
        );
        let joined = Segments::new()
            .plain("Line 1: ")
            .then(vec![plain("unknown "), code("ways")])
            .code("!")
            .into_vec();
        assert_eq!(joined, [plain("Line 1: unknown "), code("ways!")]);
    }

    #[test]
    fn backticks_mark_code() {
        assert_eq!(
            code_spans("unknown field `ways`, expected one of `sizeBytes`, `lineBytes`"),
            [
                plain("unknown field "),
                code("ways"),
                plain(", expected one of "),
                code("sizeBytes"),
                plain(", "),
                code("lineBytes"),
            ]
        );
        assert_eq!(code_spans("`n`"), [code("n")]);
        assert_eq!(code_spans("no code here."), [plain("no code here.")]);
        assert_eq!(code_spans(""), []);
    }

    #[test]
    fn a_backtick_without_a_partner_stays() {
        assert_eq!(
            code_spans("a `b` c `d"),
            [plain("a "), code("b"), plain(" c `d")]
        );
        assert_eq!(code_spans("`"), [plain("`")]);
    }

    #[test]
    fn a_backtick_inside_a_quoted_name_stays_in_it() {
        // serde quotes a name as it came, backticks and all.
        assert_eq!(
            code_spans("unknown field `a`b`, expected one of `schema_version`, `config`"),
            [
                plain("unknown field "),
                code("a`b"),
                plain(", expected one of "),
                code("schema_version"),
                plain(", "),
                code("config"),
            ]
        );
        assert_eq!(
            code_spans("unknown field ``a``, expected `at`"),
            [
                plain("unknown field "),
                code("`a`"),
                plain(", expected "),
                code("at"),
            ]
        );
        // One that ends a name early spoils that name alone.
        assert_eq!(
            code_spans("unknown field `a` b`, expected one of `at`"),
            [
                plain("unknown field "),
                code("a"),
                plain(" b`, expected one of "),
                code("at"),
            ]
        );
    }

    #[test]
    fn segments_serialize_with_their_style() {
        assert_eq!(
            serde_json::to_string(&code("ways")).unwrap(),
            "{\"text\":\"ways\",\"style\":\"code\"}"
        );
        assert_eq!(
            serde_json::to_string(&TextStyle::Emphasis).unwrap(),
            "\"emphasis\""
        );
    }
}
