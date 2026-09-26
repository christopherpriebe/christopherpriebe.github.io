//! The golden fixtures, and the canonical text of their events that the
//! snapshot tests hold.
//!
//! Only built with the `golden` feature, which the core's own tests and the
//! facade's tests turn on: the core compares the text with its committed
//! snapshots natively, and the facade compares the same text produced in
//! WebAssembly, so a change of wording or behaviour is always a reviewed
//! snapshot diff and the two builds are known to agree.
//!
//! Each fixture is a configuration and the loads and stores a student could
//! issue on it by hand, in order. Together they cover the ten access cases,
//! the design's flows A and B, the dirty eviction of frame 3b, untimed and
//! timed, every kind of stall, and the widths that change how numbers are
//! written.

use serde::Serialize;
use thiserror::Error;

use crate::config::{
    AllocatePolicy, CacheConfig, Config, ConfigError, Preset, Replacement, Timing, WordBytes,
    WritePolicy,
};
use crate::engine::{access, Emitter, EventSink, Flow, Request};
use crate::event::{ChangeRef, MicroEvent};
use crate::format;
use crate::geometry::{highest_word, AccessError, Geometry};
use crate::machine::Machine;
use crate::units::{Addr, OpIndex, Register, Word};

/// A run the snapshots pin: a configuration and the accesses issued on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    /// The fixture's name, which is also its snapshot's name.
    pub name: &'static str,
    /// What the fixture shows, for whoever reviews its snapshot.
    pub description: &'static str,
    /// The run's configuration.
    pub config: Config,
    /// The loads and stores, issued one after another from the run's start.
    pub issues: Vec<Issue>,
}

/// A load or store issued by hand, one op of its own. A load writes `r1`, as
/// a hand-issued load does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Issue {
    /// A load of the word at `addr`.
    Load {
        /// The word's address.
        addr: Addr,
    },
    /// A store of `value` to the word at `addr`.
    Store {
        /// The word's address.
        addr: Addr,
        /// The value. It must fit the configuration's word, as an issued
        /// store's value must, or the fixture is refused.
        value: Word,
    },
}

/// Why a fixture's events cannot be produced.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FixtureError {
    /// The fixture's configuration is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// One of its accesses has an address out of range or not aligned to a
    /// word.
    #[error(transparent)]
    Access(#[from] AccessError),
    /// One of its stores has a value wider than the word. The engine would
    /// keep only the value's low bits and say nothing, so the fixture is
    /// refused, as an issued store would be:
    /// `300 does not fit a 1-byte word (0–255).`
    #[error(
        "{} does not fit {} {}-byte word (0\u{2013}{}).",
        format::data(.value.get()),
        format::a_or_an(*.word_bytes),
        format::setting(*.word_bytes),
        format::setting(highest_word(*.word_bytes))
    )]
    Value {
        /// The store's value.
        value: Word,
        /// The word size, in bytes.
        word_bytes: u32,
    },
}

impl Fixture {
    /// The fixture's events, numbered as a session numbers them when each
    /// access is an op of its own: ids from 0, one op index per access, and
    /// one entry per access within run 1.
    ///
    /// # Errors
    ///
    /// [`FixtureError::Config`] if the configuration is invalid. Otherwise,
    /// for the first access an issue would refuse: [`FixtureError::Access`]
    /// if its address does not fit the address width or is not a multiple of
    /// the word size, or [`FixtureError::Value`] if it is a store whose value
    /// does not fit the word.
    pub fn events(&self) -> Result<Vec<MicroEvent>, FixtureError> {
        let mut machine = Machine::start(self.config.clone())?;
        let mut sink = Recorder::default();
        for (op, issue) in (0_u32..).zip(&self.issues) {
            let request = issue.request(&machine.geometry)?;
            let first_id = u64::try_from(sink.events.len()).unwrap_or(u64::MAX);
            let mut out = Emitter::new(&mut sink, OpIndex::new_unchecked(op), first_id);
            // The recorder never stops an access, so only an internal error
            // ends one early. The events end with that error, which the
            // snapshot then shows.
            if access(&mut machine, request, &mut out) == Flow::Stop {
                break;
            }
        }
        Ok(sink.events)
    }
}

impl Issue {
    /// The address the access reads or writes.
    fn addr(self) -> Addr {
        match self {
            Issue::Load { addr } | Issue::Store { addr, .. } => addr,
        }
    }

    /// The engine's request for this access, once its address, and a store's
    /// value, are checked against `geometry`.
    fn request(self, geometry: &Geometry) -> Result<Request, FixtureError> {
        let addr = self.addr();
        if !geometry.decode(addr.get())?.aligned {
            return Err(FixtureError::Access(AccessError::Misaligned {
                addr,
                address_bits: geometry.address_bits(),
                word_bytes: geometry.word_bytes(),
            }));
        }
        Ok(match self {
            Issue::Load { .. } => Request::Load {
                addr,
                into: Register::R1,
            },
            Issue::Store { value, .. } => {
                if geometry.fit_word(value.get()) != value {
                    return Err(FixtureError::Value {
                        value,
                        word_bytes: geometry.word_bytes(),
                    });
                }
                Request::Store {
                    addr,
                    value: value.get(),
                }
            }
        })
    }
}

/// Keeps every event the engine hands it.
#[derive(Default)]
struct Recorder {
    /// The events so far, in order.
    events: Vec<MicroEvent>,
}

impl EventSink for Recorder {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }
}

/// The canonical text of `events`, which the snapshots hold: one line per
/// event with its label, kind, cause, changed cells and counters, cycle and
/// explain line, in that order and separated by ` | `, and `-` for a field
/// that is absent or empty.
///
/// Kinds, causes and counters are written as they serialize (`writeBack`,
/// `writePolicy`, `writebacks`). A cell is its kind and place: `valid 2/0`
/// for set 2, way 0; `word 2/0/1` for its word 1; `memory 0x1A4`; `r1`; `pc`.
pub fn render(events: &[MicroEvent]) -> String {
    let lines: Vec<String> = events
        .iter()
        .map(|event| {
            let cause = event.cause.map_or_else(absent, |cause| name(&cause));
            let changed = if event.changed.is_empty() {
                absent()
            } else {
                let cells: Vec<String> = event.changed.iter().map(cell).collect();
                cells.join(", ")
            };
            let cycle = event.cycle.map_or_else(absent, |cycle| cycle.to_string());
            format!(
                "{} | {} | {cause} | {changed} | {cycle} | {}\n",
                text(&event.label),
                name(&event.kind),
                text(&event.explain)
            )
        })
        .collect();
    lines.concat()
}

/// The text of an absent field.
fn absent() -> String {
    "-".to_owned()
}

/// A text field as written, or `-` when it is empty.
fn text(field: &str) -> &str {
    if field.is_empty() {
        "-"
    } else {
        field
    }
}

/// The name `value` serializes to, so the text uses the names that cross
/// the boundary.
fn name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

/// A changed cell or counter as the canonical text writes it.
fn cell(change: &ChangeRef) -> String {
    match *change {
        ChangeRef::Valid { set, way } => format!("valid {set}/{way}"),
        ChangeRef::Tag { set, way } => format!("tag {set}/{way}"),
        ChangeRef::Dirty { set, way } => format!("dirty {set}/{way}"),
        ChangeRef::Rank { set, way } => format!("rank {set}/{way}"),
        ChangeRef::Word { set, way, word } => format!("word {set}/{way}/{word}"),
        ChangeRef::Memory { addr } => format!("memory {addr}"),
        ChangeRef::Register { register } => register.to_string(),
        ChangeRef::Pc => "pc".to_owned(),
        ChangeRef::Counter { counter } => name(&counter),
    }
}

/// Every golden fixture, in a fixed order.
pub fn fixtures() -> Vec<Fixture> {
    let mut fixtures = flows();
    fixtures.extend(stores());
    fixtures.extend(widths());
    fixtures.extend(timed());
    fixtures
}

/// The design's flows, frame 3b's accesses and a direct-mapped conflict:
/// loads under each replacement policy, and a dirty eviction.
fn flows() -> Vec<Fixture> {
    let flow_b = [
        load(0x000),
        load(0x040),
        load(0x000),
        load(0x080),
        load(0x0C0),
    ];
    vec![
        Fixture {
            name: "flow-a",
            description: "The design's flow A on direct-mapped-8: a load misses in an empty \
                          one-way set and brings in the whole line, so a load of its neighbour \
                          hits (cases 2 and 1).",
            config: Preset::DirectMapped8.config(),
            issues: vec![load(0x0A4), load(0x0A8)],
        },
        Fixture {
            name: "flow-b-lru",
            description: "The design's flow B on two-way-lru: 0x000 and 0x040 fill set 0, a hit \
                          makes way 0 the most recent, then 0x080 evicts way 1 (victim row \
                          4.3) and 0x0C0 evicts way 0 (cases 2, 1 and 3).",
            config: Preset::TwoWayLru.config(),
            issues: flow_b.to_vec(),
        },
        Fixture {
            name: "flow-b-fifo",
            description: "Flow B under FIFO: the hit moves no rank, so 0x080 evicts way 0, \
                          filled earliest, and 0x0C0 evicts way 1.",
            config: two_way(Replacement::Fifo, 12_345),
            issues: flow_b.to_vec(),
        },
        Fixture {
            name: "flow-b-random-seed-1",
            description: "Flow B under random replacement with seed 1: the victims come from \
                          the seeded generator, and no rank ever moves.",
            config: two_way(Replacement::Random, 1),
            issues: flow_b.to_vec(),
        },
        Fixture {
            name: "frame-3b-accesses",
            description: "Frame 3b's accesses issued by hand on two-way-lru, untimed, in the \
                          order of its program: a store dirties 0x0B0 in set 3, a load fills \
                          way 0 of set 2 and a store dirties 0x1A0 in way 1, a hit makes way 0 \
                          the most recent, and the load of 0x2A4 writes back and evicts the \
                          dirty 0x1A0 from way 1 (cases 7, 2, 7, 1 and 4). Every load goes to \
                          r1, where the program loads r1, r2 and r3.",
            config: Preset::TwoWayLru.config(),
            issues: vec![
                store(0x0B0, 5),
                load(0x0A4),
                store(0x1A4, 7),
                load(0x0A8),
                load(0x2A4),
            ],
        },
        Fixture {
            name: "direct-mapped-conflict",
            description: "Direct-mapped-8: 0x000 and 0x080 share set 0, its only way, so each \
                          load evicts the other (cases 2 and 3).",
            config: Preset::DirectMapped8.config(),
            issues: vec![load(0x000), load(0x080), load(0x000)],
        },
    ]
}

/// Stores under each write policy and each allocate policy.
fn stores() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "write-back-stores",
            description: "Store hits under write-back on direct-mapped-8: a clean line becomes \
                          dirty and memory stale; a dirty line is written again, once with a \
                          value memory already holds; then a store miss writes the dirty line \
                          back before it evicts it (cases 2, 5 and 7).",
            config: Preset::DirectMapped8.config(),
            issues: vec![
                load(0x0A4),
                store(0x0A4, 7),
                store(0x0A8, 9),
                store(0x0AC, 79),
                load(0x0B0),
                store(0x0B0, 92),
                store(0x1A4, 3),
            ],
        },
        Fixture {
            name: "write-through-stores",
            description: "Write-through on direct-mapped-8: a store miss fills then writes the \
                          cache and memory; store hits write both, the second with the value \
                          memory already holds; a store miss evicts a clean line (cases 8 and \
                          6).",
            config: direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
            issues: vec![
                store(0x0A4, 7),
                store(0x0A8, 9),
                store(0x0A8, 9),
                store(0x1A4, 3),
            ],
        },
        Fixture {
            name: "write-through-two-way",
            description: "Write-through on two ways with LRU: after a store miss and a store \
                          hit, the rank move comes last, after the write-through (cases 2, 8 \
                          and 6).",
            config: Config {
                cache: CacheConfig {
                    write_policy: WritePolicy::WriteThrough,
                    ..Preset::TwoWayLru.config().cache
                },
                ..Preset::TwoWayLru.config()
            },
            issues: vec![load(0x0A4), store(0x1A4, 3), store(0x0A4, 5)],
        },
        Fixture {
            name: "write-no-allocate-write-back",
            description: "Write-no-allocate under write-back: a store miss goes around the \
                          cache and counts as a write-around; the second writes the value \
                          memory already holds (case 9).",
            config: direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
            issues: vec![store(0x0A4, 7), store(0x0A8, 66)],
        },
        Fixture {
            name: "write-no-allocate-write-through",
            description: "Write-no-allocate under write-through: store misses go around the \
                          cache and count as write-arounds, not write-throughs (case 10).",
            config: direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
            issues: vec![store(0x0A4, 7), store(0x1A4, 3)],
        },
        Fixture {
            name: "no-allocate-store-then-load",
            description: "A store miss under write-no-allocate leaves the cache empty, so a load \
                          of the same line misses too and reads the stored value from memory \
                          (cases 9 and 2).",
            config: direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
            issues: vec![store(0x0A4, 7), load(0x0A4)],
        },
    ]
}

/// The shapes that change how numbers are written: a one-line cache, every
/// address width and every word size.
fn widths() -> Vec<Fixture> {
    let mut fixtures = vec![Fixture {
        name: "one-line-cache",
        description: "A 16 B cache of one 16 B line: fully associative with one way, so the \
                          decode names no set and the victim line does not say direct-mapped \
                          (cases 2, 1 and 3).",
        config: Config {
            cache: CacheConfig {
                size_bytes: 16,
                ..Preset::DirectMapped8.config().cache
            },
            ..Preset::DirectMapped8.config()
        },
        issues: vec![load(0x0A4), load(0x0A8), load(0x1A4)],
    }];
    for (name, description, address_bits) in [
        (
            "address-bits-8",
            "The same accesses with 8-bit addresses: two hex digits and a one-bit tag.",
            8,
        ),
        (
            "address-bits-12",
            "The same accesses with 12-bit addresses: three hex digits and a five-bit tag.",
            12,
        ),
        (
            "address-bits-16",
            "The same accesses with 16-bit addresses: four hex digits and a nine-bit tag, \
             written with three digits.",
            16,
        ),
        (
            "address-bits-32",
            "The same accesses with 32-bit addresses: eight hex digits and a 25-bit tag, \
             written with seven digits.",
            32,
        ),
    ] {
        fixtures.push(Fixture {
            name,
            description,
            config: Config {
                address_bits,
                ..Preset::DirectMapped8.config()
            },
            issues: vec![load(0x0A8), load(0x028), store(0x0A8, 7)],
        });
    }
    for (name, description, word_bytes, largest) in [
        (
            "word-bytes-1",
            "1-byte words: sixteen to a line; the largest word, 255, stored and loaded.",
            WordBytes::One,
            u64::from(u8::MAX),
        ),
        (
            "word-bytes-2",
            "2-byte words: eight to a line; the largest word, 65,535, stored and loaded.",
            WordBytes::Two,
            u64::from(u16::MAX),
        ),
        (
            "word-bytes-4",
            "4-byte words: four to a line; the largest word, 4,294,967,295, stored and loaded.",
            WordBytes::Four,
            u64::from(u32::MAX),
        ),
        (
            "word-bytes-8",
            "8-byte words: two to a line; the largest word, 2^64 \u{2212} 1, stored and loaded.",
            WordBytes::Eight,
            u64::MAX,
        ),
    ] {
        fixtures.push(Fixture {
            name,
            description,
            config: Config {
                word_bytes,
                ..Preset::DirectMapped8.config()
            },
            issues: vec![store(0x0A8, largest), load(0x0A8)],
        });
    }
    fixtures
}

/// Timed mode: the cycle stamps of each case, and every kind of stall.
fn timed() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "frame-3b-accesses-timed",
            description: "Frame 3b's accesses as in frame-3b-accesses, timed 1/20: each miss \
                          stalls once its victim is chosen, and the dirty eviction stalls for 40 \
                          cycles, 20 for the write-back and 20 for the fill. Cycles are charged \
                          where the time is spent, so the write-back row reads 85 and the fill \
                          105, where the design charges the whole miss at the Miss row (cases 7, \
                          2, 7, 1 and 4).",
            config: timed_config(Preset::TwoWayLru.config(), 1, 20),
            issues: vec![
                store(0x0B0, 5),
                load(0x0A4),
                store(0x1A4, 7),
                load(0x0A8),
                load(0x2A4),
            ],
        },
        Fixture {
            name: "timed-write-through",
            description: "Write-through on direct-mapped-8, timed 1/20: a store miss stalls for \
                          the fill, a store hit costs the hit time alone although it writes \
                          memory too, and a load that evicts a clean line stalls for the fill \
                          only (cases 8, 6 and 3).",
            config: timed_config(
                direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
                1,
                20,
            ),
            issues: vec![store(0x0A4, 7), store(0x0A8, 9), load(0x1A4)],
        },
        Fixture {
            name: "timed-write-no-allocate",
            description: "Write-no-allocate under write-back, timed 1/20: a store miss stalls \
                          right after the compare while the store goes to memory, the load of \
                          its line then misses and stalls for the fill, and a store hit costs \
                          the hit time (cases 9, 2 and 5).",
            config: timed_config(
                direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
                1,
                20,
            ),
            issues: vec![store(0x0A4, 7), load(0x0A4), store(0x0A8, 9)],
        },
        Fixture {
            name: "timed-one-cycle-misses",
            description: "Direct-mapped-8 timed with 3-cycle hits and 1-cycle misses: the stall \
                          of a single cycle is singular, a dirty eviction stalls for 2, and a \
                          hit costs more than a transfer (cases 7, 4 and 1).",
            config: timed_config(Preset::DirectMapped8.config(), 3, 1),
            issues: vec![store(0x0A4, 7), load(0x1A4), load(0x1A8)],
        },
    ]
}

/// A load of the word at `addr`.
fn load(addr: u32) -> Issue {
    Issue::Load {
        addr: Addr::new_unchecked(addr),
    }
}

/// A store of `value` to the word at `addr`.
fn store(addr: u32, value: u64) -> Issue {
    Issue::Store {
        addr: Addr::new_unchecked(addr),
        value: Word::new_unchecked(value),
    }
}

/// `direct-mapped-8` under the given write and allocate policies.
fn direct_mapped(write_policy: WritePolicy, allocate_policy: AllocatePolicy) -> Config {
    let config = Preset::DirectMapped8.config();
    Config {
        cache: CacheConfig {
            write_policy,
            allocate_policy,
            ..config.cache
        },
        ..config
    }
}

/// `config` in timed mode, with `hit`-cycle hits and `miss`-cycle misses.
fn timed_config(config: Config, hit: u32, miss: u32) -> Config {
    Config {
        timing: Some(Timing {
            hit_cycles: hit,
            miss_cycles: miss,
        }),
        ..config
    }
}

/// `two-way-lru` under another replacement policy and seed.
fn two_way(replacement: Replacement, seed: u64) -> Config {
    let config = Preset::TwoWayLru.config();
    Config {
        cache: CacheConfig {
            replacement,
            ..config.cache
        },
        seed,
        ..config
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::event::{Cause, Counter, EventKind};
    use crate::units::{Cycle, SetIndex, WayIndex, WordIndex};

    fn event(label: &str, kind: EventKind) -> MicroEvent {
        MicroEvent {
            id: 0,
            op_index: OpIndex::new_unchecked(0),
            run: 1,
            entry: Some(1),
            seq: 1,
            label: label.to_owned(),
            cycle: None,
            kind,
            addr: None,
            set: None,
            way: None,
            changed: Vec::new(),
            explain: String::new(),
            cause: None,
        }
    }

    #[test]
    fn render_writes_one_line_per_event_with_every_field() {
        let set = SetIndex::new_unchecked(2);
        let way = WayIndex::new_unchecked(1);
        let events = [
            MicroEvent {
                cycle: Some(Cycle::new_unchecked(121)),
                changed: vec![
                    ChangeRef::Memory {
                        addr: Addr::new_unchecked(0x1A4),
                    },
                    ChangeRef::Counter {
                        counter: Counter::Writebacks,
                    },
                ],
                explain: "Way 1 is dirty: write line 0x1A0 back to memory first.".to_owned(),
                cause: Some(Cause::WritePolicy),
                ..event("5.4", EventKind::WriteBack)
            },
            MicroEvent {
                changed: vec![
                    ChangeRef::Valid { set, way },
                    ChangeRef::Tag { set, way },
                    ChangeRef::Dirty { set, way },
                    ChangeRef::Rank { set, way },
                    ChangeRef::Word {
                        set,
                        way,
                        word: WordIndex::new_unchecked(3),
                    },
                    ChangeRef::Register {
                        register: Register::R3,
                    },
                    ChangeRef::Pc,
                ],
                explain: "Read word 1 of way 1 (68) into r3.".to_owned(),
                ..event("5.7", EventKind::Complete)
            },
            event("", EventKind::Setup),
        ];
        assert_eq!(
            render(&events),
            "5.4 | writeBack | writePolicy | memory 0x1A4, writebacks | 121 | Way 1 is dirty: \
             write line 0x1A0 back to memory first.\n\
             5.7 | complete | - | valid 2/1, tag 2/1, dirty 2/1, rank 2/1, word 2/1/3, r3, pc | \
             - | Read word 1 of way 1 (68) into r3.\n\
             - | setup | - | - | - | -\n"
        );
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn fixture_names_are_unique_and_every_fixture_runs() {
        let fixtures = fixtures();
        for (index, fixture) in fixtures.iter().enumerate() {
            assert!(
                fixtures
                    .iter()
                    .skip(index + 1)
                    .all(|other| other.name != fixture.name),
                "{} is named twice",
                fixture.name
            );
            let events = fixture.events().unwrap();
            assert!(!events.is_empty(), "{}", fixture.name);
            assert!(
                events.iter().all(|event| event.kind != EventKind::Error),
                "{}",
                fixture.name
            );
        }
    }

    #[test]
    fn events_are_numbered_one_op_and_one_entry_per_access() {
        let fixture = fixtures()
            .into_iter()
            .find(|fixture| fixture.name == "flow-a")
            .unwrap();
        let events = fixture.events().unwrap();
        let numbers: Vec<(u64, u32, Option<u32>, u32, &str)> = events
            .iter()
            .map(|event| {
                (
                    event.id,
                    event.op_index.get(),
                    event.entry,
                    event.run,
                    event.label.as_str(),
                )
            })
            .collect();
        assert_eq!(
            numbers,
            [
                (0, 0, Some(1), 1, "1.1"),
                (1, 0, Some(1), 1, "1.2"),
                (2, 0, Some(1), 1, "1.3"),
                (3, 0, Some(1), 1, "1.4"),
                (4, 0, Some(1), 1, "1.5"),
                (5, 1, Some(2), 1, "2.1"),
                (6, 1, Some(2), 1, "2.2"),
                (7, 1, Some(2), 1, "2.3"),
            ]
        );
    }

    #[test]
    fn a_fixture_refuses_what_an_issue_would_refuse() {
        let config = Preset::DirectMapped8.config();
        let fixture = |issues| Fixture {
            name: "refused",
            description: "",
            config: config.clone(),
            issues,
        };
        let word_bytes = config.word_bytes.bytes();
        let misaligned = 0x0A4 + word_bytes / 2;
        assert_eq!(
            fixture(vec![load(misaligned)]).events(),
            Err(FixtureError::Access(AccessError::Misaligned {
                addr: Addr::new_unchecked(misaligned),
                address_bits: config.address_bits,
                word_bytes,
            }))
        );
        let beyond = 1 << config.address_bits;
        assert_eq!(
            fixture(vec![load(0x0A4), store(beyond, 1)]).events(),
            Err(FixtureError::Access(AccessError::OutOfRange {
                addr: beyond,
                address_bits: config.address_bits,
            }))
        );
        let invalid = Fixture {
            config: Config {
                address_bits: 0,
                ..config.clone()
            },
            ..fixture(Vec::new())
        };
        assert!(matches!(invalid.events(), Err(FixtureError::Config(_))));
    }

    #[test]
    fn a_fixture_refuses_a_store_value_the_word_cannot_hold() {
        let fixture = |word_bytes, value| Fixture {
            name: "refused",
            description: "",
            config: Config {
                word_bytes,
                ..Preset::DirectMapped8.config()
            },
            issues: vec![load(0x0A8), store(0x0A8, value)],
        };
        let refused = fixture(WordBytes::One, 300).events();
        assert_eq!(
            refused,
            Err(FixtureError::Value {
                value: Word::new_unchecked(300),
                word_bytes: 1,
            })
        );
        assert_eq!(
            refused.map_err(|error| error.to_string()),
            Err("300 does not fit a 1-byte word (0\u{2013}255).".to_owned())
        );
        assert_eq!(
            FixtureError::Value {
                value: Word::new_unchecked(u64::from(u32::MAX) + 1),
                word_bytes: 4,
            }
            .to_string(),
            "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295)."
        );
        // The largest word of each size still fits.
        for (word_bytes, largest) in [(WordBytes::One, 255), (WordBytes::Eight, u64::MAX)] {
            assert!(fixture(word_bytes, largest).events().is_ok());
        }
    }
}
