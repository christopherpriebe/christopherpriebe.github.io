//! Programs and traces: every parse error with its line and column, each
//! parser limit one over its constant, the trace format, and programs
//! executed through the golden harness: labels, branches and loops, `r0`,
//! faults on addresses held in registers, the instruction budget, and loads
//! issued by hand beside a program; and, through the session, the session's
//! event limit reached inside a loop, and each instruction retiring at its
//! last event, with a cursor stopped inside one showing it in flight.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::golden::{Fixture, FixtureError, Issue};
use cache_explorer_core::limits::{
    INSTRUCTION_BUDGET, MAX_EVENTS_PER_ENTRY, MAX_LABEL_CHARS, MAX_PARSE_ERRORS, MAX_PROGRAM_BYTES,
    MAX_PROGRAM_INSTRUCTIONS, MAX_PROGRAM_LINES, SESSION_EVENT_LIMIT,
};
use cache_explorer_core::{
    AccessKind, ChangeRef, Config, EventKind, MicroEvent, Op, OpError, ParseError, ParseResult,
    Preset, ProgramFormat, Session, WordBytes,
};

use common::grouped;
use ProgramFormat::{Asm, Trace};

#[cfg(test)]
mod common;

/// The tests' address width: 12 bits, as in every design frame, so the
/// messages below read as the design's. It is set here rather than taken
/// from `Config::default()`, so changing the default does not break these
/// tests.
const ADDRESS_BITS: u8 = 12;

/// An error's line, column, length and message.
type Place = (u32, u32, u32, String);

/// `direct-mapped-8` at the tests' address width.
fn config() -> Config {
    Config {
        address_bits: ADDRESS_BITS,
        ..Preset::DirectMapped8.config()
    }
}

/// `config()` with words of `word_bytes`.
fn words(word_bytes: WordBytes) -> Config {
    Config {
        word_bytes,
        ..config()
    }
}

/// `source` parsed as `format` under `config`.
fn parse_under(
    config: &Config,
    source: &str,
    format: ProgramFormat,
) -> Result<ParseResult, String> {
    let geometry = config.validate().map_err(|error| error.to_string())?;
    Ok(geometry.parse_program(source, format))
}

/// `source` parsed as `format` under `config()`.
fn parse(source: &str, format: ProgramFormat) -> Result<ParseResult, String> {
    parse_under(&config(), source, format)
}

/// The errors of `source` parsed as `format` under `config`; none if it
/// parses.
fn errors_under(
    config: &Config,
    source: &str,
    format: ProgramFormat,
) -> Result<Vec<ParseError>, String> {
    Ok(match parse_under(config, source, format)? {
        ParseResult::Ok { .. } => Vec::new(),
        ParseResult::Invalid { errors } => errors,
    })
}

/// The only error of `source` parsed as `format` under `config`, as its
/// line, column, length and message. Its text is checked against them.
fn only_error(config: &Config, source: &str, format: ProgramFormat) -> Result<Place, String> {
    let errors = errors_under(config, source, format)?;
    let [error] = errors.as_slice() else {
        return Err(format!("not one error for {source:?}: {errors:?}"));
    };
    assert_eq!(
        error.text,
        format!(
            "Line {}, column {}: {}",
            error.line, error.column, error.message
        )
    );
    Ok((
        error.line,
        error.column,
        error.length,
        error.message.clone(),
    ))
}

/// The only error of the assembly `source` under `config()`.
fn asm_error(source: &str) -> Result<Place, String> {
    only_error(&config(), source, Asm)
}

/// The only error of the trace `source` under `config()`.
fn trace_error(source: &str) -> Result<Place, String> {
    only_error(&config(), source, Trace)
}

/// An error on line `line` at column `column`, `length` characters long.
fn place(line: u32, column: u32, length: u32, message: &str) -> Place {
    (line, column, length, message.to_owned())
}

/// A limit as a count of things to build.
fn count(limit: u32) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

/// "program" or "trace".
fn noun(format: ProgramFormat) -> &'static str {
    match format {
        Asm => "program",
        Trace => "trace",
    }
}

// ---------------------------------------------------------------------------
// The assembler.
// ---------------------------------------------------------------------------

/// The shape messages of the four instructions.
const LD: &str = "`ld` needs a register and an address, like `ld r1, [0x0A4]`.";
const ST: &str = "`st` needs an address and a register or a value, like `st [0x0A4], #7`.";
const ADD: &str = "`add` needs two registers and a register or a value, like `add r1, r1, #4`.";
const BNEZ: &str = "`bnez` needs a register and a label, like `bnez r1, loop`.";

#[test]
fn a_program_parses_into_its_count_and_first_instruction() {
    // The design's first-visit program.
    let sample = "start:\n  ld  r1, [0x0A4]\n  ld  r2, [0x0A8]\n  ld  r3, [0x1A4]\n  st  [0x0A4], \
                  #7\n  ld  r4, [0x0A4]";
    assert_eq!(
        parse(sample, Asm),
        Ok(ParseResult::Ok {
            instructions: 5,
            summary: "5 instructions \u{b7} next: ld r1, [0x0A4]".to_owned(),
        })
    );
    assert_eq!(
        parse("  add   r1,r0 ,\t#1 ; one", Asm),
        Ok(ParseResult::Ok {
            instructions: 1,
            summary: "1 instruction \u{b7} next: add r1,r0 , #1".to_owned(),
        })
    );
}

#[test]
fn a_parse_result_serializes_tagged_by_status() {
    let ok = serde_json::to_value(parse("ld r1, [0x0A4]", Asm).unwrap()).unwrap();
    assert_eq!(
        ok,
        serde_json::json!({
            "status": "ok",
            "instructions": 1,
            "summary": "1 instruction \u{b7} next: ld r1, [0x0A4]",
        })
    );
    let invalid = serde_json::to_value(parse("\nlw r1", Asm).unwrap()).unwrap();
    assert_eq!(
        invalid,
        serde_json::json!({
            "status": "invalid",
            "errors": [{
                "line": 2,
                "column": 1,
                "length": 2,
                "message": "unknown instruction `lw`; use ld, st, add or bnez.",
                "text": "Line 2, column 1: unknown instruction `lw`; use ld, st, add or bnez.",
            }],
        })
    );
    assert_eq!(serde_json::to_value(Asm).unwrap(), "asm");
    assert_eq!(serde_json::to_value(Trace).unwrap(), "trace");
}

#[test]
fn unknown_mnemonics_and_registers_have_their_own_messages() {
    let register = |name: &str| format!("`{name}` is not a register; use r0\u{2013}r7.");
    let cases = [
        (
            "start:\n  ld r1, [0x0A4]\n    lw r2, [0x0A8]",
            Ok(place(
                3,
                5,
                2,
                "unknown instruction `lw`; use ld, st, add or bnez.",
            )),
        ),
        ("ld r8, [0x0A4]", Ok(place(1, 4, 2, &register("r8")))),
        ("ld r1, [R10]", Ok(place(1, 9, 3, &register("R10")))),
        ("add r1, r1, r9", Ok(place(1, 13, 2, &register("r9")))),
        ("st [0x0A4], r01", Ok(place(1, 13, 3, &register("r01")))),
    ];
    for (source, expected) in cases {
        assert_eq!(asm_error(source), expected, "{source:?}");
    }
}

#[test]
fn a_literal_address_must_be_hex_within_the_width_and_aligned() {
    let beyond = |text: &str| format!("{text} does not fit a 12-bit address (0x000\u{2013}0xFFF).");
    let cases = [
        (
            "ld r1, [164]",
            Ok(place(1, 9, 3, "addresses are hex: write 0x0A4, not 164.")),
        ),
        ("ld r1, [0x1000]", Ok(place(1, 9, 6, &beyond("0x1000")))),
        (
            "st [0x100000000], #1",
            Ok(place(1, 5, 11, &beyond("0x100000000"))),
        ),
        (
            "ld r1, [0x10000000000000000]",
            Ok(place(
                1,
                9,
                19,
                "`0x10000000000000000` does not fit 64 bits.",
            )),
        ),
        (
            "ld r1, [0x0A6]",
            Ok(place(
                1,
                9,
                5,
                "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
            )),
        ),
        ("ld r1, [A4]", Ok(place(1, 9, 2, LD))),
    ];
    for (source, expected) in cases {
        assert_eq!(asm_error(source), expected, "{source:?}");
    }
}

#[test]
fn misshapen_operands_get_the_instruction_s_own_example() {
    let cases = [
        ("ld r1", Ok(place(1, 1, 2, LD))),
        ("ld r1, [0x0A4], r2", Ok(place(1, 15, 1, LD))),
        ("ld [0x0A4], r1", Ok(place(1, 4, 1, LD))),
        ("st [0x0A4], 7", Ok(place(1, 13, 1, ST))),
        ("st [0x0A4] #7", Ok(place(1, 12, 2, ST))),
        ("add r1, #1", Ok(place(1, 9, 2, ADD))),
        ("add r1, r1, #x", Ok(place(1, 13, 2, ADD))),
        ("bnez r1", Ok(place(1, 1, 4, BNEZ))),
        ("bnez r1, loop, r2", Ok(place(1, 14, 1, BNEZ))),
    ];
    for (source, expected) in cases {
        assert_eq!(asm_error(source), expected, "{source:?}");
    }
}

#[test]
fn labels_are_defined_once_named_like_labels_and_resolved() {
    let not_a_label = |text: &str| {
        format!(
            "`{text}` cannot be a label; start it with a letter or `_`, and use only letters, \
             digits and `_`."
        )
    };
    let cases = [
        (
            "add r1, r0, #1\nbnez r1, loop",
            Ok(place(2, 10, 4, "label `loop` is not defined.")),
        ),
        (
            "; count up\nloop: add r1, r1, #1\n  loop:  bnez r1, loop",
            Ok(place(3, 3, 4, "label `loop` is already defined on line 2.")),
        ),
        (
            "r1: add r1, r1, #1",
            Ok(place(
                1,
                1,
                2,
                "`r1` is a register, so it cannot be a label.",
            )),
        ),
        (
            "ADD: add r1, r1, #1",
            Ok(place(
                1,
                1,
                3,
                "`ADD` is an instruction, so it cannot be a label.",
            )),
        ),
        (
            "bnez r1, r2",
            Ok(place(
                1,
                10,
                2,
                "`r2` is a register, so it cannot be a label.",
            )),
        ),
        (
            "2go: add r1, r1, #1",
            Ok(place(1, 1, 3, &not_a_label("2go"))),
        ),
        ("bnez r1, 0", Ok(place(1, 10, 1, &not_a_label("0")))),
        ("", Ok(place(1, 1, 0, "the program has no instructions."))),
        (
            "; nothing but a comment\n\nend:",
            Ok(place(1, 1, 0, "the program has no instructions.")),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(asm_error(source), expected, "{source:?}");
    }
}

#[test]
fn an_immediate_must_fit_the_word_or_the_register() {
    let one_byte = words(WordBytes::One);
    let wide = |text: &str| format!("{text} does not fit a 1-byte word (\u{2212}128 to 255).");
    for (source, length, message) in [
        ("st [0x0A4], #300", 4, wide("300")),
        ("st [0x0A4], #-129", 5, wide("\u{2212}129")),
        ("st [0x0A4], #0x100", 6, wide("256")),
    ] {
        assert_eq!(
            only_error(&one_byte, source, Asm),
            Ok(place(1, 13, length, &message))
        );
    }
    // A store's immediate fits as a signed or an unsigned word, and an add's
    // may be any 64-bit pattern.
    for source in [
        "st [0x0A4], #255",
        "st [0x0A4], #-128",
        "st [0x0A4], #0xFF",
        "add r1, r1, #0xFFFFFFFFFFFFFFFF",
        "add r1, r1, #-9223372036854775808",
    ] {
        assert_eq!(
            errors_under(&one_byte, source, Asm),
            Ok(Vec::new()),
            "{source}"
        );
    }
    assert_eq!(
        asm_error("add r1, r1, #-9223372036854775809").map(|place| place.3),
        Ok(
            "\u{2212}9,223,372,036,854,775,809 does not fit an 8-byte word \
            (\u{2212}9,223,372,036,854,775,808 to 18,446,744,073,709,551,615)."
                .to_owned()
        )
    );
    assert_eq!(
        asm_error("add r1, r1, #18446744073709551616").map(|place| place.3),
        Ok("`#18446744073709551616` does not fit 64 bits.".to_owned())
    );
}

#[test]
fn a_line_reports_its_first_problem_and_the_errors_come_in_source_order() {
    let source = "ld r8, [0x0A6]\nbnez r1, nowhere\nlw\n\nst [0x0A4], #1\nadd r1, r1";
    let errors = errors_under(&config(), source, Asm).unwrap();
    let places: Vec<(u32, u32)> = errors
        .iter()
        .map(|error| (error.line, error.column))
        .collect();
    assert_eq!(places, [(1, 4), (2, 10), (3, 1), (6, 1)]);
}

#[test]
fn mnemonics_and_registers_are_read_in_either_case_and_labels_are_case_sensitive() {
    assert!(matches!(
        parse(
            "LD R1, [0X0a4]\nSt [r1], #0xff\nAdd r2, R2, #1\nloop: BNEZ r0, loop",
            Asm
        ),
        Ok(ParseResult::Ok {
            instructions: 4,
            ..
        })
    ));
    assert_eq!(
        asm_error("Loop: add r1, r1, #1\nbnez r1, loop"),
        Ok(place(2, 10, 4, "label `loop` is not defined."))
    );
}

#[test]
fn a_label_may_stand_alone_share_a_line_or_mark_the_end() {
    let source = "top:\n  add r1, r1, #1 ; count\nnext: bnez r0, top\n  bnez r1, end\n_end2:\nend:";
    assert!(matches!(
        parse(source, Asm),
        Ok(ParseResult::Ok {
            instructions: 3,
            ..
        })
    ));
}

#[test]
fn error_lengths_count_characters() {
    // "ldé" is four bytes and three characters.
    assert_eq!(
        asm_error("ld\u{e9} r1, [0x0A4]"),
        Ok(place(
            1,
            1,
            3,
            "unknown instruction `ld\u{e9}`; use ld, st, add or bnez."
        ))
    );
    // A comment may hold any character.
    assert_eq!(
        errors_under(&config(), "ld r1, [0x0A4] ; caf\u{e9} \u{2014} ok", Asm),
        Ok(Vec::new())
    );
}

#[test]
fn a_mnemonic_ends_where_its_word_does() {
    // `[` ends a word, so `st` needs no space before its address.
    assert_eq!(
        parse("st[0x0A4], #7", Asm),
        Ok(ParseResult::Ok {
            instructions: 1,
            summary: "1 instruction \u{b7} next: st[0x0A4], #7".to_owned(),
        })
    );
    // A register is a word, so `ld` needs a space or a tab before it.
    assert_eq!(
        asm_error("ldr1, [0x0A4]"),
        Ok(place(
            1,
            1,
            4,
            "unknown instruction `ldr1`; use ld, st, add or bnez."
        ))
    );
}

#[test]
fn examples_in_messages_follow_the_address_width_and_the_word() {
    let wide = Config {
        address_bits: 16,
        ..words(WordBytes::Eight)
    };
    let message = |source: &str| only_error(&wide, source, Asm).map(|place| place.3);
    assert_eq!(
        message("ld r1"),
        Ok("`ld` needs a register and an address, like `ld r1, [0x00A0]`.".to_owned())
    );
    assert_eq!(
        message("ld r1, [164]"),
        Ok("addresses are hex: write 0x00A4, not 164.".to_owned())
    );
    assert_eq!(
        message("ld r1, [0x00A4]"),
        Ok("0x00A4 is not aligned to an 8-byte word; use 0x00A0 or 0x00A8.".to_owned())
    );
}

// ---------------------------------------------------------------------------
// The trace parser.
// ---------------------------------------------------------------------------

/// The shape messages of the two accesses.
const R: &str = "`R` needs an address, like `R 0x0A4`.";
const W: &str = "`W` needs an address and a value, like `W 0x0A4 7`.";

#[test]
fn a_trace_parses_into_its_count_of_accesses() {
    // The design's sample trace.
    assert_eq!(
        parse("R 0x0A4\nR 0x0A8\nW 0x0A4 7\nR 0x1A4\nR 0x0A4", Trace),
        Ok(ParseResult::Ok {
            instructions: 5,
            summary: "5 accesses \u{b7} next: R 0x0A4".to_owned(),
        })
    );
    assert_eq!(
        parse("; one store\n  w\t0x0a8   0x2A  ; the answer\n", Trace),
        Ok(ParseResult::Ok {
            instructions: 1,
            summary: "1 access \u{b7} next: w 0x0a8 0x2A".to_owned(),
        })
    );
}

#[test]
fn a_trace_line_is_an_address_after_r_or_an_address_and_a_value_after_w() {
    let cases = [
        ("R", Ok(place(1, 1, 1, R))),
        ("R 0x0A4 7", Ok(place(1, 9, 1, R))),
        ("R [0x0A4]", Ok(place(1, 3, 1, R))),
        ("W", Ok(place(1, 1, 1, W))),
        (
            "W 0x0B0",
            Ok(place(1, 1, 1, "a store needs a value: W 0x0B0 7.")),
        ),
        ("W 0x0A4 -3", Ok(place(1, 9, 2, W))),
        ("W 0x0A4 7 8", Ok(place(1, 11, 1, W))),
        ("W 0x0A4 7\u{e9}", Ok(place(1, 9, 2, W))),
        (
            "R 0x0A4\nX 0x0A4",
            Ok(place(
                2,
                1,
                1,
                "unknown access `X`; use R to load or W to store.",
            )),
        ),
        (
            "ld r1, [0x0A4]",
            Ok(place(
                1,
                1,
                2,
                "unknown access `ld`; use R to load or W to store.",
            )),
        ),
        (
            "; nothing\n",
            Ok(place(1, 1, 0, "the trace has no accesses.")),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(trace_error(source), expected, "{source:?}");
    }
}

#[test]
fn a_trace_address_and_value_follow_the_assembler_s_rules() {
    let cases = [
        (
            "R 164",
            Ok(place(1, 3, 3, "addresses are hex: write 0x0A4, not 164.")),
        ),
        (
            "R 0x1000",
            Ok(place(
                1,
                3,
                6,
                "0x1000 does not fit a 12-bit address (0x000\u{2013}0xFFF).",
            )),
        ),
        (
            "W 0x0A6 1",
            Ok(place(
                1,
                3,
                5,
                "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
            )),
        ),
        (
            "W 0x0A4 99999999999999999999",
            Ok(place(
                1,
                9,
                20,
                "`99999999999999999999` does not fit 64 bits.",
            )),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(trace_error(source), expected, "{source:?}");
    }
    // A trace value has no sign, so the range it may take is unsigned.
    for (source, length, message) in [
        (
            "W 0x0A4 300",
            3,
            "300 does not fit a 1-byte word (0\u{2013}255).",
        ),
        (
            "W 0x0A4 0x100",
            5,
            "256 does not fit a 1-byte word (0\u{2013}255).",
        ),
    ] {
        assert_eq!(
            only_error(&words(WordBytes::One), source, Trace),
            Ok(place(1, 9, length, message)),
            "{source:?}"
        );
    }
    assert_eq!(
        errors_under(&words(WordBytes::One), "W 0x0A4 255", Trace),
        Ok(Vec::new())
    );
}

#[test]
fn a_trace_store_reports_its_value_before_a_word_after_it() {
    let cases = [
        ("W 0x0A4, 7", Ok(place(1, 8, 1, W))),
        ("W 0x0A4 abc 7", Ok(place(1, 9, 3, W))),
        (
            "W 0x0A4 99999999999999999999 7",
            Ok(place(
                1,
                9,
                20,
                "`99999999999999999999` does not fit 64 bits.",
            )),
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(trace_error(source), expected, "{source:?}");
    }
    assert_eq!(
        only_error(&words(WordBytes::One), "W 0x0A4 300 7", Trace),
        Ok(place(
            1,
            9,
            3,
            "300 does not fit a 1-byte word (0\u{2013}255)."
        ))
    );
}

// ---------------------------------------------------------------------------
// The parsers' limits.
// ---------------------------------------------------------------------------

#[test]
fn a_source_one_byte_over_the_limit_is_refused_where_it_crosses_it() {
    for (format, first) in [(Asm, "add r1, r1, #1 ;"), (Trace, "R 0x0A4 ;")] {
        let padding = "x".repeat(count(MAX_PROGRAM_BYTES) - first.len());
        let at_limit = format!("{first}{padding}");
        assert_eq!(at_limit.len(), count(MAX_PROGRAM_BYTES));
        assert_eq!(errors_under(&config(), &at_limit, format), Ok(Vec::new()));
        let message = format!(
            "the {} is larger than {} bytes.",
            noun(format),
            grouped(MAX_PROGRAM_BYTES)
        );
        assert_eq!(
            only_error(&config(), &format!("{at_limit}x"), format),
            Ok(place(1, MAX_PROGRAM_BYTES + 1, 1, &message))
        );
    }
}

#[test]
fn a_source_one_line_over_the_limit_is_refused_at_the_extra_line() {
    for (format, first) in [(Asm, "add r1, r1, #1"), (Trace, "R 0x0A4")] {
        let lines = |count: usize| {
            let mut lines = vec![first; 1];
            lines.resize(count, "; more");
            lines.join("\n")
        };
        let at_limit = lines(count(MAX_PROGRAM_LINES));
        assert!(
            at_limit.len() <= count(MAX_PROGRAM_BYTES),
            "the lines fit the byte limit"
        );
        assert_eq!(errors_under(&config(), &at_limit, format), Ok(Vec::new()));
        let message = format!(
            "the {} has more than {} lines.",
            noun(format),
            grouped(MAX_PROGRAM_LINES)
        );
        assert_eq!(
            only_error(&config(), &lines(count(MAX_PROGRAM_LINES) + 1), format),
            Ok(place(MAX_PROGRAM_LINES + 1, 1, 6, &message))
        );
    }
}

#[test]
fn a_program_one_instruction_over_the_limit_is_refused_at_the_extra_instruction() {
    let program = |count: usize| vec!["add r1, r1, #1"; count].join("\n");
    let at_limit = program(count(MAX_PROGRAM_INSTRUCTIONS));
    assert!(
        at_limit.len() <= count(MAX_PROGRAM_BYTES),
        "the program fits the byte limit"
    );
    assert!(
        matches!(
            parse(&at_limit, Asm),
            Ok(ParseResult::Ok { instructions, .. }) if instructions == MAX_PROGRAM_INSTRUCTIONS
        ),
        "one instruction a line fits the line limit"
    );
    let message = format!(
        "the program has more than {} instructions.",
        grouped(MAX_PROGRAM_INSTRUCTIONS)
    );
    // Reported once, at the first instruction over.
    assert_eq!(
        asm_error(&program(count(MAX_PROGRAM_INSTRUCTIONS) + 2)),
        Ok(place(MAX_PROGRAM_INSTRUCTIONS + 1, 1, 3, &message))
    );
}

#[test]
fn a_label_one_character_over_the_limit_is_refused() {
    let label = |length: usize| format!("L{}", "x".repeat(length - 1));
    let longest = label(count(MAX_LABEL_CHARS));
    assert_eq!(
        errors_under(&config(), &format!("{longest}: bnez r0, {longest}"), Asm),
        Ok(Vec::new())
    );
    let over = label(count(MAX_LABEL_CHARS) + 1);
    let message = format!(
        "a label has at most {} characters; this one has {}.",
        grouped(MAX_LABEL_CHARS),
        MAX_LABEL_CHARS + 1
    );
    assert_eq!(
        asm_error(&format!("{over}: add r1, r1, #1")),
        Ok(place(1, 1, MAX_LABEL_CHARS + 1, &message))
    );
}

#[test]
fn a_result_lists_at_most_the_limit_of_errors() {
    let source = vec!["lw"; count(MAX_PARSE_ERRORS) + 1].join("\n");
    let errors = errors_under(&config(), &source, Asm).unwrap();
    let lines: Vec<u32> = errors.iter().map(|error| error.line).collect();
    assert_eq!(lines, (1..=MAX_PARSE_ERRORS).collect::<Vec<u32>>());
    // Refused as an op, the program counts every error.
    let refused = run(&config(), &[program(&source)]);
    assert!(
        matches!(
            &refused,
            Err(FixtureError::Op(OpError::Program { errors, .. }))
                if *errors == MAX_PARSE_ERRORS + 1
        ),
        "{refused:?}"
    );
}

// ---------------------------------------------------------------------------
// Execution, through the golden harness.
// ---------------------------------------------------------------------------

/// The events of `issues` applied on `config` from the run's start.
fn run(config: &Config, issues: &[Issue]) -> Result<Vec<MicroEvent>, FixtureError> {
    Fixture {
        name: "isa",
        description: "",
        config: *config,
        issues: issues.to_vec(),
    }
    .events()
}

/// A load of the assembly program `source`.
fn program(source: &str) -> Issue {
    Issue::Program {
        source: source.to_owned(),
        format: Asm,
    }
}

/// A load of `source` as `program(source)`, then `steps` steps.
fn load_and_step(source: &str, steps: usize) -> Vec<Issue> {
    let mut issues = vec![program(source)];
    issues.resize(steps + 1, Issue::Step);
    issues
}

/// The explain line of each entry's last event, after the program load.
fn results(events: &[MicroEvent]) -> Vec<&str> {
    events
        .chunk_by(|a, b| a.entry == b.entry)
        .filter(|entry| entry.first().is_some_and(|event| event.entry.is_some()))
        .filter_map(|entry| entry.last().map(|event| event.explain.as_str()))
        .collect()
}

#[test]
fn labels_branches_and_loops_run_as_written() {
    let source =
        "        add r1, r0, #2\nloop:   add r2, r2, #10\n        add r1, r1, #-1\n        \
                  bnez r1, loop\n        bnez r2, done\n        add r3, r0, #1\ndone:";
    let events = run(&config(), &load_and_step(source, 8)).unwrap();
    assert_eq!(
        results(&events),
        [
            "add: r1 \u{2190} 2. No memory access.",
            "add: r2 \u{2190} 10. No memory access.",
            "add: r1 \u{2190} 1. No memory access.",
            "bnez: r1 is 1, branch taken to loop.",
            "add: r2 \u{2190} 20. No memory access.",
            "add: r1 \u{2190} 0. No memory access.",
            "bnez: r1 is 0, branch not taken.",
            "bnez: r2 is 20, branch taken to done.",
        ]
    );
    assert!(events
        .iter()
        .skip(1)
        .all(|event| event.kind == EventKind::Complete));
    // The branch to the label after the last instruction finished it.
    assert_eq!(
        run(&config(), &load_and_step(source, 9)),
        Err(FixtureError::Op(OpError::Finished))
    );
}

#[test]
fn r0_reads_zero_and_drops_what_is_written_to_it() {
    let source = "add r0, r0, #7\nadd r1, r0, #5\nld r0, [0x0A4]\nst [0x0A4], r0\nbnez r0, \
                  end\nend:\nld r2, [0x0A4]";
    let events = run(&config(), &load_and_step(source, 6)).unwrap();
    assert_eq!(
        results(&events),
        [
            "add: r0 always reads 0, so the result 7 is dropped.",
            "add: r1 \u{2190} 5. No memory access.",
            "Read word 1 of way 0 (53); r0 always reads 0, so the value is dropped.",
            "Write 0 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 is now stale.",
            "bnez: r0 is 0, branch not taken.",
            "Read word 1 of way 0 (0) into r2.",
        ]
    );
}

#[test]
fn an_address_held_in_a_register_faults_when_it_does_not_fit() {
    for (source, message) in [
        (
            "; walk\nadd r2, r0, #0xFF0\nadd r2, r2, #16\nld r1, [r2]",
            "Line 4: r2 holds 4096, which does not fit a 12-bit address. The program stopped.",
        ),
        (
            "; walk\nadd r2, r0, #0xA4\nadd r2, r2, #2\nst [r2], #1",
            "Line 4: r2 holds 166 (0x0A6), which is not aligned to a 4-byte word. The program \
             stopped.",
        ),
    ] {
        let events = run(&config(), &load_and_step(source, 3)).unwrap();
        let error = events.last().unwrap();
        assert_eq!(
            (error.label.as_str(), error.kind, error.explain.as_str()),
            ("3.1", EventKind::Error, message)
        );
        assert!(error.changed.is_empty());
        // The program has stopped, so the next step is refused.
        assert_eq!(
            run(&config(), &load_and_step(source, 4)),
            Err(FixtureError::Op(OpError::Stopped { line: 4 }))
        );
        // Loading it again starts it over.
        let mut again = load_and_step(source, 3);
        again.extend(load_and_step(source, 1));
        let events = run(&config(), &again).unwrap();
        assert_eq!(events.last().unwrap().kind, EventKind::Complete);
    }
}

#[test]
fn the_instruction_after_the_budget_is_replaced_by_an_error() {
    let source = "        add r1, r0, #1\nloop:   bnez r1, loop";
    let budget = count(INSTRUCTION_BUDGET);
    // Stepped to an access that never comes, in one op: stepped one op an
    // instruction, the log would hold more ops than a link can first.
    let to_the_end = [program(source), Issue::StepAccess];
    let events = run(&config(), &to_the_end).unwrap();
    let (last, executed) = events.split_last().unwrap();
    assert_eq!(
        executed.len(),
        1 + budget,
        "a setup and one event per instruction"
    );
    assert!(executed
        .iter()
        .skip(1)
        .all(|event| event.kind == EventKind::Complete));
    assert_eq!(last.entry, Some(INSTRUCTION_BUDGET + 1));
    assert_eq!(last.kind, EventKind::Error);
    assert_eq!(
        last.explain,
        format!(
            "The program used its budget of {} instructions and stopped. Is a loop missing its \
             exit?",
            grouped(INSTRUCTION_BUDGET)
        )
    );
    assert_eq!(
        run(
            &config(),
            &[to_the_end.to_vec(), vec![Issue::Step]].concat()
        ),
        Err(FixtureError::Op(OpError::Stopped { line: 2 }))
    );
}

#[test]
fn a_program_that_ends_on_the_last_instruction_of_its_budget_finishes() {
    // 1 + 2 × `n` instructions, and one more if the budget is even.
    let n = (INSTRUCTION_BUDGET - 1) / 2;
    let pad = if (INSTRUCTION_BUDGET - 1) % 2 == 1 {
        "\nadd r2, r2, #1"
    } else {
        ""
    };
    let source = format!("add r1, r0, #{n}\nloop: add r1, r1, #-1\nbnez r1, loop{pad}");
    let budget = count(INSTRUCTION_BUDGET);
    // In one op, as the loop makes no access.
    let to_the_end = [program(&source), Issue::StepAccess];
    let events = run(&config(), &to_the_end).unwrap();
    assert_eq!(
        events.len(),
        1 + budget,
        "a setup and one event per instruction"
    );
    assert!(events.iter().all(|event| event.kind != EventKind::Error));
    assert_eq!(
        run(
            &config(),
            &[to_the_end.to_vec(), vec![Issue::Step]].concat()
        ),
        Err(FixtureError::Op(OpError::Finished))
    );
}

#[test]
fn a_load_issued_by_hand_beside_a_program_overwrites_its_r1() {
    let geometry = config().validate().unwrap();
    let load = |at: u32| Issue::Load {
        addr: geometry.addr(at).unwrap(),
    };
    let issues = [
        program("add r1, r0, #5\nadd r2, r1, #0"),
        Issue::Step,
        load(0x0A4),
        Issue::Step,
        load(0x0A8),
    ];
    let events = run(&config(), &issues).unwrap();
    assert_eq!(
        results(&events),
        [
            "add: r1 \u{2190} 5. No memory access.",
            "Read word 1 of way 0 (53) into r1. A program is loaded, so its r1 now holds 53.",
            // The program reads the r1 the load wrote.
            "add: r2 \u{2190} 53. No memory access.",
            // It has finished, so a load by hand no longer mentions it.
            "Read word 2 of way 0 (66) into r1.",
        ]
    );
    // The load by hand moved no pc.
    assert!(events
        .iter()
        .filter(|event| event.entry == Some(2))
        .all(|event| !event.changed.contains(&ChangeRef::Pc)));
}

#[test]
fn a_load_issued_by_hand_beside_a_stopped_program_still_says_its_r1_changed() {
    let geometry = config().validate().unwrap();
    let load = Issue::Load {
        addr: geometry.addr(0x0A4).unwrap(),
    };
    // A program that faulted, and one that used its budget, are loaded and
    // have not finished: the load writes the r1 they show.
    for stopped in [
        load_and_step("add r2, r0, #0xA6\nld r1, [r2]", 2),
        vec![
            program("add r1, r0, #1\nloop: bnez r1, loop"),
            Issue::StepAccess,
        ],
    ] {
        let events = run(&config(), &[stopped, vec![load.clone()]].concat()).unwrap();
        assert!(events.iter().any(|event| event.kind == EventKind::Error));
        assert_eq!(
            events.last().map(|event| event.explain.as_str()),
            Some("Read word 1 of way 0 (53) into r1. A program is loaded, so its r1 now holds 53.")
        );
    }
}

#[test]
fn a_load_issued_by_hand_beside_a_trace_says_nothing_of_it() {
    // A trace reads no register, so the r1 the load writes is nothing it
    // uses, even with one of its two accesses still to come.
    let geometry = config().validate().unwrap();
    let issues = [
        Issue::Program {
            source: "R 0x0B0\nW 0x0B4 7".to_owned(),
            format: Trace,
        },
        Issue::Step,
        Issue::Load {
            addr: geometry.addr(0x0A4).unwrap(),
        },
    ];
    let events = run(&config(), &issues).unwrap();
    assert_eq!(
        events.last().map(|event| event.explain.as_str()),
        Some("Read word 1 of way 0 (53) into r1.")
    );
}

#[test]
fn a_trace_loads_into_r1_and_stores_its_values() {
    let issues = [
        Issue::Program {
            source: "R 0x0A4\nW 0x0A4 0x2A\nR 0x0A4".to_owned(),
            format: Trace,
        },
        Issue::Step,
        Issue::Step,
        Issue::Step,
    ];
    let events = run(&config(), &issues).unwrap();
    assert_eq!(
        events[0].explain,
        "Loaded a trace of 3 accesses. Registers and the pc are reset; the cache and memory \
         keep their contents."
    );
    assert_eq!(
        results(&events),
        [
            "Read word 1 of way 0 (53) into r1.",
            "Write 42 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 is now stale.",
            "Read word 1 of way 0 (42) into r1.",
        ]
    );
}

#[test]
fn a_program_that_does_not_parse_or_a_step_with_nothing_to_run_is_refused() {
    assert_eq!(
        run(&config(), &[Issue::Step]),
        Err(FixtureError::Op(OpError::NoProgram))
    );
    let refused = run(&config(), &[program("ld r1, [0x0A4]\nlw r2\nst r1")])
        .map_err(|error| error.to_string());
    assert_eq!(
        refused,
        Err(
            "The program has 2 errors; the first is on line 2: unknown instruction `lw`; use \
             ld, st, add or bnez."
                .to_owned()
        )
    );
    let trace = Issue::Program {
        source: "R 164".to_owned(),
        format: Trace,
    };
    assert_eq!(
        run(&config(), &[trace]).map_err(|error| error.to_string()),
        Err(
            "The trace has an error on line 1: addresses are hex: write 0x0A4, not 164.".to_owned()
        )
    );
}

#[test]
fn the_session_limit_stops_a_loop_with_an_error() {
    let mut session = Session::with_config(config()).unwrap();
    let endless = Op::Program {
        source: "        add r1, r0, #1\nloop:   bnez r1, loop".to_owned(),
        format: Asm,
    };
    let budget_error = format!(
        "The program used its budget of {} instructions and stopped. Is a loop missing its exit?",
        grouped(INSTRUCTION_BUDGET)
    );
    // Each pair of ops loads the loop and runs it to its budget: the load's
    // setup, the budget's instructions and its error. The last pair must
    // still find room for its load; a load by hand first shifts the pairs off
    // the one alignment where it would not.
    let pair = INSTRUCTION_BUDGET + 2;
    if (SESSION_EVENT_LIMIT - MAX_EVENTS_PER_ENTRY + 1) % pair == 0 {
        let geometry = config().validate().unwrap();
        session
            .apply(Op::Issue {
                kind: AccessKind::Read,
                addr: geometry.addr(0x0A4).unwrap(),
                value: None,
            })
            .unwrap();
    }
    let mut last_op = Vec::new();
    for _ in 0..=SESSION_EVENT_LIMIT / pair {
        session.apply(endless.clone()).unwrap();
        last_op = session.apply(Op::StepAccess).unwrap().to_vec();
        if last_op.last().map(|event| &event.explain) != Some(&budget_error) {
            break;
        }
    }
    let (error, executed) = last_op.split_last().unwrap();
    assert!(!executed.is_empty());
    assert!(executed
        .iter()
        .all(|event| event.kind == EventKind::Complete));
    assert_eq!(error.kind, EventKind::Error);
    assert_eq!(
        error.explain,
        format!(
            "The session reached its limit of {} steps, so the program stopped here.",
            grouped(SESSION_EVENT_LIMIT)
        )
    );
    assert!(error.changed.is_empty());
    assert_eq!(error.addr, None);
    // The error is an entry of its own, after the last instruction.
    assert_eq!(
        error.entry,
        executed
            .last()
            .and_then(|event| event.entry)
            .map(|entry| entry + 1)
    );
    assert_eq!(error.seq, 1);
    // The log stays within the limit, and has no room for another entry.
    let stored = u32::try_from(error.id + 1).unwrap();
    assert!(stored <= SESSION_EVENT_LIMIT);
    assert!(stored + MAX_EVENTS_PER_ENTRY > SESSION_EVENT_LIMIT);
    assert_eq!(session.apply(Op::Step), Err(OpError::SessionFull));
    assert_eq!(session.apply(endless), Err(OpError::SessionFull));
}

// ---------------------------------------------------------------------------
// Execution, through the session's snapshot.
// ---------------------------------------------------------------------------

#[test]
fn an_instruction_retires_at_its_last_event_and_a_cursor_inside_it_shows_it_in_flight() {
    let ops = [
        Op::Program {
            source: "ld r1, [0x0A4]\nadd r2, r1, #1".to_owned(),
            format: Asm,
        },
        Op::Run { n: 2 },
    ];
    let mut session = Session::with_config(config()).unwrap();
    for op in ops.clone() {
        session.apply(op).unwrap();
    }
    let events = session.events().to_vec();
    let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds,
        [
            EventKind::Setup,
            EventKind::Decode,
            EventKind::Miss,
            EventKind::Victim,
            EventKind::Fill,
            EventKind::Complete,
            EventKind::Complete,
        ]
    );
    // At every event, the drawer's line, whether it is in flight, and how
    // many instructions have retired; the machine is the one the fold
    // stopped after that event gives, inside the `Run` op as at its end.
    let mut shown = Vec::new();
    for at in 1..=u32::try_from(events.len()).unwrap() {
        let pos = serde_json::from_value(at.into()).unwrap();
        session.seek(pos);
        assert_eq!(
            session.machine(),
            &Session::replay_to(&config(), &ops, pos).unwrap(),
            "at {at}"
        );
        let program = session.state().program.unwrap();
        shown.push((program.current_line, program.executing, program.executed));
    }
    assert_eq!(
        shown,
        [
            // Loaded: the first line is next.
            (Some(1), false, 0),
            // The `ld` in flight: its line stays until it retires.
            (Some(1), true, 0),
            (Some(1), true, 0),
            (Some(1), true, 0),
            (Some(1), true, 0),
            // Its last event retires it.
            (Some(2), false, 1),
            // The `add` retires at its one event, and the program is done.
            (None, false, 2),
        ]
    );
    // Only an instruction's last event lists the pc.
    let pc: Vec<bool> = events
        .iter()
        .map(|event| event.changed.contains(&ChangeRef::Pc))
        .collect();
    assert_eq!(pc, [false, false, false, false, false, true, true]);
}
