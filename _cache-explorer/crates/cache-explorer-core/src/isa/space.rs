//! The address space a program is checked against.

use crate::geometry::Geometry;
use crate::units::{Addr, Word};

/// What the assembler and the trace parser ask of the machine they parse
/// for: how wide an address is, how big a word, and which literal addresses
/// and values fit. The parsers know nothing else of the machine, so any
/// memory system that answers these can have programs parsed for it.
pub(crate) trait AddressSpace {
    /// Width of a byte address.
    fn address_bits(&self) -> u8;

    /// Bytes per word, also the alignment of an address.
    fn word_bytes(&self) -> u32;

    /// `value` as an address, if it fits the address width.
    fn addr(&self, value: u32) -> Option<Addr>;

    /// The word a store of `value` writes: its low `8 × word_bytes` bits.
    fn fit_word(&self, value: u64) -> Word;
}

impl AddressSpace for Geometry {
    fn address_bits(&self) -> u8 {
        Geometry::address_bits(self)
    }

    fn word_bytes(&self) -> u32 {
        Geometry::word_bytes(self)
    }

    fn addr(&self, value: u32) -> Option<Addr> {
        Geometry::addr(self, value)
    }

    fn fit_word(&self, value: u64) -> Word {
        Geometry::fit_word(self, value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::geometry::AccessError;
    use crate::input;
    use crate::isa::{Address, Instruction, Operand, Program};
    use crate::op::ProgramFormat;
    use crate::units::Register;

    /// A space that is no cache's geometry: six address bits and two-byte
    /// words, narrower than any configuration the cache accepts.
    struct Narrow;

    impl AddressSpace for Narrow {
        fn address_bits(&self) -> u8 {
            6
        }

        fn word_bytes(&self) -> u32 {
            2
        }

        fn addr(&self, value: u32) -> Option<Addr> {
            (value < 1 << 6).then(|| Addr::new_unchecked(value))
        }

        fn fit_word(&self, value: u64) -> Word {
            Word::new_unchecked(value & 0xFFFF)
        }
    }

    fn first_error(source: &str, format: ProgramFormat) -> String {
        Program::parse(&Narrow, source, format)
            .unwrap_err()
            .first()
            .message
            .clone()
    }

    #[test]
    fn the_assembler_checks_against_the_space_it_is_given() {
        let program = Program::parse(
            &Narrow,
            "ld r1, [0x3E]\nst [0x10], #65535\nst [0x12], #-32768",
            ProgramFormat::Asm,
        )
        .unwrap();
        let instructions: Vec<&Instruction> = (0..program.len())
            .filter_map(|index| program.get(index))
            .map(|placed| &placed.instruction)
            .collect();
        assert_eq!(
            instructions,
            [
                &Instruction::Load {
                    into: Register::R1,
                    from: Address::Literal(Addr::new_unchecked(0x3E)),
                },
                &Instruction::Store {
                    to: Address::Literal(Addr::new_unchecked(0x10)),
                    value: Operand::Immediate(0xFFFF),
                },
                &Instruction::Store {
                    to: Address::Literal(Addr::new_unchecked(0x12)),
                    value: Operand::Immediate(0x8000),
                },
            ]
        );
    }

    #[test]
    fn the_assembler_words_its_errors_for_the_space() {
        assert_eq!(
            first_error("ld r1, [0x40]", ProgramFormat::Asm),
            AccessError::OutOfRange {
                addr: 0x40,
                address_bits: 6
            }
            .to_string()
        );
        assert_eq!(
            first_error("ld r1, [0x3D]", ProgramFormat::Asm),
            AccessError::Misaligned {
                addr: Addr::new_unchecked(0x3D),
                address_bits: 6,
                word_bytes: 2
            }
            .to_string()
        );
        assert_eq!(
            first_error("st [0x10], #65536", ProgramFormat::Asm),
            input::fit_word(false, 65_536, 2).unwrap_err().to_string()
        );
    }

    #[test]
    fn the_trace_parser_checks_against_the_space_it_is_given() {
        let program =
            Program::parse(&Narrow, "R 0x20\nW 0x22 0xFFFF", ProgramFormat::Trace).unwrap();
        assert_eq!(
            program.get(1).map(|placed| &placed.instruction),
            Some(&Instruction::Store {
                to: Address::Literal(Addr::new_unchecked(0x22)),
                value: Operand::Immediate(0xFFFF),
            })
        );
        assert!(first_error("R 0x40", ProgramFormat::Trace).contains("0x40"));
        assert!(first_error("W 0x22 65536", ProgramFormat::Trace).contains("2-byte word"));
    }
}
