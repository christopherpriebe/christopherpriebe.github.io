//! The CPU: its registers, the loaded program, and its place in it.
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

use std::fmt::Debug;
use std::sync::Arc;

use crate::isa::{AddressSpace, Placed, Program};
use crate::op::ProgramFormat;
use crate::units::Register;

/// The CPU: its registers, the loaded program or trace, and its place in it,
/// all zero, empty and running when a run starts.
///
/// Its fields are private: the program is replaced by [`CpuState::load`],
/// which also resets the registers, the pc and the budget; a register
/// changes by [`CpuState::write`]; the pc moves forward by
/// [`CpuState::retire`]; and [`CpuState::halt`] stops the program. Every
/// state the CPU is in is one of those calls reached.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CpuState {
    /// The CPU's registers.
    registers: Registers,
    /// The loaded program or trace, if any. It is never changed once
    /// parsed, so a copy of the CPU, such as a checkpoint's, shares it with
    /// the CPU it was copied from; `Arc` rather than `Rc` keeps machines and
    /// sessions `Send` and `Sync`.
    program: Option<Arc<Program>>,
    /// The index of the next instruction to execute. At the program's
    /// length, the program has finished.
    pc: u32,
    /// The instructions retired since the program was loaded.
    executed: u32,
    /// Why the program stopped before its end, if it did.
    halted: Option<Halted>,
}

/// What instruction execution needs of the machine around a CPU: the CPU of
/// each core, the address space its addresses are checked against, and a hook
/// for the start of an entry that is not an access.
///
/// A machine with one CPU has the core `()`; one with several names each by
/// a value of its own, and one executor steps whichever core it is told to.
pub(crate) trait CpuHost {
    /// Which CPU an instruction runs on.
    type Core: Copy + Eq + Debug;
    /// The address space a program's addresses are checked against. Small
    /// and `Copy`, so a message can be worded after the host changes.
    type Space: AddressSpace + Copy;

    /// The CPU of `core`.
    fn cpu(&self, core: Self::Core) -> &CpuState;

    /// The CPU of `core`, to change through [`CpuState`]'s methods.
    fn cpu_mut(&mut self, core: Self::Core) -> &mut CpuState;

    /// The address space.
    fn space(&self) -> &Self::Space;

    /// An entry that is not an access has begun, so the access of the entry
    /// before it, if the host keeps one, is no longer the one in progress.
    fn entry_begun(&mut self);
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

/// What loading a program reset, for the event that reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cleared {
    /// The registers that held a value, in number order.
    pub(crate) registers: Vec<Register>,
    /// Whether the pc was not already 0.
    pub(crate) pc: bool,
}

/// Why the loaded program cannot execute an instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotReady {
    /// No program is loaded.
    NoProgram,
    /// The program faulted or ran out of budget, at the instruction on this
    /// source line.
    Stopped {
        /// The line of the instruction that did not execute.
        line: u32,
    },
    /// The program has executed its last instruction.
    Finished,
}

impl CpuState {
    /// Loads `program`: the registers are cleared, and the pc, the budget
    /// and the program's status reset. Says what the load changed.
    pub(crate) fn load(&mut self, program: Program) -> Cleared {
        let cleared = Cleared {
            registers: self.registers.clear(),
            pc: self.pc != 0,
        };
        self.program = Some(Arc::new(program));
        self.pc = 0;
        self.executed = 0;
        self.halted = None;
        cleared
    }

    /// The value `register` holds; always 0 for `r0`.
    pub(crate) fn register(&self, register: Register) -> u64 {
        self.registers.get(register)
    }

    /// Puts `value` into `register`, and says whether its value changed. A
    /// write to `r0` is dropped and changes nothing.
    pub(crate) fn write(&mut self, register: Register, value: u64) -> bool {
        self.registers.set(register, value)
    }

    /// Retires the instruction at the pc and moves the pc to `next`. Says
    /// whether the pc changed: a branch to itself leaves it where it was.
    pub(crate) fn retire(&mut self, next: u32) -> bool {
        self.executed = self.executed.saturating_add(1);
        let moved = self.pc != next;
        self.pc = next;
        moved
    }

    /// Stops the program before the instruction at the pc, which does not
    /// execute.
    pub(crate) fn halt(&mut self, halted: Halted) {
        self.halted = Some(halted);
    }

    /// The loaded program or trace, if any.
    pub(crate) fn program(&self) -> Option<&Arc<Program>> {
        self.program.as_ref()
    }

    /// The index of the next instruction.
    pub(crate) fn pc(&self) -> u32 {
        self.pc
    }

    /// The instructions retired since the program was loaded.
    pub(crate) fn executed(&self) -> u32 {
        self.executed
    }

    /// Why the program stopped before its end, if it did.
    pub(crate) fn halted(&self) -> Option<Halted> {
        self.halted
    }

    /// The instruction the program executes next: the one at the pc, while
    /// a program is loaded, has not stopped and has not finished.
    pub(crate) fn next_instruction(&self) -> Option<&Placed> {
        if self.halted.is_some() {
            return None;
        }
        self.program.as_ref()?.get(self.pc)
    }

    /// Whether the loaded program can execute an instruction.
    ///
    /// # Errors
    ///
    /// [`NotReady::NoProgram`] with no program loaded;
    /// [`NotReady::Stopped`] once it has faulted or run out of budget,
    /// naming the line it stopped at; and [`NotReady::Finished`] once it
    /// has executed its last instruction.
    pub(crate) fn ready(&self) -> Result<(), NotReady> {
        let Some(program) = &self.program else {
            return Err(NotReady::NoProgram);
        };
        let at = program.get(self.pc);
        match (self.halted, at) {
            (None, Some(_)) => Ok(()),
            (Some(_), Some(placed)) => Err(NotReady::Stopped { line: placed.line }),
            (_, None) => Err(NotReady::Finished),
        }
    }

    /// Whether an assembly program is loaded with an instruction left at the
    /// pc, so that a register a hand-issued load writes may be one it uses.
    /// One stopped by a fault or by its budget has not finished: it is still
    /// loaded, with its registers on show. A trace reads no register.
    pub(crate) fn asm_unfinished(&self) -> bool {
        self.program.as_ref().is_some_and(|program| {
            program.format() == ProgramFormat::Asm && self.pc < program.len()
        })
    }

    /// Sets the budget's count, so a test reaches its end without executing
    /// every instruction.
    #[cfg(test)]
    pub(crate) fn set_executed(&mut self, executed: u32) {
        self.executed = executed;
    }
}

/// The eight registers' values, all 0 when a run starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Registers([u64; 8]);

impl Registers {
    /// The value `register` holds; always 0 for `r0`.
    fn get(&self, register: Register) -> u64 {
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
    fn set(&mut self, register: Register, value: u64) -> bool {
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
    fn clear(&mut self) -> Vec<Register> {
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
    use crate::config::Config;

    /// A program of `source`, assembled for the default configuration.
    fn program(source: &str) -> Program {
        let geometry = Config::default().validate().unwrap();
        Program::parse(&geometry, source, ProgramFormat::Asm).unwrap()
    }

    #[test]
    fn loading_resets_the_cpu_and_says_what_changed() {
        let mut cpu = CpuState::default();
        cpu.load(program("add r3, r0, #7\nadd r1, r0, #2\nadd r2, r0, #1"));
        cpu.write(Register::R3, 7);
        cpu.write(Register::R1, 2);
        cpu.retire(2);
        cpu.halt(Halted::Faulted);
        let cleared = cpu.load(program("add r1, r0, #1"));
        assert_eq!(
            cleared,
            Cleared {
                registers: vec![Register::R1, Register::R3],
                pc: true
            }
        );
        assert_eq!(cpu.register(Register::R3), 0);
        assert_eq!((cpu.pc(), cpu.executed(), cpu.halted()), (0, 0, None));
        assert_eq!(cpu.program().map(|loaded| loaded.len()), Some(1));
        // Loading again right after a load changes nothing to report.
        let again = cpu.load(program("add r1, r0, #1"));
        assert_eq!(
            again,
            Cleared {
                registers: Vec::new(),
                pc: false
            }
        );
    }

    #[test]
    fn a_cpu_is_ready_only_with_an_instruction_left_that_it_may_execute() {
        let mut cpu = CpuState::default();
        assert_eq!(cpu.ready(), Err(NotReady::NoProgram));
        assert!(cpu.next_instruction().is_none());
        cpu.load(program("add r1, r0, #1\nadd r2, r0, #1"));
        assert_eq!(cpu.ready(), Ok(()));
        assert_eq!(cpu.next_instruction().map(|placed| placed.line), Some(1));
        cpu.retire(1);
        cpu.halt(Halted::OutOfBudget);
        assert_eq!(cpu.ready(), Err(NotReady::Stopped { line: 2 }));
        assert!(cpu.next_instruction().is_none());
        cpu.load(program("add r1, r0, #1"));
        cpu.retire(1);
        assert_eq!(cpu.ready(), Err(NotReady::Finished));
    }

    #[test]
    fn only_an_assembly_program_with_instructions_left_is_unfinished() {
        let mut cpu = CpuState::default();
        assert!(!cpu.asm_unfinished());
        cpu.load(program("add r1, r0, #1"));
        assert!(cpu.asm_unfinished());
        cpu.halt(Halted::Faulted);
        assert!(cpu.asm_unfinished());
        cpu.retire(1);
        assert!(!cpu.asm_unfinished());
    }

    #[test]
    fn retiring_moves_the_pc_and_counts_the_instruction() {
        let mut cpu = CpuState::default();
        assert!(cpu.retire(1));
        assert!(cpu.retire(5));
        // A branch to itself leaves the pc where it was.
        assert!(!cpu.retire(5));
        assert_eq!(
            cpu,
            CpuState {
                pc: 5,
                executed: 3,
                ..CpuState::default()
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
