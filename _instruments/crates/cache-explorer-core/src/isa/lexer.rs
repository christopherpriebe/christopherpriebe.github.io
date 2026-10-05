//! The lexer the assemblers and the trace parsers share: a line split into
//! tokens, each with the column the program drawer underlines, and the
//! readings of a token as a number or a label.
//!
//! A comment runs from the syntax's comment mark to the end of the line.
//! Spaces and tabs separate tokens, as do the punctuation marks `,` and `:`
//! and the syntax's pair of brackets, which are tokens of their own.
//! Everything else belongs to a word: a mnemonic, a register, a number, an
//! immediate or a label. Columns are 1-based and counted in Unicode scalar
//! values, so a line with an accented comment underlines the same characters
//! the student sees.

use std::sync::Arc;

/// The marks of a language that differ from one instruction set to the
/// next: where a comment starts and what brackets a memory operand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syntax {
    /// The mark that starts a comment, which runs to the end of the line.
    pub comment: char,
    /// The mark that opens a memory operand.
    pub open: char,
    /// The mark that closes one.
    pub close: char,
}

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// A run of characters other than spaces, tabs and the marks below.
    Word,
    /// `,`, between operands.
    Comma,
    /// The syntax's opening bracket, which opens a memory operand.
    Open,
    /// Its closing bracket, which closes one.
    Close,
    /// `:`, which ends a label.
    Colon,
}

/// One token of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token<'s> {
    /// What the token is.
    pub kind: TokenKind,
    /// The token as written.
    pub text: &'s str,
    /// The column of its first character, from 1.
    pub column: u32,
    /// Its length in characters.
    pub length: u32,
    /// The byte offset of its first character within the line.
    pub start: usize,
    /// The byte offset just past its last character.
    pub end: usize,
}

/// What a word reads as where a register may stand, for an instruction set
/// whose registers are of type `R`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterWord<R> {
    /// A register.
    Register(R),
    /// A word shaped like a register's name that names none, such as `r8`.
    NotARegister,
    /// Anything else.
    Other,
}

/// What a word reads as where a number may stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberWord {
    /// `0x` or `0X` and hex digits.
    Hex(Option<u64>),
    /// Decimal digits.
    Decimal(Option<u64>),
    /// Anything else.
    Other,
}

/// The tokens of `line`, up to its comment, in `syntax`.
pub fn tokens(line: &str, syntax: Syntax) -> Vec<Token<'_>> {
    let code = line.split(syntax.comment).next().unwrap_or_default();
    let mut tokens = Vec::new();
    // The word being read: its first byte offset and column.
    let mut word: Option<(usize, u32)> = None;
    let mut column = 0_u32;
    for (offset, character) in code.char_indices() {
        column = column.saturating_add(1);
        let kind = match character {
            ',' => Some(TokenKind::Comma),
            ':' => Some(TokenKind::Colon),
            _ if character == syntax.open => Some(TokenKind::Open),
            _ if character == syntax.close => Some(TokenKind::Close),
            _ => None,
        };
        let separates = kind.is_some() || character == ' ' || character == '\t';
        if separates {
            if let Some((start, first)) = word.take() {
                tokens.push(token(line, TokenKind::Word, start, offset, first, column));
            }
        } else if word.is_none() {
            word = Some((offset, column));
        }
        if let Some(kind) = kind {
            let end = offset + character.len_utf8();
            tokens.push(token(
                line,
                kind,
                offset,
                end,
                column,
                column.saturating_add(1),
            ));
        }
    }
    if let Some((start, first)) = word {
        tokens.push(token(
            line,
            TokenKind::Word,
            start,
            code.len(),
            first,
            column.saturating_add(1),
        ));
    }
    tokens
}

/// The token of `kind` from byte `start` to byte `end` of `line`, which
/// starts at column `first` and ends before column `next`.
fn token(
    line: &str,
    kind: TokenKind,
    start: usize,
    end: usize,
    first: u32,
    next: u32,
) -> Token<'_> {
    Token {
        kind,
        text: line.get(start..end).unwrap_or_default(),
        column: first,
        length: next.saturating_sub(first),
        start,
        end,
    }
}

/// `text` with every run of spaces and tabs made one space, as the drawer's
/// summary and the CPU box show an instruction. The text is shared: the
/// program keeps it, and so does the log entry the instruction makes.
pub fn collapse(text: &str) -> Arc<str> {
    let mut collapsed = String::with_capacity(text.len());
    for part in text.split([' ', '\t']).filter(|part| !part.is_empty()) {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(part);
    }
    collapsed.into()
}

impl Token<'_> {
    /// Whether the token is a word written `name` in any case.
    pub fn is(&self, name: &str) -> bool {
        self.kind == TokenKind::Word && self.text.eq_ignore_ascii_case(name)
    }

    /// The token read as a number: its value, or `None` for one that does
    /// not fit 64 bits.
    pub fn number(&self) -> NumberWord {
        if self.kind != TokenKind::Word {
            return NumberWord::Other;
        }
        number(self.text)
    }

    /// Whether the token is a label as the grammar spells one: a letter or
    /// `_`, then letters, digits and `_`. Letters are ASCII.
    pub fn is_label(&self) -> bool {
        let mut characters = self.text.chars();
        self.kind == TokenKind::Word
            && characters
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && characters.all(|rest| rest.is_ascii_alphanumeric() || rest == '_')
    }
}

/// `text` read as a number, hex after `0x` or `0X`, decimal otherwise.
pub fn number(text: &str) -> NumberWord {
    if let Some(digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        if !digits.is_empty() && digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return NumberWord::Hex(value(digits, 16));
        }
        return NumberWord::Other;
    }
    if is_digits(text) {
        return NumberWord::Decimal(value(text, 10));
    }
    NumberWord::Other
}

/// Whether `text` is one or more ASCII digits.
pub fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// The number the digits of `text` spell in `radix`, or `None` if it does
/// not fit 64 bits. Every character must already be a digit of the radix.
fn value(text: &str, radix: u32) -> Option<u64> {
    text.chars().try_fold(0_u64, |number, digit| {
        number
            .checked_mul(u64::from(radix))?
            .checked_add(u64::from(digit.to_digit(radix)?))
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    /// The tiny language's marks.
    const TINY: Syntax = Syntax {
        comment: ';',
        open: '[',
        close: ']',
    };

    /// Each token's kind, text, column and length.
    fn read(line: &str) -> Vec<(TokenKind, &str, u32, u32)> {
        tokens(line, TINY)
            .iter()
            .map(|token| (token.kind, token.text, token.column, token.length))
            .collect()
    }

    #[test]
    fn words_and_marks_are_tokens_with_their_columns() {
        use TokenKind::{Close, Colon, Comma, Open, Word};
        assert_eq!(
            read("loop: ld r1, [0x0A4] ; the first load"),
            [
                (Word, "loop", 1, 4),
                (Colon, ":", 5, 1),
                (Word, "ld", 7, 2),
                (Word, "r1", 10, 2),
                (Comma, ",", 12, 1),
                (Open, "[", 14, 1),
                (Word, "0x0A4", 15, 5),
                (Close, "]", 20, 1),
            ]
        );
        assert_eq!(
            read("\tst\t[r2],#-3"),
            [
                (Word, "st", 2, 2),
                (Open, "[", 5, 1),
                (Word, "r2", 6, 2),
                (Close, "]", 8, 1),
                (Comma, ",", 9, 1),
                (Word, "#-3", 10, 3),
            ]
        );
    }

    #[test]
    fn a_comment_ends_the_line() {
        assert!(tokens("; nothing but a comment", TINY).is_empty());
        assert!(tokens("   \t ", TINY).is_empty());
        assert_eq!(read("R 0x0A4;load"), read("R 0x0A4"));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // "é" is two bytes but one column.
        assert_eq!(
            read("é ld"),
            [(TokenKind::Word, "é", 1, 1), (TokenKind::Word, "ld", 3, 2)]
        );
        let token = tokens("x\u{e9}y z", TINY).first().copied().unwrap();
        assert_eq!(
            (token.text, token.length, token.start, token.end),
            ("x\u{e9}y", 3, 0, 4)
        );
    }

    #[test]
    fn runs_of_spaces_and_tabs_collapse_to_one_space() {
        assert_eq!(&*collapse("ld  r1,\t[0x0A4]"), "ld r1, [0x0A4]");
        assert_eq!(&*collapse("  W 0x0A4   7 "), "W 0x0A4 7");
    }

    #[test]
    fn numbers_are_hex_after_0x_and_decimal_otherwise() {
        assert_eq!(number("0x0A4"), NumberWord::Hex(Some(0xA4)));
        assert_eq!(number("0XffFF"), NumberWord::Hex(Some(0xFFFF)));
        assert_eq!(number("164"), NumberWord::Decimal(Some(164)));
        assert_eq!(
            number("0xFFFFFFFFFFFFFFFF"),
            NumberWord::Hex(Some(u64::MAX))
        );
        assert_eq!(number("0x10000000000000000"), NumberWord::Hex(None));
        assert_eq!(
            number("18446744073709551615"),
            NumberWord::Decimal(Some(u64::MAX))
        );
        assert_eq!(number("18446744073709551616"), NumberWord::Decimal(None));
        for text in ["0x", "0xG", "A4", "-3", "#3", "1.5", ""] {
            assert_eq!(number(text), NumberWord::Other, "{text}");
        }
    }

    #[test]
    fn labels_start_with_a_letter_or_underscore() {
        let is_label = |text: &str| tokens(text, TINY).first().is_some_and(Token::is_label);
        for text in ["loop", "_x", "L2", "end_of_it"] {
            assert!(is_label(text), "{text}");
        }
        for text in ["2x", "my-loop", "caf\u{e9}", "#1"] {
            assert!(!is_label(text), "{text}");
        }
    }

    /// Each token's kind and text.
    fn kinds(line: &str, syntax: Syntax) -> Vec<(TokenKind, &str)> {
        tokens(line, syntax)
            .iter()
            .map(|token| (token.kind, token.text))
            .collect()
    }

    #[test]
    fn the_syntax_chooses_the_comment_and_the_brackets() {
        use TokenKind::{Close, Open, Word};
        let paren = Syntax {
            comment: '#',
            open: '(',
            close: ')',
        };
        assert_eq!(
            kinds("lw a0, 4(sp) # load", paren),
            [
                (Word, "lw"),
                (Word, "a0"),
                (TokenKind::Comma, ","),
                (Word, "4"),
                (Open, "("),
                (Word, "sp"),
                (Close, ")"),
            ]
        );
        // The same line in the tiny syntax: no brackets, and the comment starts at `;`.
        assert_eq!(
            kinds("lw a0, 4(sp) # load", TINY),
            [
                (Word, "lw"),
                (Word, "a0"),
                (TokenKind::Comma, ","),
                (Word, "4(sp)"),
                (Word, "#"),
                (Word, "load"),
            ]
        );
    }
}
