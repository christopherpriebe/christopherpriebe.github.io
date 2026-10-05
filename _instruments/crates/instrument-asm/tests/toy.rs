//! A second instruction set read through the toolkit's public interface
//! alone: a small RISC-V flavoured language, with the needs of RV32I as its
//! checklist. It has 32 registers with their ABI names, comments that start
//! at `#`, memory operands in the form `offset(base)`, 12-bit immediates,
//! branches that hold a byte offset to a label, `li`, a pseudo-instruction
//! that is sometimes two instructions, and the 32-bit encodings of its
//! instructions. Its branches reach a few instructions only, so a short
//! program can be out of reach.

#![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

use instrument_asm::lexer::{self, NumberWord, RegisterWord, Syntax, Token, TokenKind};
use instrument_asm::program::Program;
use instrument_asm::set::{
    Context, Encode, InstructionSet, Limits, Problem, Read, Reason, RegisterFile,
};

/// The language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Toy {}

/// How a source is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// Assembly, with labels.
    Asm,
    /// One instruction a line and no labels.
    Flat,
}

/// A register, by its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reg(u32);

/// An instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Op {
    /// `addi rd, rs1, imm`.
    Addi { rd: Reg, rs1: Reg, imm: i64 },
    /// `lui rd, imm`, whose immediate is 20 bits.
    Lui { rd: Reg, imm: i64 },
    /// `lw rd, offset(base)`.
    Lw { rd: Reg, base: Reg, offset: i64 },
    /// `beq rs1, rs2, label`, whose offset is in bytes from the instruction.
    Beq { rs1: Reg, rs2: Reg, offset: i64 },
}

/// The farthest a branch reaches, in bytes, either way.
const REACH: i64 = 12;

/// The ABI names of the first registers.
const ABI: [(&str, u32); 3] = [("zero", 0), ("ra", 1), ("sp", 2)];

impl RegisterFile for Toy {
    type Register = Reg;

    const COUNT: u32 = 32;

    fn read(token: &Token<'_>) -> RegisterWord<Reg> {
        if token.kind != TokenKind::Word {
            return RegisterWord::Other;
        }
        if let Some((_, number)) = ABI.iter().find(|(name, _)| token.is(name)) {
            return RegisterWord::Register(Reg(*number));
        }
        let Some(digits) = token.text.strip_prefix(['x', 'X']) else {
            return RegisterWord::Other;
        };
        if !lexer::is_digits(digits) {
            return RegisterWord::Other;
        }
        match digits.parse::<u32>() {
            Ok(number) if number < Self::COUNT && digits.len() == number.to_string().len() => {
                RegisterWord::Register(Reg(number))
            }
            _ => RegisterWord::NotARegister,
        }
    }

    fn number(register: Reg) -> u32 {
        register.0
    }
}

impl InstructionSet for Toy {
    type Instruction = Op;
    type Format = Format;
    type Space = ();
    type Registers = Self;

    const SYNTAX: Syntax = Syntax {
        comment: '#',
        open: '(',
        close: ')',
    };

    const MNEMONICS: &'static [&'static str] = &["addi", "lui", "lw", "beq", "li", "nop", "j"];

    fn limits(format: Format) -> Limits {
        Limits {
            program_bytes: 200,
            program_lines: 10,
            instructions: match format {
                Format::Asm => Some(6),
                Format::Flat => None,
            },
            label_chars: 8,
        }
    }

    fn has_labels(format: Format) -> bool {
        format == Format::Asm
    }

    fn read_line<'s>(
        context: &Context<'_, Self>,
        tokens: &[Token<'s>],
    ) -> Result<Read<'s, Op>, Problem> {
        let [first, rest @ ..] = tokens else {
            return Err(problem(tokens.first(), "empty line"));
        };
        let mut operands = Operands {
            tokens: rest,
            at: first,
        };
        let read = if first.is("addi") {
            let rd = operands.register()?;
            operands.comma()?;
            let rs1 = operands.register()?;
            operands.comma()?;
            let imm = operands.immediate(12)?;
            Read::one(Op::Addi { rd, rs1, imm })
        } else if first.is("lw") {
            let rd = operands.register()?;
            operands.comma()?;
            let offset = operands.immediate(12)?;
            operands.expect(TokenKind::Open)?;
            let base = operands.register()?;
            operands.expect(TokenKind::Close)?;
            Read::one(Op::Lw { rd, base, offset })
        } else if first.is("lui") {
            let rd = operands.register()?;
            operands.comma()?;
            let imm = operands.number(0, 0xF_FFFF)?;
            Read::one(Op::Lui { rd, imm })
        } else if first.is("beq") {
            let rs1 = operands.register()?;
            operands.comma()?;
            let rs2 = operands.register()?;
            operands.comma()?;
            let label = operands.label(context)?;
            Read::branching(
                Op::Beq {
                    rs1,
                    rs2,
                    offset: 0,
                },
                label,
            )
        } else if first.is("j") {
            let label = operands.label(context)?;
            let zero = Reg(0);
            Read::branching(
                Op::Beq {
                    rs1: zero,
                    rs2: zero,
                    offset: 0,
                },
                label,
            )
        } else if first.is("nop") {
            Read::one(Op::Addi {
                rd: Reg(0),
                rs1: Reg(0),
                imm: 0,
            })
        } else if first.is("li") {
            let rd = operands.register()?;
            operands.comma()?;
            let value = operands.number(-(1 << 31), (1 << 31) - 1)?;
            li(rd, value)
        } else {
            return Err(problem(
                Some(first),
                &format!("unknown instruction `{}`", first.text),
            ));
        };
        operands.finish()?;
        Ok(read)
    }

    fn resolve(instruction: &mut Op, at: u32, target: u32) -> Result<(), String> {
        let Op::Beq { offset, .. } = instruction else {
            return Ok(());
        };
        let bytes = (i64::from(target) - i64::from(at)) * 4;
        if bytes.abs() > REACH {
            return Err(format!(
                "the label is {bytes} bytes away, past the reach of {REACH}"
            ));
        }
        *offset = bytes;
        Ok(())
    }

    fn word(reason: Reason<'_>, (): &(), format: Format) -> String {
        match reason {
            Reason::TooLarge => format!("{format:?} source too large"),
            Reason::TooManyLines => "too many lines".to_owned(),
            Reason::TooManyInstructions => "too many instructions".to_owned(),
            Reason::Empty => "nothing to read".to_owned(),
            Reason::NotALabel { text } => format!("`{text}` is not spelled like a label"),
            Reason::ReservedLabel { name, register } => {
                let what = if register { "a register" } else { "a mnemonic" };
                format!("`{name}` is {what}")
            }
            Reason::LabelTooLong { chars } => format!("a label of {chars} characters"),
            Reason::DuplicateLabel { name, line } => format!("`{name}` is on line {line}"),
            Reason::UndefinedLabel { name } => format!("`{name}` is nowhere"),
        }
    }

    fn locate(line: u32, column: u32, message: &str) -> String {
        format!("{line}:{column}: {message}")
    }
}

impl Encode for Toy {
    type Word = u32;

    fn encode(instruction: &Op) -> u32 {
        match *instruction {
            Op::Addi { rd, rs1, imm } => (bits(imm, 12) << 20) | (rs1.0 << 15) | (rd.0 << 7) | 0x13,
            Op::Lui { rd, imm } => (bits(imm, 20) << 12) | (rd.0 << 7) | 0x37,
            Op::Lw { rd, base, offset } => {
                (bits(offset, 12) << 20) | (base.0 << 15) | (0b010 << 12) | (rd.0 << 7) | 0x03
            }
            Op::Beq { rs1, rs2, offset } => {
                let offset = bits(offset, 13);
                ((offset >> 12) << 31)
                    | (((offset >> 5) & 0x3F) << 25)
                    | (rs2.0 << 20)
                    | (rs1.0 << 15)
                    | (((offset >> 1) & 0xF) << 8)
                    | (((offset >> 11) & 1) << 7)
                    | 0x63
            }
        }
    }
}

/// The low `width` bits of `value`, two's complement.
fn bits(value: i64, width: u32) -> u32 {
    u32::try_from(value & ((1 << width) - 1)).unwrap_or_default()
}

/// A problem at `token`, or at the start of the line without one.
fn problem(token: Option<&Token<'_>>, message: &str) -> Problem {
    Problem {
        column: token.map_or(1, |token| token.column),
        length: token.map_or(0, |token| token.length),
        message: message.to_owned(),
    }
}

/// `li rd, value`: one `addi` if the value fits 12 bits, else a `lui` of its
/// upper bits and an `addi` of its lower.
fn li<'s>(rd: Reg, value: i64) -> Read<'s, Op> {
    let lower = ((value & 0xFFF) ^ 0x800) - 0x800;
    if lower == value {
        return Read::one(Op::Addi {
            rd,
            rs1: Reg(0),
            imm: lower,
        });
    }
    let mut read = Read::one(Op::Lui {
        rd,
        imm: ((value - lower) >> 12) & 0xF_FFFF,
    });
    read.instructions.push(Op::Addi {
        rd,
        rs1: rd,
        imm: lower,
    });
    read
}

/// The operands of one instruction, read in order.
struct Operands<'a, 's> {
    /// The tokens after the mnemonic, yet to be read.
    tokens: &'a [Token<'s>],
    /// The mnemonic, where an error goes when an operand is missing.
    at: &'a Token<'s>,
}

impl<'s> Operands<'_, 's> {
    /// The next token.
    fn next(&mut self) -> Result<Token<'s>, Problem> {
        let Some((&first, rest)) = self.tokens.split_first() else {
            return Err(problem(Some(self.at), "an operand is missing"));
        };
        self.tokens = rest;
        Ok(first)
    }

    /// A token of `kind`.
    fn expect(&mut self, kind: TokenKind) -> Result<(), Problem> {
        let token = self.next()?;
        if token.kind == kind {
            Ok(())
        } else {
            Err(problem(Some(&token), "not what the operand needs"))
        }
    }

    /// A `,`.
    fn comma(&mut self) -> Result<(), Problem> {
        self.expect(TokenKind::Comma)
    }

    /// A register.
    fn register(&mut self) -> Result<Reg, Problem> {
        let token = self.next()?;
        match <Toy as RegisterFile>::read(&token) {
            RegisterWord::Register(register) => Ok(register),
            RegisterWord::NotARegister => Err(problem(Some(&token), "not a register")),
            RegisterWord::Other => Err(problem(Some(&token), "a register is needed")),
        }
    }

    /// A number, possibly negative, between `low` and `high`.
    fn number(&mut self, low: i64, high: i64) -> Result<i64, Problem> {
        let token = self.next()?;
        let (negative, digits) = match token.text.strip_prefix('-') {
            Some(digits) => (true, digits),
            None => (false, token.text),
        };
        let magnitude = match lexer::number(digits) {
            NumberWord::Hex(Some(value)) | NumberWord::Decimal(Some(value)) => i64::try_from(value),
            NumberWord::Hex(None) | NumberWord::Decimal(None) | NumberWord::Other => {
                return Err(problem(Some(&token), "a number is needed"));
            }
        }
        .map_err(|_| problem(Some(&token), "a number is too big"))?;
        let value = if negative { -magnitude } else { magnitude };
        if (low..=high).contains(&value) {
            Ok(value)
        } else {
            Err(problem(Some(&token), &format!("{value} is out of range")))
        }
    }

    /// A signed immediate of `width` bits.
    fn immediate(&mut self, width: u32) -> Result<i64, Problem> {
        self.number(-(1 << (width - 1)), (1 << (width - 1)) - 1)
    }

    /// A label, which is only a token until the program is read.
    fn label(&mut self, context: &Context<'_, Toy>) -> Result<Token<'s>, Problem> {
        let token = self.next()?;
        context.label(&token)?;
        Ok(token)
    }

    /// That no token is left over.
    fn finish(&self) -> Result<(), Problem> {
        match self.tokens.first() {
            Some(extra) => Err(problem(Some(extra), "too many operands")),
            None => Ok(()),
        }
    }
}

/// The program `source` makes, or the messages of its errors with their
/// places, as `line:column length message`.
fn read(source: &str, format: Format) -> Result<Program<Toy>, Vec<String>> {
    Program::parse(&(), source, format).map_err(|failure| {
        let count = failure.count();
        failure
            .into_listed(count)
            .into_iter()
            .map(|error| {
                format!(
                    "{}:{} {} {}",
                    error.line, error.column, error.length, error.message
                )
            })
            .collect()
    })
}

/// The instructions of `program`.
fn ops(program: &Program<Toy>) -> Vec<Op> {
    (0..program.len())
        .filter_map(|index| program.get(index))
        .map(|placed| placed.instruction.clone())
        .collect()
}

#[test]
fn a_program_reads_into_instructions_with_their_lines_and_text() {
    let source = "start:  addi  x5, sp, -4   # frame\n\n  lw a0, 8(x5)\n";
    // `a0` is not an ABI name of this language, so the second line fails.
    assert_eq!(
        read(source, Format::Asm),
        Err(vec!["3:6 2 a register is needed".to_owned()])
    );
    let program = read(
        "start:  addi  x5, sp, -4   # frame\n\n  lw x6, 8(x5)\n",
        Format::Asm,
    )
    .unwrap();
    assert_eq!(
        ops(&program),
        [
            Op::Addi {
                rd: Reg(5),
                rs1: Reg(2),
                imm: -4
            },
            Op::Lw {
                rd: Reg(6),
                base: Reg(5),
                offset: 8
            },
        ]
    );
    let lines: Vec<(u32, &str)> = (0..program.len())
        .filter_map(|index| program.get(index))
        .map(|placed| (placed.line, &*placed.text))
        .collect();
    assert_eq!(lines, [(1, "addi x5, sp, -4"), (3, "lw x6, 8(x5)")]);
    assert_eq!(program.format(), Format::Asm);
    assert_eq!(
        program.source(),
        "start:  addi  x5, sp, -4   # frame\n\n  lw x6, 8(x5)\n"
    );
}

#[test]
fn registers_have_numbers_and_abi_names() {
    let read = |text: &str| <Toy as RegisterFile>::read(&lexer::tokens(text, Toy::SYNTAX)[0]);
    assert_eq!(read("x0"), RegisterWord::Register(Reg(0)));
    assert_eq!(read("X31"), RegisterWord::Register(Reg(31)));
    assert_eq!(read("zero"), RegisterWord::Register(Reg(0)));
    assert_eq!(read("RA"), RegisterWord::Register(Reg(1)));
    for text in ["x32", "x01", "x99999999999999999999"] {
        assert_eq!(read(text), RegisterWord::NotARegister, "{text}");
    }
    for text in ["x", "xx", "r1", "loop", "12"] {
        assert_eq!(read(text), RegisterWord::Other, "{text}");
    }
    assert_eq!(<Toy as RegisterFile>::COUNT, 32);
    assert_eq!(<Toy as RegisterFile>::number(Reg(7)), 7);
}

#[test]
fn a_pseudo_instruction_stands_for_several_instructions() {
    let program = read(
        "beq x1, x0, end\nli x5, 0x12345678\nend: nop\n",
        Format::Asm,
    )
    .unwrap();
    // The label marks the fourth instruction: `li` is two.
    assert_eq!(
        ops(&program),
        [
            Op::Beq {
                rs1: Reg(1),
                rs2: Reg(0),
                offset: 12
            },
            Op::Lui {
                rd: Reg(5),
                imm: 0x12345
            },
            Op::Addi {
                rd: Reg(5),
                rs1: Reg(5),
                imm: 0x678
            },
            Op::Addi {
                rd: Reg(0),
                rs1: Reg(0),
                imm: 0
            },
        ]
    );
    // Both halves of the `li` come from its one line and its one text.
    let halves: Vec<(u32, &str)> = [1, 2]
        .into_iter()
        .filter_map(|index| program.get(index))
        .map(|placed| (placed.line, &*placed.text))
        .collect();
    assert_eq!(halves, [(2, "li x5, 0x12345678"); 2]);
    // A small value is one instruction, and a negative lower half carries
    // into the upper.
    let small = read("li x1, -5", Format::Asm).unwrap();
    assert_eq!(small.len(), 1);
    let carried = read("li x1, 0x00000FFF", Format::Asm).unwrap();
    assert_eq!(
        ops(&carried),
        [
            Op::Lui { rd: Reg(1), imm: 1 },
            Op::Addi {
                rd: Reg(1),
                rs1: Reg(1),
                imm: -1
            },
        ]
    );
}

#[test]
fn a_branch_holds_the_byte_offset_to_its_label_either_way() {
    let program = read(
        "loop: addi x1, x1, -1\nbeq x1, x0, done\nj loop\ndone: nop",
        Format::Asm,
    )
    .unwrap();
    let offsets: Vec<i64> = ops(&program)
        .into_iter()
        .filter_map(|op| match op {
            Op::Beq { offset, .. } => Some(offset),
            _ => None,
        })
        .collect();
    // From the second instruction to the fourth, and from the third to the first.
    assert_eq!(offsets, [8, -8]);
}

#[test]
fn a_branch_out_of_reach_is_an_error_at_its_label() {
    let source = "beq x0, x0, far\nnop\nnop\nnop\nnop\nfar: nop\n";
    assert_eq!(
        read(source, Format::Asm),
        Err(vec![
            "1:13 3 the label is 20 bytes away, past the reach of 12".to_owned()
        ])
    );
}

#[test]
fn words_are_the_encodings_of_riscv() {
    let program = read(
        "addi x1, x0, 5\nlw x2, 4(x1)\nlui x5, 0x12345\nbeq x1, x2, next\nnext: nop",
        Format::Asm,
    )
    .unwrap();
    assert_eq!(
        program.words().collect::<Vec<_>>(),
        [
            0x0050_0093,
            0x0040_A103,
            0x1234_52B7,
            0x0020_8263,
            0x0000_0013
        ]
    );
    let backward = read("top: nop\nbeq x1, x2, top", Format::Asm).unwrap();
    // A branch of -4 bytes sets the sign bit and the middle bits.
    assert_eq!(backward.words().last(), Some(0xFE20_8EE3));
}

#[test]
fn a_label_is_checked_against_the_registers_and_the_mnemonics_of_the_language() {
    let cases = [
        ("sp: nop", "1:1 2 `sp` is a register"),
        ("X7: nop", "1:1 2 `X7` is a register"),
        ("li: nop", "1:1 2 `li` is a mnemonic"),
        ("1x: nop", "1:1 2 `1x` is not spelled like a label"),
        ("nine_chars: nop", "1:1 10 a label of 10 characters"),
        ("a: nop\na: nop", "2:1 1 `a` is on line 1"),
        ("j nowhere", "1:3 7 `nowhere` is nowhere"),
        ("beq x0, x0, 3x", "1:13 2 `3x` is not spelled like a label"),
    ];
    for (source, message) in cases {
        assert_eq!(
            read(source, Format::Asm),
            Err(vec![message.to_owned()]),
            "{source}"
        );
    }
    // A label may end the program, marking one past the last instruction.
    assert_eq!(ops(&read("j end\nend:", Format::Asm).unwrap()).len(), 1);
}

#[test]
fn a_format_decides_the_labels_and_the_limits() {
    // No labels: `top:` is where the mnemonic goes.
    assert_eq!(
        read("top: nop", Format::Flat),
        Err(vec!["1:1 3 unknown instruction `top`".to_owned()])
    );
    // No instruction limit, only the line limit.
    let long = "nop\n".repeat(8);
    assert_eq!(
        read(&long, Format::Asm),
        Err(vec!["7:1 3 too many instructions".to_owned()])
    );
    assert_eq!(read(&long, Format::Flat).unwrap().len(), 8);
}

#[test]
fn the_limits_are_the_instruction_sets_and_each_is_reported_at_its_place() {
    // 200 bytes is allowed, and the first byte past it is the error's place.
    let at_limit = format!("{}\n", "# ".repeat(99));
    assert_eq!(at_limit.len(), 199);
    assert_eq!(
        read(&at_limit, Format::Asm),
        Err(vec!["1:1 0 nothing to read".to_owned()])
    );
    let too_large = "# ".repeat(101);
    assert_eq!(
        read(&too_large, Format::Asm),
        Err(vec!["1:201 1 Asm source too large".to_owned()])
    );
    // Ten lines are read; the eleventh is underlined whole.
    let lines = "# c\n".repeat(10) + "addi x1, x1, 1";
    assert_eq!(
        read(&lines, Format::Asm),
        Err(vec!["11:1 14 too many lines".to_owned()])
    );
}

#[test]
fn every_error_is_found_in_source_order_and_counted() {
    let source = "nop\nbad x1\naddi x1, x1\nbeq x0, x0, gone\nlw x1, 4(x2) extra\n";
    let failure = Program::<Toy>::parse(&(), source, Format::Asm).unwrap_err();
    assert_eq!(failure.count(), 4);
    assert_eq!(failure.format(), Format::Asm);
    assert_eq!(failure.first().text, "2:1: unknown instruction `bad`");
    let listed = failure.into_listed(3);
    assert_eq!(
        listed.iter().map(|error| error.line).collect::<Vec<_>>(),
        [2, 3, 4]
    );
}
