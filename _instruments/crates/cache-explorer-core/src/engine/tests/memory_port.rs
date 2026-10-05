//! The memory port: instruction execution with a memory that is not the
//! cache, a flat array of words that has no lines, hits or misses.

use std::collections::BTreeMap;

use super::*;
use crate::cpu::CpuHost;
use crate::engine::exec::{execute_through, step_through};
use crate::geometry::Geometry;
use crate::isa::AddressSpace;

/// A memory of words by address, which every access takes one entry and one
/// `Complete` event to reach.
#[derive(Default)]
struct Flat {
    words: BTreeMap<u32, u64>,
}

impl<H: EventHost> MemoryPort<H> for Flat {
    fn access(&mut self, host: &mut H, request: Request<H::Core>, out: &mut Emitter<'_>) -> Flow {
        let (Request::Load { core, .. } | Request::Store { core, .. }) = request;
        out.begin_entry(host);
        host.entry_begun();
        let mut changed = Vec::new();
        let explain = match request {
            Request::Load { addr, into, .. } => {
                let value = self.words.get(&addr.get()).copied().unwrap_or(0);
                if host.cpu_mut(core).write(into, value) {
                    changed.push(ChangeRef::Register { register: into });
                }
                format!("flat load {value}")
            }
            Request::Store { addr, value, .. } => {
                let word = host.space().fit_word(value).get();
                self.words.insert(addr.get(), word);
                format!("flat store {word}")
            }
        };
        if request.origin() == Origin::Program {
            let cpu = host.cpu_mut(core);
            let next = cpu.pc().saturating_add(1);
            if cpu.retire(next) {
                changed.push(ChangeRef::Pc);
            }
        }
        let draft = Draft {
            addr: Some(request.addr()),
            changed,
            ..Draft::new(Complete, None)
        };
        let result = out.emit(host, draft, || explain);
        end_entry(host, result, out)
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

    let (flow, stored) = driven.apply(|machine, out| step_through(&mut flat, machine, (), out));
    assert_eq!(flow, Flow::Continue);
    let kinds: Vec<_> = stored.iter().map(|(event, _)| event.kind).collect();
    assert_eq!(kinds, [Complete]);
    assert_eq!(stored[0].0.explain, "flat store 5");
    assert_eq!(flat.words.get(&0xA4), Some(&5));
    // The instruction retired at the access's last event.
    assert_eq!(stored[0].0.changed, [ChangeRef::Pc]);

    let (_, loaded) = driven.apply(|machine, out| step_through(&mut flat, machine, (), out));
    assert_eq!(loaded[0].0.explain, "flat load 5");
    assert_eq!(driven.machine().cpu.register(Register::R1), 5);
    assert_eq!(driven.machine().cpu.pc(), 2);
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
        execute_through(
            &mut flat,
            machine,
            (),
            Until::Count(10),
            OpProgress::START,
            out,
        );
        Flow::Continue
    });
    // Five instructions, one entry and one event each.
    assert_eq!(trace.len(), 5);
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
    assert_eq!(driven.machine().cpu.register(Register::R2), 7);
    assert_eq!(flat.words.get(&0xA8), Some(&7));
    assert_eq!(driven.machine().counters, crate::stats::Counters::default());
}

/// A machine with two CPUs and one memory: the first CPU is the cache
/// machine's own, the second a CPU of its own beside it.
struct Duo {
    machine: Machine,
    second: CpuState,
}

impl CpuHost for Duo {
    type Core = u8;
    type Space = Geometry;

    fn cpu(&self, core: u8) -> &CpuState {
        match core {
            0 => &self.machine.cpu,
            _ => &self.second,
        }
    }

    fn cpu_mut(&mut self, core: u8) -> &mut CpuState {
        match core {
            0 => &mut self.machine.cpu,
            _ => &mut self.second,
        }
    }

    fn space(&self) -> &Geometry {
        self.machine.space()
    }

    fn entry_begun(&mut self) {
        self.machine.entry_begun();
    }
}

impl EventHost for Duo {
    fn machine(&self) -> &Machine {
        &self.machine
    }

    fn machine_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }
}

#[test]
fn one_executor_steps_either_core_of_a_two_core_host() {
    let machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let geometry = machine.geometry;
    let mut duo = Duo {
        machine,
        second: CpuState::default(),
    };
    let mut flat = Flat::default();
    let mut recorder = Recorder::default();
    let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(0), 0);
    let programs = [(0, "st [0x0A4], #5"), (1, "ld r1, [0x0A4]\nld r2, [0x0A4]")];
    for (core, source) in programs {
        let program = Program::parse(&geometry, source, ProgramFormat::Asm).unwrap();
        let flow = load_program(&mut duo, core, program, &mut out);
        assert_eq!(flow, Flow::Continue);
    }

    // The second core loads before anything is stored, the first stores, and
    // the second loads what it stored.
    for core in [1, 0, 1] {
        let flow = step_through(&mut flat, &mut duo, core, &mut out);
        assert_eq!(flow, Flow::Continue);
    }

    assert_eq!(duo.second.register(Register::R1), 0);
    assert_eq!(duo.second.register(Register::R2), 5);
    assert_eq!(duo.machine.cpu.register(Register::R2), 0);
    assert_eq!((duo.machine.cpu.pc(), duo.second.pc()), (1, 2));
    // The entries are the run's, numbered across both cores, and neither
    // the cache nor its counters were touched.
    let labels: Vec<_> = recorder
        .events
        .iter()
        .map(|event| event.label.as_str())
        .collect();
    assert_eq!(labels, ["", "", "1.1", "2.1", "3.1"]);
    assert_eq!(duo.machine.counters, crate::stats::Counters::default());
}
