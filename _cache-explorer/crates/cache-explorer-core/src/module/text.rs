//! Runs of styled text: the form in which a message that names a field or
//! quotes a value reaches the page, so the page sets the code in mono
//! without parsing anything.
//!
//! A guided module's text is written in a small part of markdown, read
//! here into the same runs: `` `code` `` and `*emphasis*`, and nothing
//! else. Any other markdown is an error, so an author sees it when the
//! module is read rather than as stray asterisks on the page. The text is
//! one paragraph: a line break, a heading, a list item, a block quote, an
//! indented code block and a rule are refused, and so are strong emphasis,
//! underscores, strikethrough, links, images, HTML, entities and backslash
//! escapes. Code is literal and cannot be empty; emphasis cannot hold code,
//! nor start or end with a space.

use std::fmt;

use serde::Serialize;
use thiserror::Error;

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

/// Why a module's text is not in the markdown it may use. `Display` is the
/// message, which a module error opens with the text's place in the module;
/// a character is counted from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum MarkupError {
    /// The text is empty, or only spaces.
    #[error("the text is empty.")]
    Empty,
    /// Markdown other than code and emphasis.
    #[error(
        "{what} at character {at} is not supported; module text allows only `code` and \
         *emphasis*."
    )]
    Unsupported {
        /// What the markdown is.
        what: Markdown,
        /// The character it starts at.
        at: u32,
    },
    /// A backtick with no backtick after it.
    #[error("the backtick at character {at} opens code that is never closed.")]
    UnclosedCode {
        /// The backtick's character.
        at: u32,
    },
    /// Two backticks together: empty code, or the fence or double
    /// backticks of fuller markdown.
    #[error("the backticks at character {at} hold no code; code takes one backtick on each side.")]
    EmptyCode {
        /// The first backtick's character.
        at: u32,
    },
    /// An asterisk with no asterisk after it.
    #[error(
        "the asterisk at character {at} opens emphasis that is never closed; put a literal \
         asterisk inside `code`."
    )]
    UnclosedEmphasis {
        /// The asterisk's character.
        at: u32,
    },
    /// Emphasis that starts or ends with a space.
    #[error(
        "the emphasis at character {at} starts or ends with a space; write *word*, not \
         * word *."
    )]
    SpacedEmphasis {
        /// The opening asterisk's character.
        at: u32,
    },
    /// Emphasis that holds code, which no style of segment can show.
    #[error(
        "the emphasis at character {at} holds code, which module text does not support; close \
         the emphasis first."
    )]
    CodeInEmphasis {
        /// The opening asterisk's character.
        at: u32,
    },
}

/// Markdown that module text does not support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Markdown {
    /// A line break: the text is one paragraph.
    LineBreak,
    /// A heading: `#` at the start.
    Heading,
    /// A list item: `-`, `+`, `*` or a number and `.` at the start.
    ListItem,
    /// A block quote: `>` at the start.
    BlockQuote,
    /// An indented code block: four spaces at the start, or a tab after
    /// fewer.
    IndentedCode,
    /// A horizontal rule: `---` alone.
    Rule,
    /// Strong emphasis: `**`.
    Strong,
    /// An underscore, which markdown reads as emphasis.
    Underscore,
    /// Strikethrough: `~~`.
    Strikethrough,
    /// A link: `[text](url)` or `[text][ref]`.
    Link,
    /// An image: `![text](url)`.
    Image,
    /// HTML: `<` and a tag.
    Html,
    /// An HTML entity: `&amp;`.
    Entity,
    /// A backslash escape: a backslash before a punctuation mark, which
    /// markdown would print without the backslash.
    Escape,
}

impl fmt::Display for Markdown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Markdown::LineBreak => "a line break",
            Markdown::Heading => "a heading",
            Markdown::ListItem => "a list item",
            Markdown::BlockQuote => "a block quote",
            Markdown::IndentedCode => "an indented code block",
            Markdown::Rule => "a horizontal rule",
            Markdown::Strong => "strong emphasis (`**`)",
            Markdown::Underscore => "an underscore outside `code`",
            Markdown::Strikethrough => "strikethrough (`~~`)",
            Markdown::Link => "a link",
            Markdown::Image => "an image",
            Markdown::Html => "HTML",
            Markdown::Entity => "an HTML entity",
            Markdown::Escape => "a backslash escape",
        })
    }
}

/// `text`, a module's text, read as segments: plain text, `` `code` `` in
/// mono and `*emphasis*` stressed.
///
/// # Errors
///
/// The [`MarkupError`] of the first thing in `text` that is not plain
/// text, code or emphasis, or of code or emphasis written wrongly.
pub(crate) fn markup(text: &str) -> Result<Vec<TextSegment>, MarkupError> {
    if text.trim().is_empty() {
        return Err(MarkupError::Empty);
    }
    let chars: Vec<char> = text.chars().collect();
    block_start(&chars)?;
    let mut segments = Segments::new();
    let mut plain = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let at = place(i);
        match c {
            '`' => {
                if chars.get(i.saturating_add(1)) == Some(&'`') {
                    return Err(MarkupError::EmptyCode { at });
                }
                let (code, close) = span(&chars, i, '`').ok_or(MarkupError::UnclosedCode { at })?;
                if let Some(line) = code.iter().position(|&c| is_line_break(c)) {
                    return Err(unsupported(Markdown::LineBreak, i.saturating_add(1) + line));
                }
                segments.push(TextStyle::Plain, &std::mem::take(&mut plain));
                segments.push(TextStyle::Code, &code.iter().collect::<String>());
                i = close.saturating_add(1);
            }
            '*' => {
                if chars.get(i.saturating_add(1)) == Some(&'*') {
                    return Err(unsupported(Markdown::Strong, i));
                }
                let (stressed, close) =
                    span(&chars, i, '*').ok_or(MarkupError::UnclosedEmphasis { at })?;
                let spaced = |c: Option<&char>| c.is_some_and(|c| c.is_whitespace());
                if spaced(stressed.first()) || spaced(stressed.last()) {
                    return Err(MarkupError::SpacedEmphasis { at });
                }
                if stressed.contains(&'`') {
                    return Err(MarkupError::CodeInEmphasis { at });
                }
                for index in i.saturating_add(1)..close {
                    inline(&chars, index)?;
                }
                segments.push(TextStyle::Plain, &std::mem::take(&mut plain));
                segments.push(TextStyle::Emphasis, &stressed.iter().collect::<String>());
                i = close.saturating_add(1);
            }
            _ => {
                inline(&chars, i)?;
                plain.push(c);
                i = i.saturating_add(1);
            }
        }
    }
    segments.push(TextStyle::Plain, &plain);
    Ok(segments.into_vec())
}

/// Refuses the markdown a paragraph can open with: an indented code block,
/// a heading, a block quote, a list item or a rule.
fn block_start(chars: &[char]) -> Result<(), MarkupError> {
    // Four columns of indentation make a code block. A tab goes on to the
    // next tab stop, four columns apart, so a tab after fewer spaces does
    // too.
    let spaces = chars.iter().take_while(|&&c| c == ' ').count();
    if spaces >= 4 || chars.get(spaces) == Some(&'\t') {
        return Err(unsupported(Markdown::IndentedCode, 0));
    }
    let rest = chars.get(spaces..).unwrap_or_default();
    let after = |n: usize| rest.get(n).is_none_or(|c| c.is_whitespace());
    let hashes = rest.iter().take_while(|&&c| c == '#').count();
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    let bullet = matches!(rest.first(), Some('-' | '+' | '*')) && after(1);
    let numbered = (1..=9).contains(&digits)
        && matches!(rest.get(digits), Some('.' | ')'))
        && after(digits.saturating_add(1));
    let what = if (1..=6).contains(&hashes) && after(hashes) {
        Some(Markdown::Heading)
    } else if rest.first() == Some(&'>') {
        Some(Markdown::BlockQuote)
    } else if rest.iter().all(|&c| c == '-' || c == ' ')
        && rest.iter().filter(|&&c| c == '-').count() >= 3
    {
        Some(Markdown::Rule)
    } else if bullet || numbered {
        Some(Markdown::ListItem)
    } else {
        None
    };
    match what {
        Some(what) => Err(unsupported(what, spaces)),
        None => Ok(()),
    }
}

/// Refuses the inline markdown that the character at `i` of `chars`
/// starts, other than code and emphasis: a line break, an underscore, a
/// backslash escape, strikethrough, an image, a link, HTML or an entity.
fn inline(chars: &[char], i: usize) -> Result<(), MarkupError> {
    let next = chars.get(i.saturating_add(1)).copied();
    let what = match chars.get(i) {
        Some(&c) if is_line_break(c) => Some(Markdown::LineBreak),
        Some('_') => Some(Markdown::Underscore),
        Some('\\') if next.is_some_and(|c| c.is_ascii_punctuation()) => Some(Markdown::Escape),
        Some('~') if next == Some('~') => Some(Markdown::Strikethrough),
        Some('!') if next == Some('[') => Some(Markdown::Image),
        Some('[') if links(chars, i) => Some(Markdown::Link),
        Some('<')
            if next.is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?')) =>
        {
            Some(Markdown::Html)
        }
        Some('&') if entity(chars, i) => Some(Markdown::Entity),
        _ => None,
    };
    match what {
        Some(what) => Err(unsupported(what, i)),
        None => Ok(()),
    }
}

/// Whether the `[` at `i` of `chars` opens a link: the `]` that closes it
/// is followed by `(` or by another `[`. Brackets pair as markdown pairs
/// them in a link's text: a pair may hold other pairs, and a bracket inside
/// code is text.
fn links(chars: &[char], i: usize) -> bool {
    let mut open = 0_u32;
    let mut at = i;
    while let Some(&c) = chars.get(at) {
        match c {
            // Code is passed over whole; a backtick that nothing closes is
            // text.
            '`' => at = span(chars, at, '`').map_or(at, |(_, close)| close),
            '[' => open = open.saturating_add(1),
            ']' => {
                open = open.saturating_sub(1);
                if open == 0 {
                    return matches!(chars.get(at.saturating_add(1)), Some('(' | '['));
                }
            }
            _ => {}
        }
        at = at.saturating_add(1);
    }
    false
}

/// Whether the `&` at `i` of `chars` starts an entity: letters, digits or
/// `#`, then `;`.
fn entity(chars: &[char], i: usize) -> bool {
    let rest = chars.get(i.saturating_add(1)..).unwrap_or_default();
    let name = rest
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '#')
        .count();
    name > 0 && rest.get(name) == Some(&';')
}

/// The characters between the delimiter at `open` of `chars` and the next
/// `delimiter`, and where that one is; none if no other follows.
fn span(chars: &[char], open: usize, delimiter: char) -> Option<(&[char], usize)> {
    let start = open.saturating_add(1);
    let close = chars
        .get(start..)?
        .iter()
        .position(|&c| c == delimiter)?
        .saturating_add(start);
    Some((chars.get(start..close)?, close))
}

/// Whether `c` breaks a line.
fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r')
}

/// The refusal of `what` starting at index `i`.
fn unsupported(what: Markdown, i: usize) -> MarkupError {
    MarkupError::Unsupported { what, at: place(i) }
}

/// The character number of index `i`, counted from 1.
fn place(i: usize) -> u32 {
    u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX)
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

    fn emphasis(text: &str) -> TextSegment {
        TextSegment {
            text: text.to_owned(),
            style: TextStyle::Emphasis,
        }
    }

    #[test]
    fn module_text_reads_code_and_emphasis() {
        assert_eq!(
            markup("Issue a load to `0x0A4` and watch the *tag compare*.").unwrap(),
            [
                plain("Issue a load to "),
                code("0x0A4"),
                plain(" and watch the "),
                emphasis("tag compare"),
                plain("."),
            ]
        );
        // Code is literal: markdown inside it is text.
        assert_eq!(
            markup("`ld r1, [0x0A4]` then `a_b*c`").unwrap(),
            [code("ld r1, [0x0A4]"), plain(" then "), code("a_b*c")]
        );
        assert_eq!(markup("*all*").unwrap(), [emphasis("all")]);
        // Characters that open no markdown are text.
        assert_eq!(
            markup("#5 hit: 3 < 4, 50% & (x) [0x0A4] ~1 !").unwrap(),
            [plain("#5 hit: 3 < 4, 50% & (x) [0x0A4] ~1 !")]
        );
        assert_eq!(
            markup("128 B lines. 0x0A4 = tag 0x01").unwrap(),
            [plain("128 B lines. 0x0A4 = tag 0x01")]
        );
        // Brackets that close no link, the pair first: a `](` that no
        // bracket opens, or one inside code, is text.
        assert_eq!(
            markup("[a] b](c) and `[d](e)`").unwrap(),
            [plain("[a] b](c) and "), code("[d](e)")]
        );
        assert_eq!(
            markup("[x `a](b` y] z").unwrap(),
            [plain("[x "), code("a](b"), plain(" y] z")]
        );
        assert_eq!(markup("  two spaces").unwrap(), [plain("  two spaces")]);
        // A backslash before anything but punctuation escapes nothing.
        assert_eq!(markup("C:\\temp \\").unwrap(), [plain("C:\\temp \\")]);
    }

    #[test]
    fn empty_text_is_refused() {
        assert_eq!(markup(""), Err(MarkupError::Empty));
        assert_eq!(markup("   "), Err(MarkupError::Empty));
    }

    #[test]
    fn code_and_emphasis_written_wrongly_are_refused() {
        assert_eq!(markup("a `b"), Err(MarkupError::UnclosedCode { at: 3 }));
        assert_eq!(markup("a ``b``"), Err(MarkupError::EmptyCode { at: 3 }));
        assert_eq!(markup("```rust"), Err(MarkupError::EmptyCode { at: 1 }));
        assert_eq!(markup("a *b"), Err(MarkupError::UnclosedEmphasis { at: 3 }));
        assert_eq!(
            markup("3 * 4"),
            Err(MarkupError::UnclosedEmphasis { at: 3 })
        );
        assert_eq!(markup("a * b*"), Err(MarkupError::SpacedEmphasis { at: 3 }));
        assert_eq!(markup("a *b *"), Err(MarkupError::SpacedEmphasis { at: 3 }));
        assert_eq!(
            markup("a *the `tag`*"),
            Err(MarkupError::CodeInEmphasis { at: 3 })
        );
    }

    #[test]
    fn other_markdown_is_refused_where_it_starts() {
        let unsupported = |what, at| Err(MarkupError::Unsupported { what, at });
        let cases = [
            ("one\ntwo", unsupported(Markdown::LineBreak, 4)),
            ("a `b\nc`", unsupported(Markdown::LineBreak, 5)),
            ("# Heading", unsupported(Markdown::Heading, 1)),
            ("  ### Heading", unsupported(Markdown::Heading, 3)),
            ("> quoted", unsupported(Markdown::BlockQuote, 1)),
            ("- item", unsupported(Markdown::ListItem, 1)),
            ("* item", unsupported(Markdown::ListItem, 1)),
            ("12. item", unsupported(Markdown::ListItem, 1)),
            ("3) item", unsupported(Markdown::ListItem, 1)),
            ("    indented", unsupported(Markdown::IndentedCode, 1)),
            ("\tindented", unsupported(Markdown::IndentedCode, 1)),
            // A tab reaches the next tab stop, four columns on.
            (" \tindented", unsupported(Markdown::IndentedCode, 1)),
            ("   \tindented", unsupported(Markdown::IndentedCode, 1)),
            ("---", unsupported(Markdown::Rule, 1)),
            ("a **b**", unsupported(Markdown::Strong, 3)),
            ("write_back", unsupported(Markdown::Underscore, 6)),
            ("a *b_c*", unsupported(Markdown::Underscore, 5)),
            ("a ~~b~~", unsupported(Markdown::Strikethrough, 3)),
            ("see [the log](#log)", unsupported(Markdown::Link, 5)),
            ("see [the log][1]", unsupported(Markdown::Link, 5)),
            // Link text may hold brackets in pairs, and code.
            ("see [a [b] c](d)", unsupported(Markdown::Link, 5)),
            ("see [a `]` b](d)", unsupported(Markdown::Link, 5)),
            ("see [a `b](d)", unsupported(Markdown::Link, 5)),
            ("an ![image](a.png)", unsupported(Markdown::Image, 4)),
            ("a <b>bold</b>", unsupported(Markdown::Html, 3)),
            ("a &amp; b", unsupported(Markdown::Entity, 3)),
            ("a \\* b", unsupported(Markdown::Escape, 3)),
            ("x\\_1", unsupported(Markdown::Escape, 2)),
        ];
        for (text, expected) in cases {
            assert_eq!(markup(text), expected, "{text:?}");
        }
    }

    #[test]
    fn markup_errors_name_the_character() {
        assert_eq!(
            MarkupError::Unsupported {
                what: Markdown::Strong,
                at: 3
            }
            .to_string(),
            "strong emphasis (`**`) at character 3 is not supported; module text allows only \
             `code` and *emphasis*."
        );
        assert_eq!(
            MarkupError::UnclosedCode { at: 7 }.to_string(),
            "the backtick at character 7 opens code that is never closed."
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
