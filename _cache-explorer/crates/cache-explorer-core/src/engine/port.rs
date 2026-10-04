//! What an instruction needs from a memory system.

use crate::engine::access::access;
use crate::engine::{Emitter, Flow};
use crate::machine::Machine;
use crate::request::Request;

/// The memory an executing program loads from and stores to.
///
/// Instruction execution decides which access an instruction makes and at
/// which address, then hands it to the port. The port performs it as the
/// run's next entry: it moves the machine, reports each micro-event through
/// `out`, and for a program's request retires the instruction at the entry's
/// last event, putting a load's word into its register. It returns
/// [`Flow::Stop`] when the sink stopped the entry or the machine broke an
/// invariant, as the executor expects, and [`Flow::Continue`] otherwise. The
/// request's address has been checked against the address width and the
/// word size.
pub(crate) trait MemoryPort {
    /// Performs `request` as the run's next entry.
    fn access(&mut self, machine: &mut Machine, request: Request, out: &mut Emitter<'_>) -> Flow;
}

/// The cache machine's own memory: an access runs through the cache, a miss
/// going on to memory, with the events of its case.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CacheMemory;

impl MemoryPort for CacheMemory {
    fn access(&mut self, machine: &mut Machine, request: Request, out: &mut Emitter<'_>) -> Flow {
        access(machine, request, out)
    }
}
