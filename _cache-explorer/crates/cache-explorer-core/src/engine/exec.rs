//! Programs on the machine: loading one, and executing its instructions one
//! at a time.
//!
//! A load is one `Setup` event, an entry the run does not number. It replaces
//! the program, clears the registers and resets the pc, the budget and the
//! program's status; the cache, memory and counters keep what they hold, so
//! a program can run on a warm cache.
//!
//! Each instruction is one entry. `ld` and `st` are accesses, with the
//! events of their case; `add` and `bnez` are one `Complete` event each. The
//! pc moves at the instruction's last event, when it retires, so the
//! instruction in flight is the one at the pc; a `bnez` that branches to
//! itself leaves it where it was. When the pc passes the last instruction the
//! program has finished.
//!
//! Two things stop a program before its end, each with an `Error` event
//! that is an entry of its own and ends the op, never a panic. An address
//! held in a register that does not fit the address width, or is not a
//! multiple of the word size, faults. And every load of a program has a
//! budget of [`INSTRUCTION_BUDGET`] instructions: the one after the last is
//! not executed, and an `Error` event takes its place. Either way the pc
//! stays at the instruction that did not execute. Literal addresses and
//! immediates were checked when the program was parsed, so they never fault.
//!
//! An instruction's `ld` and `st` are performed by a [`MemoryPort`], which the
//! executor is generic over: the cache machine's own memory in a run, and any
//! other memory system that takes a request and emits its events in a test.
//!
//! `StepAccess` and `Run` execute one instruction after another, each its own
//! entry, until an access has completed or until they have executed as many
//! as asked; the program finishing or stopping ends them too. Between two
//! instructions they check the session's limit, and stop with an `Error`
//! event, as the budget does, when the log has no room for another entry.

use std::sync::Arc;

use crate::cpu::{Cpu, Halted};
use crate::engine::port::{CacheMemory, MemoryPort};
use crate::engine::{end_entry, Draft, Emitter, Flow, Halt, OpProgress};
use crate::event::{ChangeRef, EventKind};
use crate::explain;
use crate::isa::{Address, AddressSpace, Instruction, Operand, Program};
use crate::limits::INSTRUCTION_BUDGET;
use crate::machine::Machine;
use crate::op::OpError;
use crate::request::{Origin, Request};
use crate::units::{Addr, Register};

/// When an op that executes several instructions stops, if the program has
/// not finished or stopped first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Until {
    /// Once an instruction has made a memory access: `StepAccess`.
    Access,
    /// Once this many instructions have executed: `Run`.
    Count(u32),
}

/// Why an address held in a register cannot be accessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    /// It does not fit the address width.
    OutOfRange {
        /// The register.
        register: Register,
        /// Its value.
        value: u64,
    },
    /// It is not a multiple of the word size.
    Misaligned {
        /// The register.
        register: Register,
        /// Its value, which fits the address width.
        addr: Addr,
    },
}

/// Loads `program` as a `Setup` entry: the registers are cleared, and the
/// pc, the budget and the program's status reset; the cache, memory and
/// counters are kept. The event lists each register that held a value, and
/// the pc if it was not already 0.
///
/// Returns [`Flow::Stop`] when the sink stopped the load, or when an
/// invariant no longer holds.
pub(crate) fn load_program(machine: &mut Machine, program: Program, out: &mut Emitter<'_>) -> Flow {
    out.begin_setup();
    machine.access = None;
    let mut changed: Vec<ChangeRef> = machine
        .registers
        .clear()
        .into_iter()
        .map(|register| ChangeRef::Register { register })
        .collect();
    if machine.cpu.pc != 0 {
        changed.push(ChangeRef::Pc);
    }
    machine.cpu = Cpu::default();
    let (format, instructions) = (program.format(), program.len());
    machine.program = Some(Arc::new(program));
    let draft = Draft {
        changed,
        ..Draft::new(EventKind::Setup, None)
    };
    let result = out.emit(machine, draft, || {
        explain::program_loaded(format, instructions)
    });
    end_entry(machine, result, out)
}

/// Whether the loaded program can execute an instruction.
///
/// # Errors
///
/// [`OpError::NoProgram`] with no program loaded; [`OpError::Stopped`] once
/// it has faulted or run out of budget, naming the line it stopped at; and
/// [`OpError::Finished`] once it has executed its last instruction.
pub(crate) fn ready(machine: &Machine) -> Result<(), OpError> {
    let Some(program) = &machine.program else {
        return Err(OpError::NoProgram);
    };
    let at = program.get(machine.cpu.pc);
    match (machine.cpu.halted, at) {
        (None, Some(_)) => Ok(()),
        (Some(_), Some(placed)) => Err(OpError::Stopped { line: placed.line }),
        (_, None) => Err(OpError::Finished),
    }
}

/// [`step_through`] on the cache machine's own memory.
pub(crate) fn step(machine: &mut Machine, out: &mut Emitter<'_>) -> Flow {
    step_through(&mut CacheMemory, machine, out)
}

/// Executes the instruction at the pc as the run's next entry: its access,
/// or its `Complete` event; or, when the budget is spent or its address
/// faults, an `Error` event that stops the program. The caller has checked
/// with [`ready`] that there is an instruction to execute; if there is none,
/// nothing happens.
///
/// Returns [`Flow::Continue`] when the instruction retired, and
/// [`Flow::Stop`] when the op ends here: the sink stopped it, an `Error`
/// event ended it, or an invariant no longer holds.
///
/// A `ld` or `st` is performed by `memory`; everything else is the
/// executor's own.
pub(crate) fn step_through(
    memory: &mut impl MemoryPort,
    machine: &mut Machine,
    out: &mut Emitter<'_>,
) -> Flow {
    let Some(placed) = machine.next_instruction() else {
        return Flow::Stop;
    };
    let (instruction, line) = (placed.instruction.clone(), placed.line);
    if machine.cpu.executed >= INSTRUCTION_BUDGET {
        return stop(
            machine,
            Halted::OutOfBudget,
            None,
            out,
            explain::out_of_budget,
        );
    }
    let request = match instruction {
        Instruction::Load { into, from } => {
            address(machine, &machine.geometry, from).map(|addr| Request::Load {
                addr,
                into,
                origin: Origin::Program,
            })
        }
        Instruction::Store { to, value } => {
            let (value, from) = match value {
                Operand::Register(register) => (machine.registers.get(register), Some(register)),
                Operand::Immediate(value) => (value, None),
            };
            address(machine, &machine.geometry, to).map(|addr| Request::Store {
                addr,
                value,
                from,
                origin: Origin::Program,
            })
        }
        Instruction::Add { into, left, right } => {
            let sum = machine
                .registers
                .get(left)
                .wrapping_add(read(machine, right));
            let next = machine.cpu.pc.saturating_add(1);
            let result = complete(machine, Some((into, sum)), next, out, || {
                explain::add(into, sum)
            });
            return end_entry(machine, result, out);
        }
        Instruction::BranchIfNotZero {
            test,
            target,
            label,
        } => {
            let value = machine.registers.get(test);
            let taken = value != 0;
            let next = if taken {
                target
            } else {
                machine.cpu.pc.saturating_add(1)
            };
            let result = complete(machine, None, next, out, || {
                explain::bnez(test, value, taken.then_some(label.as_str()))
            });
            return end_entry(machine, result, out);
        }
    };
    match request {
        Ok(request) => memory.access(machine, request, out),
        Err(fault) => {
            let geometry = machine.geometry;
            let addr = match fault {
                Fault::Misaligned { addr, .. } => Some(addr),
                Fault::OutOfRange { .. } => None,
            };
            stop(machine, Halted::Faulted, addr, out, || match fault {
                Fault::OutOfRange { register, value } => {
                    explain::fault_out_of_range(&geometry, line, register, value)
                }
                Fault::Misaligned { register, addr } => {
                    explain::fault_misaligned(&geometry, line, register, addr)
                }
            })
        }
    }
}

/// [`execute_through`] on the cache machine's own memory.
pub(crate) fn execute(
    machine: &mut Machine,
    until: Until,
    progress: OpProgress,
    out: &mut Emitter<'_>,
) {
    execute_through(&mut CacheMemory, machine, until, progress, out);
}

/// Executes instructions as entries of one op until `until` says to stop,
/// the program finishes, or an `Error` event ends the op, starting after the
/// `progress` instructions the op has already executed. The caller has
/// checked with [`ready`] that there is an instruction to execute at the
/// op's start.
///
/// At each entry boundary where the op goes on, the sink is told of it with
/// the op's progress there, and may stop the op, which [`resume`] then
/// continues from the same boundary. Then the session's limit is checked:
/// without room for one more entry, an `Error` event stops the program as
/// its budget would, and the op ends.
///
/// [`resume`]: crate::engine::resume
pub(crate) fn execute_through(
    memory: &mut impl MemoryPort,
    machine: &mut Machine,
    until: Until,
    mut progress: OpProgress,
    out: &mut Emitter<'_>,
) {
    loop {
        if progress != OpProgress::START {
            let counted = matches!(until, Until::Count(n) if progress.get() >= n);
            if counted || machine.next_instruction().is_none() {
                return;
            }
            if out.boundary(machine, progress) == Flow::Stop {
                return;
            }
            if !out.has_room() {
                stop(
                    machine,
                    Halted::OutOfBudget,
                    None,
                    out,
                    explain::session_limit,
                );
                return;
            }
        }
        let accesses = machine.next_instruction().is_some_and(|placed| {
            matches!(
                placed.instruction,
                Instruction::Load { .. } | Instruction::Store { .. }
            )
        });
        if step_through(memory, machine, out) == Flow::Stop {
            return;
        }
        progress = progress.next();
        if accesses && until == Until::Access {
            return;
        }
    }
}

/// The address `from` names: a literal as parsed, or a register's value,
/// which must fit the address width and be a multiple of the word size.
fn address(machine: &Machine, space: &impl AddressSpace, from: Address) -> Result<Addr, Fault> {
    let register = match from {
        Address::Literal(addr) => return Ok(addr),
        Address::Register(register) => register,
    };
    let value = machine.registers.get(register);
    let Some(addr) = u32::try_from(value)
        .ok()
        .and_then(|value| space.addr(value))
    else {
        return Err(Fault::OutOfRange { register, value });
    };
    if addr.get() % space.word_bytes() != 0 {
        return Err(Fault::Misaligned { register, addr });
    }
    Ok(addr)
}

/// The value `operand` reads.
fn read(machine: &Machine, operand: Operand) -> u64 {
    match operand {
        Operand::Register(register) => machine.registers.get(register),
        Operand::Immediate(value) => value,
    }
}

/// An instruction without a memory access: one `Complete` event that puts
/// the value of `write`, if any, into its register, retires the instruction
/// and moves the pc to `next`. The event lists the register if its value
/// changed, and the pc if it moved.
fn complete(
    machine: &mut Machine,
    write: Option<(Register, u64)>,
    next: u32,
    out: &mut Emitter<'_>,
    explain: impl FnOnce() -> String,
) -> Result<(), Halt> {
    out.begin_entry(machine);
    machine.access = None;
    let mut changed = Vec::new();
    if let Some((register, value)) = write {
        if machine.registers.set(register, value) {
            changed.push(ChangeRef::Register { register });
        }
    }
    if machine.cpu.retire(next) {
        changed.push(ChangeRef::Pc);
    }
    let draft = Draft {
        changed,
        ..Draft::new(EventKind::Complete, None)
    };
    out.emit(machine, draft, explain)
}

/// Stops the program before the instruction at the pc, which does not
/// execute: an `Error` entry of its own, at `addr` if the fault has one,
/// which ends the op.
fn stop(
    machine: &mut Machine,
    halted: Halted,
    addr: Option<Addr>,
    out: &mut Emitter<'_>,
    explain: impl FnOnce() -> String,
) -> Flow {
    out.begin_entry(machine);
    machine.access = None;
    machine.cpu.halted = Some(halted);
    let draft = Draft {
        addr,
        ..Draft::new(EventKind::Error, None)
    };
    // The op ends here whatever the sink answers.
    let _ = out.emit(machine, draft, explain);
    Flow::Stop
}
