//! What an instruction needs from a memory system.

use crate::cpu::CpuHost;
use crate::engine::access::access;
use crate::engine::{Emitter, Flow};
use crate::machine::Machine;
use crate::request::Request;

/// The memory an executing program loads from and stores to, on a host `H`
/// that holds the CPUs.
///
/// Instruction execution decides which access an instruction makes, for which
/// core and at which address, then hands it to the port. The port performs it
/// as the run's next entry: it moves the host, reports each micro-event
/// through `out`, and for a program's request retires the instruction at the
/// entry's last event through [`CpuState::retire`], putting a load's word into
/// its register through [`CpuState::write`]. It returns [`Flow::Stop`] when
/// the sink stopped the entry or the machine broke an invariant, as the
/// executor expects, and [`Flow::Continue`] otherwise. The request's address
/// has been checked against the address width and the word size.
///
/// [`CpuState::retire`]: crate::cpu::CpuState::retire
/// [`CpuState::write`]: crate::cpu::CpuState::write
pub(crate) trait MemoryPort<H: CpuHost> {
    /// Performs `request` as the run's next entry.
    fn access(&mut self, host: &mut H, request: Request<H::Core>, out: &mut Emitter<'_>) -> Flow;
}

/// The cache machine's own memory: an access runs through the cache, a miss
/// going on to memory, with the events of its case.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CacheMemory;

impl MemoryPort<Machine> for CacheMemory {
    fn access(&mut self, machine: &mut Machine, request: Request, out: &mut Emitter<'_>) -> Flow {
        access(machine, request, out)
    }
}
