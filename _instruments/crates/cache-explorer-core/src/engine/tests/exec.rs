//! Programs: loading one, each instruction retiring at its last event, the
//! changed cells of every program event, stopping mid-instruction, faults,
//! the budget, and hand-issued accesses beside a program; and the room an op
//! needs in the log, runs that stop at their count, and ops stopped at a
//! boundary and resumed from it.

use super::*;

/// Frame 3b's program, as `golden::FRAME_3B` has it. The unit tests do not
/// depend on the `golden` feature, so they keep this copy.
/// The CPU's place in its program: the pc, the instructions executed, and
/// why it stopped, if it did.
fn place(cpu: &CpuState) -> (u32, u32, Option<Halted>) {
    (cpu.pc(), cpu.executed(), cpu.halted())
}

const FRAME_3B: &str = concat!(
    "  st  [0x0B0], #5\n",
    "  ld  r1, [0x0A4]\n",
    "  st  [0x1A4], #7\n",
    "  ld  r2, [0x0A8]\n",
    "  ld  r3, [0x2A4]\n",
    "  ld  r4, [0x1A4]",
);

/// A loop over six words 0x44 apart, within the first 24 lines: each is
/// loaded, incremented and stored back through a register, beside a store
/// of an immediate and a load into r0.
const STRIDED: &str = concat!(
    "        add r1, r0, #0\n",
    "        add r2, r0, #6\n",
    "loop:   ld r3, [r1]\n",
    "        add r3, r3, #1\n",
    "        st [r1], r3\n",
    "        st [0x040], #-1\n",
    "        add r1, r1, #0x44\n",
    "        add r2, r2, #-1\n",
    "        bnez r2, loop\n",
    "        ld r0, [0x040]",
);

#[test]
fn a_program_load_resets_the_cpu_and_keeps_the_cache() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    let trace = driven.load("ld r1, [0x0A4]\nadd r2, r1, #1\nadd r3, r0, #7");
    let (setup, after) = &trace[0];
    assert_eq!(trace.len(), 1);
    assert_eq!(
        (setup.kind, setup.entry, setup.seq, setup.label.as_str()),
        (EventKind::Setup, None, 1, "")
    );
    // Nothing held a value and the pc was 0: nothing changed.
    assert!(setup.changed.is_empty());
    assert_eq!(
        setup.explain,
        "Loaded a program of 3 instructions. Registers and the pc are reset; the cache and \
         memory keep their contents."
    );
    assert_eq!(
        (setup.cause, setup.addr, setup.set, setup.way),
        (None, None, None, None)
    );
    assert_eq!(place(&after.cpu), (0, 0, None));
    assert_eq!(after.run.entries, 0);

    driven.step();
    driven.step();
    let before = driven.machine().clone();
    assert_eq!(before.cpu.register(Register::R2), 54);
    assert_eq!(before.cpu.pc(), 2);

    let trace = driven.load("ld r4, [0x0A8]");
    let (setup, after) = &trace[0];
    assert_eq!(
        setup.changed,
        [
            register(Register::R1),
            register(Register::R2),
            ChangeRef::Pc
        ]
    );
    assert!(registers_zero(&after.cpu));
    assert_eq!(place(&after.cpu), (0, 0, None));
    assert_eq!(after.access, None);
    // The cache, memory, counters and entries are kept.
    assert_eq!(after.cache, before.cache);
    assert_eq!(after.memory, before.memory);
    assert_eq!(after.counters, before.counters);
    assert_eq!(after.run.entries, before.run.entries);
    assert_eq!(after.cpu.program().map(|program| program.len()), Some(1));

    // The new program's load hits the line the old one brought in.
    let kinds = kinds(
        &driven
            .step()
            .into_iter()
            .map(|(event, _)| event)
            .collect::<Vec<_>>(),
    );
    assert_eq!(kinds, [Decode, Hit, Complete]);
}

/// Checks that each instruction of `source`, on `config`, retires at its
/// last event: the pc moves there and not before, the executed count goes up
/// by one, and the entry is numbered as the instruction's.
fn check_instruction_retirement(config: Config, source: &str) {
    let mut driven = Driven::new(config);
    driven.load(source);
    let mut entries = 0;
    while ready(driven.machine()).is_ok() {
        let before = driven.machine().clone();
        let trace = driven.step();
        entries += 1;
        let (last, after) = trace.last().unwrap();
        // In flight, the instruction is the one at the pc.
        for (event, during) in &trace[..trace.len() - 1] {
            assert_eq!(place(&during.cpu), place(&before.cpu), "{}", event.label);
            assert!(!event.changed.contains(&ChangeRef::Pc), "{}", event.label);
        }
        assert_eq!(
            after.cpu.executed(),
            before.cpu.executed() + 1,
            "instruction {entries}"
        );
        assert_eq!(
            last.changed.contains(&ChangeRef::Pc),
            after.cpu.pc() != before.cpu.pc(),
            "instruction {entries}"
        );
        assert!(!after.in_flight(), "instruction {entries}");
        assert_eq!(after.run.entries, entries, "instruction {entries}");
        assert!(
            trace.iter().all(|(event, _)| event.entry == Some(entries)),
            "instruction {entries}"
        );
    }
    let program = driven.machine().cpu.program().unwrap();
    assert_eq!(driven.machine().cpu.pc(), program.len());
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
}

cases! {
    check_instruction_retirement;
    each_instruction_of_frame_3b_retires_at_its_last_event_on_two_way_lru =>
        (preset(Preset::TwoWayLru), FRAME_3B),
    each_instruction_of_a_strided_loop_retires_at_its_last_event_on_two_way_fifo =>
        (two_way(Replacement::Fifo), STRIDED),
    each_instruction_of_a_strided_loop_retires_at_its_last_event_under_write_no_allocate =>
        (
            direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
            STRIDED,
        ),
    each_instruction_of_frame_3b_retires_at_its_last_event_when_timed =>
        (timed(preset(Preset::TwoWayLru), 1, 20), FRAME_3B),
}

#[test]
fn every_program_event_lists_exactly_what_it_changed() {
    let mut kinds_seen = std::collections::BTreeSet::new();
    for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
        for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
            for allocate_policy in [
                AllocatePolicy::WriteAllocate,
                AllocatePolicy::WriteNoAllocate,
            ] {
                for timing in [None, Some(timing(2, 5))] {
                    let config = Config {
                        timing,
                        ..config(128, 2, replacement, write_policy, allocate_policy)
                    };
                    let mut driven = Driven::new(config);
                    let mut before = driven.machine().clone();
                    let mut trace = driven.load(STRIDED);
                    for _ in 0..7 {
                        trace.extend(driven.step());
                    }
                    // A second load, midway, clears registers that hold values.
                    trace.extend(driven.load(STRIDED));
                    trace.extend(driven.finish());
                    for (event, after) in &trace {
                        kinds_seen.insert(format!("{:?}", event.kind));
                        assert_eq!(
                            event.changed,
                            diff(&before, after),
                            "{} {:?}",
                            event.label,
                            event.kind
                        );
                        before = after.clone();
                    }
                }
            }
        }
    }
    for kind in [
        "Setup",
        "Complete",
        "Write",
        "WriteThrough",
        "MemWrite",
        "Rank",
    ] {
        assert!(kinds_seen.contains(kind), "{kind} never came up");
    }
}

#[test]
fn stopping_after_any_event_of_an_instruction_leaves_the_machine_as_the_full_step_has_it() {
    for timing in [None, Some(timing(1, 20))] {
        let mut driven = Driven::new(Config {
            timing,
            ..preset(Preset::TwoWayLru)
        });
        driven.load(FRAME_3B);
        for _ in 0..4 {
            driven.step();
        }
        // Access #5, `ld r3, [0x2A4]`, with its dirty eviction and rank.
        let base = driven.machine().clone();
        let full = driven.step();
        for stop in 1..=full.len() {
            let mut machine = base.clone();
            let mut recorder = Recorder {
                stop_after: Some(stop),
                ..Recorder::default()
            };
            let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(9), 0);
            // The sink stops the step, even at its last event.
            assert_eq!(
                step(&mut machine, &mut out),
                Flow::Stop,
                "stopped after {stop}"
            );
            assert_eq!(recorder.events.len(), stop, "stopped after {stop}");
            assert_eq!(machine, full[stop - 1].1, "stopped after {stop}");

            let mut quiet_machine = base.clone();
            let mut quiet = Quiet {
                stop_after: Some(stop),
                ..Quiet::default()
            };
            let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(9), 0);
            step(&mut quiet_machine, &mut out);
            assert_eq!(quiet.seen, stop, "stopped after {stop}");
            assert_eq!(quiet_machine, machine, "quietly stopped after {stop}");
        }
    }
}

/// Checks that the program `source`, whose last instruction's address in a
/// register does not fit, faults there with an error at `at` (or none), and
/// changes only the program's status and the entry count.
fn check_register_address_fault(source: &str, at: Option<Addr>) {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load(source);
    let lines = source.lines().count();
    for _ in 1..lines {
        driven.step();
    }
    let before = driven.machine().clone();
    let (flow, trace) = driven.apply(step);
    assert_eq!(flow, Flow::Stop);
    let [(error, after)] = trace.as_slice() else {
        panic!("one event: {trace:?}");
    };
    assert_eq!(error.kind, EventKind::Error);
    assert_eq!(error.entry, Some(before.run.entries + 1));
    assert_eq!((error.addr, error.cause), (at, None));
    assert!(error.changed.is_empty());
    assert!(
        error.explain.starts_with(&format!("Line {lines}: ")),
        "{}",
        error.explain
    );
    assert!(
        error.explain.ends_with(" The program stopped."),
        "{}",
        error.explain
    );
    // Only the program's status and the entry count change: the pc stays
    // at the instruction, which does not count against the budget.
    let mut halted = before.cpu.clone();
    halted.halt(Halted::Faulted);
    let expected = Machine {
        cpu: halted,
        run: RunPlace {
            entries: before.run.entries + 1,
            ..before.run
        },
        ..before
    };
    assert_eq!(after, &expected);
    let line = u32::try_from(lines).unwrap();
    assert_eq!(ready(after), Err(OpError::Stopped { line }));
}

cases! {
    check_register_address_fault;
    // r2 holds 4096, one past the last 12-bit address.
    a_register_address_past_the_width_faults_where_it_is =>
        ("add r2, r0, #0xFFF\nadd r2, r2, #1\nld r1, [r2]", None),
    // r2 holds 166, within the width but not a multiple of 4.
    a_misaligned_register_address_faults_where_it_is =>
        ("add r2, r0, #0x0A6\nst [r2], #1", Some(addr(0x0A6))),
    // A register wider than every address width.
    a_register_wider_than_every_address_faults_where_it_is =>
        ("add r3, r0, #-1\nst [r3], r3", None),
}

#[test]
fn the_budget_replaces_the_instruction_after_its_last() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nloop: bnez r1, loop");
    driven.step();
    // As if the loop had run until one instruction of the budget was left.
    driven.run.machine.cpu.set_executed(INSTRUCTION_BUDGET - 1);
    let trace = driven.step();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].0.kind, Complete);
    assert_eq!(driven.machine().cpu.executed(), INSTRUCTION_BUDGET);
    let before = driven.machine().clone();

    let (flow, trace) = driven.apply(step);
    assert_eq!(flow, Flow::Stop);
    let [(error, after)] = trace.as_slice() else {
        panic!("one event: {trace:?}");
    };
    assert_eq!(error.kind, EventKind::Error);
    assert_eq!(error.explain, explain::out_of_budget());
    assert_eq!((error.addr, error.cause), (None, None));
    let mut halted = before.cpu.clone();
    halted.halt(Halted::OutOfBudget);
    assert_eq!(after.cpu, halted);
    assert_eq!(ready(after), Err(OpError::Stopped { line: 2 }));

    // Loading the program again gives it a fresh budget.
    driven.load("add r1, r0, #1\nloop: bnez r1, loop");
    assert_eq!(place(&driven.machine().cpu), (0, 0, None));
    assert_eq!(driven.step()[0].0.kind, Complete);
}

#[test]
fn a_program_that_finishes_on_its_last_budgeted_instruction_does_not_run_out() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nadd r1, r1, #1");
    driven.step();
    driven.run.machine.cpu.set_executed(INSTRUCTION_BUDGET - 1);
    let trace = driven.step();
    assert_eq!(trace[0].0.kind, Complete);
    assert_eq!(driven.machine().cpu.halted(), None);
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
}

#[test]
fn ready_says_why_there_is_nothing_to_execute() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    assert_eq!(ready(driven.machine()), Err(OpError::NoProgram));
    // With nothing to execute, a step does nothing.
    let before = driven.machine().clone();
    let (flow, trace) = driven.apply(step);
    assert_eq!((flow, trace.len()), (Flow::Stop, 0));
    assert_eq!(driven.machine(), &before);

    driven.load("; one instruction\nadd r1, r0, #1\ndone:");
    assert_eq!(ready(driven.machine()), Ok(()));
    driven.step();
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
    let before = driven.machine().clone();
    let (flow, trace) = driven.apply(step);
    assert_eq!((flow, trace.len()), (Flow::Stop, 0));
    assert_eq!(driven.machine(), &before);
}

#[test]
fn a_recorded_event_keeps_no_spare_room() {
    // A program's load of the least recently used line of a fully
    // associative set of `MAX_LINES` ways moves every rank: its `Rank` event
    // lists every way and the pc, the longest list an access makes, and its
    // line names every way. The session keeps every event for as long as it
    // holds the log, so no event keeps room it does not use.
    let mut driven = Driven::new(config(
        MAX_LINES * 16,
        MAX_LINES,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    let mut trace = Vec::new();
    for line in 0..MAX_LINES {
        trace.extend(driven.issue(load(line * 0x10, Register::R1)));
    }
    trace.extend(driven.load("ld r1, [0x000]"));
    trace.extend(driven.step());
    let (rank, _) = trace.last().unwrap();
    assert_eq!(rank.kind, Rank);
    assert_eq!(rank.changed.len(), MAX_LINES as usize + 1);
    assert_eq!(rank.changed.last(), Some(&ChangeRef::Pc));
    for (event, _) in &trace {
        assert_eq!(
            (event.changed.capacity(), event.explain.capacity()),
            (event.changed.len(), event.explain.len()),
            "{}",
            event.explain
        );
    }
}

#[test]
fn a_branch_moves_the_pc_to_its_label_or_on() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load(
        "add r1, r0, #2\nloop: add r1, r1, #-1\nbnez r1, loop\nbnez r0, loop\nbnez r1, end\nend:",
    );
    let pcs: Vec<(u32, String)> = driven
        .finish()
        .into_iter()
        .map(|(event, after)| (after.cpu.pc(), event.explain))
        .collect();
    assert_eq!(
        pcs,
        [
            (1, "add: r1 \u{2190} 2. No memory access.".to_owned()),
            (2, "add: r1 \u{2190} 1. No memory access.".to_owned()),
            (1, "bnez: r1 is 1, branch taken to loop.".to_owned()),
            (2, "add: r1 \u{2190} 0. No memory access.".to_owned()),
            (3, "bnez: r1 is 0, branch not taken.".to_owned()),
            (4, "bnez: r0 is 0, branch not taken.".to_owned()),
            (5, "bnez: r1 is 0, branch not taken.".to_owned()),
        ]
    );
    // A branch taken to the label after the last instruction finishes the
    // program, and one to itself leaves the pc where it was.
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nbnez r1, end\nadd r2, r0, #5\nend:");
    driven.finish();
    assert_eq!(place(&driven.machine().cpu), (3, 2, None));
    assert_eq!(driven.machine().cpu.register(Register::R2), 0);
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nloop: bnez r1, loop");
    driven.step();
    let trace = driven.step();
    assert!(trace[0].0.changed.is_empty());
    assert_eq!(driven.machine().cpu.pc(), 1);
}

#[test]
fn a_hand_issued_access_leaves_the_program_where_it_is() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #5\nadd r2, r1, #0");
    driven.step();
    let cpu = place(&driven.machine().cpu);
    let trace = driven.issue(load(0x0A4, Register::R1));
    assert_eq!(place(&driven.machine().cpu), cpu);
    assert!(trace
        .iter()
        .all(|(event, _)| !event.changed.contains(&ChangeRef::Pc)));
    assert_eq!(
        trace.last().unwrap().0.explain,
        "Read word 1 of way 0 (53) into r1. A program is loaded, so its r1 now holds 53."
    );
    driven.issue(store(0x0A8, 9));
    assert_eq!(place(&driven.machine().cpu), cpu);
    // The program reads the r1 the hand-issued load wrote.
    driven.step();
    assert_eq!(driven.machine().cpu.register(Register::R2), 53);
    // Once the program has finished, a hand-issued load says nothing of it.
    let trace = driven.issue(load(0x0A4, Register::R1));
    assert_eq!(
        trace.last().unwrap().0.explain,
        "Read word 1 of way 0 (53) into r1."
    );
}

#[test]
fn a_store_from_a_register_names_the_bits_it_keeps_on_its_result_line_only() {
    let config = Config {
        word_bytes: WordBytes::Two,
        ..direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate)
    };
    let mut driven = Driven::new(config);
    driven
        .load("add r2, r0, #70000\nst [0x0A4], r2\nst [0x0A4], #7\nadd r2, r0, #7\nst [0x0A4], r2");
    let explains = |trace: Vec<(MicroEvent, Machine)>| -> Vec<(EventKind, String)> {
        trace
            .into_iter()
            .filter(|(event, _)| matches!(event.kind, Write | WriteThrough))
            .map(|(event, _)| (event.kind, event.explain))
            .collect()
    };
    driven.step();
    assert_eq!(
        explains(driven.step()),
        [
            (
                Write,
                "r2 = 70,000 does not fit a 2-byte word, so its low 16 bits, 4464, are stored. \
                 Write 4464 into word 2 of way 0."
                    .to_owned()
            ),
            (
                WriteThrough,
                "Write-through: 4464 also goes to memory at 0x0A4.".to_owned()
            ),
        ]
    );
    // An immediate, and a register that fits, say nothing more.
    for _ in 0..2 {
        let written = explains(driven.step());
        assert!(
            written
                .iter()
                .all(|(_, explain)| explain.starts_with("Write")),
            "{written:?}"
        );
    }
    let written = explains(driven.step());
    assert!(
        written
            .iter()
            .all(|(_, explain)| explain.starts_with("Write")),
        "{written:?}"
    );
}

#[test]
fn a_trace_reads_into_r1_and_writes_its_values() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    let setup = driven.load_as("R 0x0A4\nW 0x0A8 9\nR 0x0A8", ProgramFormat::Trace);
    assert_eq!(
        setup[0].0.explain,
        "Loaded a trace of 3 accesses. Registers and the pc are reset; the cache and memory \
         keep their contents."
    );
    let last: Vec<String> = (0..3)
        .map(|_| driven.step().pop().unwrap().0.explain)
        .collect();
    assert_eq!(
        last,
        [
            "Read word 1 of way 0 (53) into r1.",
            "Write 9 into word 2 of way 0 and set its dirty bit. Memory at 0x0A8 is now stale.",
            "Read word 2 of way 0 (9) into r1.",
        ]
    );
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
}

/// Keeps every event, and every boundary inside an op with the machine
/// there, its progress and the id of the next event.
#[derive(Default)]
struct Boundaries {
    events: Vec<MicroEvent>,
    boundaries: Vec<(Machine, OpProgress, u64)>,
}

impl EventSink for Boundaries {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }

    fn boundary(&mut self, machine: &Machine, progress: OpProgress) -> Flow {
        let next_id = self.events.len() as u64;
        self.boundaries.push((machine.clone(), progress, next_id));
        Flow::Continue
    }
}

/// A machine on `direct-mapped-8` with the assembly program `source` loaded.
#[track_caller]
fn loaded(source: &str) -> Machine {
    let mut machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let op = Op::Program {
        source: source.to_owned(),
        format: ProgramFormat::Asm,
    };
    assert_eq!(apply_op(&mut machine, &op, 0).0, Ok(()));
    machine
}

/// A loop without an exit or a memory access.
const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

#[test]
fn an_op_needs_room_for_one_more_entry_in_the_log() {
    let limit = u64::from(SESSION_EVENT_LIMIT);
    let room = u64::from(MAX_EVENTS_PER_ENTRY);
    let issue = Op::Issue {
        kind: AccessKind::Read,
        addr: addr(0x0A4),
        value: None,
    };
    let mut machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let before = machine.clone();
    for op in [issue.clone(), Op::Step, Op::Run { n: 0 }] {
        let (result, events) = apply_op(&mut machine, &op, limit - room + 1);
        assert_eq!(result, Err(OpError::SessionFull), "{op:?}");
        assert!(events.is_empty(), "{op:?}");
        assert_eq!(machine, before, "{op:?}");
    }
    let (result, events) = apply_op(&mut machine, &issue, limit - room);
    assert_eq!(result, Ok(()));
    assert_eq!(events.first().map(|event| event.id), Some(limit - room));
    assert!(events.len() < MAX_EVENTS_PER_ENTRY as usize);
}

#[test]
fn an_op_needs_room_for_one_more_op_a_link_can_hold() {
    let limit = u64::from(SESSION_EVENT_LIMIT);
    let room = u64::from(MAX_EVENTS_PER_ENTRY);
    let issue = Op::Issue {
        kind: AccessKind::Read,
        addr: addr(0x0A4),
        value: None,
    };
    let apply_as = |machine: &mut Machine, op: &Op, index: u32, first_id: u64| {
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(index), first_id);
        (apply(machine, op, &mut out), recorder.events)
    };
    let mut machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let before = machine.clone();
    // Op MAX_SHARE_OPS, from 0, would be one more than a link can hold,
    // whatever the op; the room for events is checked first.
    for op in [issue.clone(), Op::Step, Op::Run { n: 0 }] {
        let (result, events) = apply_as(&mut machine, &op, MAX_SHARE_OPS, 0);
        assert_eq!(result, Err(OpError::LinkFull), "{op:?}");
        assert!(events.is_empty(), "{op:?}");
        assert_eq!(machine, before, "{op:?}");
        let (result, _) = apply_as(&mut machine, &op, MAX_SHARE_OPS, limit - room + 1);
        assert_eq!(result, Err(OpError::SessionFull), "{op:?}");
        assert_eq!(machine, before, "{op:?}");
    }
    let (result, events) = apply_as(&mut machine, &issue, MAX_SHARE_OPS - 1, 0);
    assert_eq!(result, Ok(()));
    assert!(!events.is_empty());
}

#[test]
fn a_loop_that_runs_out_of_room_stops_with_an_error() {
    let limit = u64::from(SESSION_EVENT_LIMIT);
    let room = u64::from(MAX_EVENTS_PER_ENTRY);
    for op in [Op::StepAccess, Op::Run { n: 5 }] {
        let mut machine = loaded(ENDLESS);
        // Room for the first instruction's entry, and no more.
        let (result, events) = apply_op(&mut machine, &op, limit - room);
        assert_eq!(result, Ok(()), "{op:?}");
        let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, [Complete, EventKind::Error], "{op:?}");
        let error = events.last().unwrap();
        assert_eq!(error.explain, explain::session_limit(), "{op:?}");
        assert_eq!((error.entry, error.seq), (Some(2), 1), "{op:?}");
        assert!(error.changed.is_empty(), "{op:?}");
        assert_eq!(machine.cpu.halted(), Some(Halted::OutOfBudget), "{op:?}");
        assert_eq!(
            machine.cpu.pc(),
            1,
            "the pc stays at the instruction not executed"
        );
        assert_eq!(ready(&machine), Err(OpError::Stopped { line: 2 }), "{op:?}");
    }
}

#[test]
fn a_run_stops_at_its_count_and_a_step_access_after_an_access() {
    let source = "add r1, r0, #1\nadd r1, r1, #1\nld r2, [0x0A4]\nld r3, [0x0A8]";
    let mut machine = loaded(source);
    let (result, events) = apply_op(&mut machine, &Op::StepAccess, 1);
    assert_eq!(result, Ok(()));
    let labels: Vec<&str> = events.iter().map(|event| event.label.as_str()).collect();
    assert_eq!(labels, ["1.1", "2.1", "3.1", "3.2", "3.3", "3.4", "3.5"]);
    assert_eq!(machine.cpu.pc(), 3);
    let (result, events) = apply_op(&mut machine, &Op::Run { n: 1 }, 8);
    assert_eq!(result, Ok(()));
    assert_eq!(events.last().map(|event| event.label.as_str()), Some("4.3"));
    // A run of more than is left stops when the program finishes.
    let mut machine = loaded(source);
    let (result, events) = apply_op(&mut machine, &Op::Run { n: 10 }, 1);
    assert_eq!(result, Ok(()));
    assert!(events.iter().all(|event| event.kind != EventKind::Error));
    assert_eq!(ready(&machine), Err(OpError::Finished));
    // A run's count is checked before anything else about the program.
    let mut empty = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    for n in [0, MAX_RUN_STEPS + 1] {
        assert_eq!(
            apply_op(&mut empty, &Op::Run { n }, 0).0,
            Err(OpError::RunSteps),
            "n {n}"
        );
    }
    assert_eq!(
        apply_op(&mut empty, &Op::Run { n: MAX_RUN_STEPS }, 0).0,
        Err(OpError::NoProgram)
    );
}

#[test]
fn an_op_resumed_from_a_boundary_inside_it_goes_on_as_it_would_have() {
    let source = concat!(
        "        add r3, r0, #3\n",
        "loop:   ld r1, [0x0A4]\n",
        "        add r3, r3, #-1\n",
        "        bnez r3, loop\n",
        "        add r1, r0, #1\n",
        "        add r1, r1, #1\n",
        "        st [0x1A4], r1",
    );
    for op in [Op::Run { n: 9 }, Op::StepAccess] {
        let mut whole = loaded(source);
        if op == Op::StepAccess {
            // Past the loop, to the two adds before the store.
            let (result, _) = apply_op(&mut whole, &Op::Run { n: 10 }, 1);
            assert_eq!(result, Ok(()), "{op:?}");
        }
        let start = whole.clone();
        let mut sink = Boundaries::default();
        let mut out = Emitter::new(&mut sink, OpIndex::new_unchecked(1), 0);
        assert_eq!(apply(&mut whole, &op, &mut out), Ok(()), "{op:?}");
        assert!(sink.boundaries.len() >= 2, "{op:?}");
        for (machine, progress, next_id) in &sink.boundaries {
            let mut resumed = machine.clone();
            let mut recorder = Recorder::default();
            let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), *next_id);
            assert_eq!(
                resume(&mut resumed, &op, *progress, &mut out),
                Ok(()),
                "{op:?} from {}",
                progress.get()
            );
            assert_eq!(resumed, whole, "{op:?} from {}", progress.get());
            let before = sink
                .events
                .get(..usize::try_from(*next_id).unwrap())
                .unwrap();
            assert_eq!(
                [before, recorder.events.as_slice()].concat(),
                sink.events,
                "{op:?} from {}",
                progress.get()
            );
        }
        // From the op's start, resuming is applying.
        let mut again = start.clone();
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), 0);
        assert_eq!(
            resume(&mut again, &op, OpProgress::START, &mut out),
            Ok(()),
            "{op:?}"
        );
        assert_eq!(again, whole, "{op:?}");
    }
}

/// Keeps every event, and stops the op at its `stop_at`-th boundary,
/// counting from 0, noting the progress there.
struct StopAtBoundary {
    events: Vec<MicroEvent>,
    stop_at: usize,
    seen: usize,
    stopped: Option<OpProgress>,
}

impl EventSink for StopAtBoundary {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }

    fn boundary(&mut self, _machine: &Machine, progress: OpProgress) -> Flow {
        self.seen += 1;
        if self.seen == self.stop_at + 1 {
            self.stopped = Some(progress);
            Flow::Stop
        } else {
            Flow::Continue
        }
    }
}

#[test]
fn an_op_stopped_at_a_boundary_goes_on_from_there_when_resumed() {
    let source = concat!(
        "        add r3, r0, #3\n",
        "loop:   ld r1, [0x0A4]\n",
        "        add r3, r3, #-1\n",
        "        bnez r3, loop\n",
        "        st [0x1A4], r3",
    );
    let op = Op::Run { n: 11 };
    let mut whole = loaded(source);
    let start = whole.clone();
    let (result, events) = apply_op(&mut whole, &op, 0);
    assert_eq!(result, Ok(()));
    let entries = events.iter().filter(|event| event.seq == 1).count();
    assert_eq!(entries, 11);
    // A boundary lies between every two of the op's entries.
    for stop_at in 0..entries - 1 {
        let mut machine = start.clone();
        let mut sink = StopAtBoundary {
            events: Vec::new(),
            stop_at,
            seen: 0,
            stopped: None,
        };
        let mut out = Emitter::new(&mut sink, OpIndex::new_unchecked(1), 0);
        assert_eq!(
            apply(&mut machine, &op, &mut out),
            Ok(()),
            "stopped at boundary {stop_at}"
        );
        let progress = sink.stopped.unwrap();
        assert_eq!(
            progress.get() as usize,
            stop_at + 1,
            "stopped at boundary {stop_at}"
        );
        // Stopped at an entry boundary: its last event ends an entry.
        let next = events.get(sink.events.len()).unwrap();
        assert_eq!(next.seq, 1, "stopped at boundary {stop_at}");
        let mut recorder = Recorder::default();
        let first = sink.events.len() as u64;
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), first);
        assert_eq!(
            resume(&mut machine, &op, progress, &mut out),
            Ok(()),
            "stopped at boundary {stop_at}"
        );
        assert_eq!(machine, whole, "stopped at boundary {stop_at}");
        assert_eq!(
            [sink.events, recorder.events].concat(),
            events,
            "stopped at boundary {stop_at}"
        );
    }
}

#[test]
fn a_new_run_starts_over_with_the_program_kept_and_the_next_run_number() {
    let mut machine = loaded("ld r1, [0x0A4]\nst [0x0A8], #7");
    for (index, op) in [Op::Step, Op::Step].into_iter().enumerate() {
        assert_eq!(apply_op(&mut machine, &op, 0).0, Ok(()), "step {index}");
    }
    assert_eq!(machine.run.entries, 2);
    let config = preset(Preset::TwoWayLru);
    let (result, events) = apply_op(&mut machine, &Op::Config { config }, 0);
    assert_eq!(result, Ok(()));
    let [setup] = events.as_slice() else {
        panic!("one setup: {events:?}");
    };
    assert_eq!(
        (setup.kind, setup.run, setup.entry),
        (EventKind::Setup, 2, None)
    );
    assert!(setup.changed.is_empty());
    let mut expected = Machine::start(config).unwrap();
    expected.run.run = 2;
    let program = Program::parse(
        &expected.geometry,
        "ld r1, [0x0A4]\nst [0x0A8], #7",
        ProgramFormat::Asm,
    )
    .unwrap();
    expected.cpu.load(program);
    assert_eq!(machine, expected);
    // An invalid configuration is refused and changes nothing.
    let invalid = Config {
        address_bits: 0,
        ..config
    };
    let (result, events) = apply_op(&mut machine, &Op::Config { config: invalid }, 0);
    assert_eq!(
        result,
        Err(OpError::Config(invalid.validate().unwrap_err()))
    );
    assert!(events.is_empty());
    assert_eq!(machine, expected);
}
