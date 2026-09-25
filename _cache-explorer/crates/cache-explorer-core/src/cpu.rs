//! The CPU's registers.
//!
//! Eight 64-bit registers, `r0` to `r7`, whatever the word size, so a program
//! runs unchanged between word-size experiments. `r0` always reads 0, and
//! writes to it are dropped.

use crate::units::Register;

/// The eight registers' values, all 0 when a run starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Registers([u64; 8]);

impl Registers {
    /// The value `register` holds; always 0 for `r0`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "stores from a register and the state views read registers"
        )
    )]
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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

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
