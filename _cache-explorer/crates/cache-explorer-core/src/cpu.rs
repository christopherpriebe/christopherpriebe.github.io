//! The CPU: its registers, and its place in the loaded program.
//!
//! Eight 64-bit registers, `r0` to `r7`, whatever the word size, so a program
//! runs unchanged between word-size experiments. `r0` always reads 0, and
//! writes to it are dropped.
//!
//! The pc is the index of the next instruction. It moves at an instruction's
//! last event, when the instruction retires, so the instruction in flight is
//! the one at the pc. The CPU counts the instructions it has retired since
//! the program was loaded, which the budget bounds, and whether the program
//! stopped before its end: on a fault, or when the budget ran out.

use crate::units::Register;

/// The CPU's place in the loaded program, all zero and running when a run
/// starts and when a program is loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cpu {
    /// The index of the next instruction to execute. At the program's
    /// length, the program has finished.
    pub(crate) pc: u32,
    /// The instructions retired since the program was loaded.
    pub(crate) executed: u32,
    /// Why the program stopped before its end, if it did.
    pub(crate) halted: Option<Halted>,
}

/// Why a program stopped before its end. Either way the pc stays at the
/// instruction that did not execute, and the program must be loaded again
/// to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Halted {
    /// An address held in a register did not fit the address width, or was
    /// not a multiple of the word size.
    Faulted,
    /// The budget of instructions ran out.
    OutOfBudget,
}

impl Cpu {
    /// Retires the instruction at the pc and moves the pc to `next`. Says
    /// whether the pc changed: a branch to itself leaves it where it was.
    pub(crate) fn retire(&mut self, next: u32) -> bool {
        self.executed = self.executed.saturating_add(1);
        let moved = self.pc != next;
        self.pc = next;
        moved
    }
}

/// The eight registers' values, all 0 when a run starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Registers([u64; 8]);

impl Registers {
    /// The value `register` holds; always 0 for `r0`.
    pub(crate) fn get(&self, register: Register) -> u64 {
        if register == Register::R0 {
            return 0;
        }
        self.0
            .get(usize::from(register.get()))
            .copied()
            .unwrap_or(0)
    }

    /// Puts `value` into `register`, and says whether its value changed. A
    /// write to `r0` is dropped and changes nothing.
    pub(crate) fn set(&mut self, register: Register, value: u64) -> bool {
        if register == Register::R0 {
            return false;
        }
        match self.0.get_mut(usize::from(register.get())) {
            Some(slot) if *slot != value => {
                *slot = value;
                true
            }
            _ => false,
        }
    }

    /// Puts 0 into every register, as a program load does, and returns the
    /// registers whose value changed, in number order.
    pub(crate) fn clear(&mut self) -> Vec<Register> {
        Register::ALL
            .into_iter()
            .filter(|&register| self.set(register, 0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn retiring_moves_the_pc_and_counts_the_instruction() {
        let mut cpu = Cpu::default();
        assert!(cpu.retire(1));
        assert!(cpu.retire(5));
        // A branch to itself leaves the pc where it was.
        assert!(!cpu.retire(5));
        assert_eq!(
            cpu,
            Cpu {
                pc: 5,
                executed: 3,
                halted: None
            }
        );
    }

    #[test]
    fn clearing_names_the_registers_that_held_a_value() {
        let mut registers = Registers::default();
        registers.set(Register::R3, 7);
        registers.set(Register::R1, 2);
        assert_eq!(registers.clear(), [Register::R1, Register::R3]);
        assert_eq!(registers, Registers::default());
        assert!(registers.clear().is_empty());
    }

    #[test]
    fn registers_start_at_zero() {
        let registers = Registers::default();
        for register in [Register::R0, Register::R1, Register::R7] {
            assert_eq!(registers.get(register), 0);
        }
    }

    #[test]
    fn a_write_reports_whether_the_value_changed() {
        let mut registers = Registers::default();
        assert!(registers.set(Register::R3, 68));
        assert_eq!(registers.get(Register::R3), 68);
        assert!(!registers.set(Register::R3, 68));
        assert!(registers.set(Register::R3, u64::MAX));
        assert_eq!(registers.get(Register::R3), u64::MAX);
        assert_eq!(registers.get(Register::R2), 0);
    }

    #[test]
    fn r0_reads_zero_and_drops_writes() {
        let mut registers = Registers::default();
        assert!(!registers.set(Register::R0, 7));
        assert_eq!(registers.get(Register::R0), 0);
        assert_eq!(registers, Registers::default());
    }
}
