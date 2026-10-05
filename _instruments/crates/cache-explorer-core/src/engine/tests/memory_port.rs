//! The memory port: instruction execution with a memory that is not the
//! cache, a flat array of words that has no lines, hits or misses.

use std::collections::BTreeMap;

use super::*;
use crate::engine::exec::{execute_through, step_through};

/// A memory of words by address, which every access takes one entry and one
/// `Complete` event to reach.
#[derive(Default)]
struct Flat {
    words: BTreeMap<u32, u64>,
}

impl MemoryPort for Flat {
    fn access(&mut self, machine: &mut Machine, request: Request, out: &mut Emitter<'_>) -> Flow {
        out.begin_entry(machine);
        machine.access = None;
        let mut changed = Vec::new();
        let explain = match request {
            Request::Load { addr, into, .. } => {
                let value = self.words.get(&addr.get()).copied().unwrap_or(0);
                if machine.registers.set(into, value) {
                    changed.push(ChangeRef::Register { register: into });
                }
                format!("flat load {value}")
            }
            Request::Store { addr, value, .. } => {
                let word = machine.geometry.fit_word(value).get();
                self.words.insert(addr.get(), word);
                format!("flat store {word}")
            }
        };
        if request.origin() == Origin::Program {
            let next = machine.cpu.pc.saturating_add(1);
            if machine.cpu.retire(next) {
                changed.push(ChangeRef::Pc);
            }
        }
        let draft = Draft {
            addr: Some(request.addr()),
            changed,
            ..Draft::new(Complete, None)
        };
        let result = out.emit(machine, draft, || explain);
        end_entry(machine, result, out)
    }
}

const PROGRAM: &str = concat!(
    "        st [0x0A4], #5\n",
    "        ld r1, [0x0A4]\n",
    "        add r1, r1, #2\n",
    "        st [0x0A8], r1\n",
    "        ld r2, [0x0A8]",
);

#[test]
fn a_step_hands_its_access_to_the_port_given() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load(PROGRAM);
    let cold = driven.machine().clone();
    let mut flat = Flat::default();

    let (flow, stored) = driven.apply(|machine, out| step_through(&mut flat, machine, out));
    assert_eq!(flow, Flow::Continue);
    let kinds: Vec<_> = stored.iter().map(|(event, _)| event.kind).collect();
    assert_eq!(kinds, [Complete]);
    assert_eq!(stored[0].0.explain, "flat store 5");
    assert_eq!(flat.words.get(&0xA4), Some(&5));
    // The instruction retired at the access's last event.
    assert_eq!(stored[0].0.changed, [ChangeRef::Pc]);

    let (_, loaded) = driven.apply(|machine, out| step_through(&mut flat, machine, out));
    assert_eq!(loaded[0].0.explain, "flat load 5");
    assert_eq!(driven.machine().registers.get(Register::R1), 5);
    assert_eq!(driven.machine().cpu.pc, 2);
    // Neither the cache nor its counters were touched.
    assert_eq!(driven.machine().cache, cold.cache);
    assert_eq!(driven.machine().counters, cold.counters);
}

#[test]
fn a_run_goes_through_the_port_to_the_end_of_the_program() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load(PROGRAM);
    let mut flat = Flat::default();
    let (_, trace) = driven.apply(|machine, out| {
        execute_through(&mut flat, machine, Until::Count(10), OpProgress::START, out);
        Flow::Continue
    });
    // Five instructions, one entry and one event each.
    assert_eq!(trace.len(), 5);
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
    assert_eq!(driven.machine().registers.get(Register::R2), 7);
    assert_eq!(flat.words.get(&0xA8), Some(&7));
    assert_eq!(driven.machine().counters, crate::stats::Counters::default());
}
