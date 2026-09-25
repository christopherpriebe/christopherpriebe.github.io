//! Micro-events: the ordered steps each op yields, and what each one changed.
//!
//! Every op produces one or more micro-events, and the session's log is the
//! list of them. An event names its kind, where in the cache it happened, the
//! configuration group responsible for it, and every cell or counter whose
//! value it changed, so the page can highlight exactly those.

use serde::{Deserialize, Serialize};

use crate::units::{Addr, Cycle, OpIndex, Register, SetIndex, WayIndex, WordIndex};

/// One step of an op, as the log shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct MicroEvent {
    /// The event's place in the whole log, from 0: the event after which the
    /// cursor rests at position k has id k − 1. Ids are reused after the log
    /// is truncated, so an id is never a key that outlives a truncation.
    pub id: u64,
    /// The op that produced the event.
    pub op_index: OpIndex,
    /// The run the event belongs to, from 1.
    pub run: u32,
    /// The entry within the run, from 1; absent for a setup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<u32>,
    /// The event's number within its entry, from 1.
    pub seq: u32,
    /// `{entry}.{seq}`, such as `5.3`; empty for a setup.
    pub label: String,
    /// The run's cycle count once the event's effect is complete; present in
    /// timed mode only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<Cycle>,
    /// What happened.
    pub kind: EventKind,
    /// The address the event concerns: the accessed address, or the line
    /// moving to or from memory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addr: Option<Addr>,
    /// The set the event concerns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<SetIndex>,
    /// The way the event concerns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub way: Option<WayIndex>,
    /// Every cell and counter whose value this event changed, sorted and
    /// without duplicates.
    pub changed: Vec<ChangeRef>,
    /// The log line.
    pub explain: String,
    /// The configuration group responsible for the event, which the log's
    /// "why:" link flashes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
}

/// What an event did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    /// The address was split into tag, index and offset. Always an access's
    /// first event.
    Decode,
    /// The tag compare found the line: a hit.
    Hit,
    /// The tag compare did not find the line: a miss.
    Miss,
    /// A miss chose the way its line will go into.
    Victim,
    /// A dirty victim's line was written back to memory.
    WriteBack,
    /// A write-through store's word was also written to memory.
    WriteThrough,
    /// A line was brought in from memory.
    Fill,
    /// A store wrote its word into the cache.
    Write,
    /// The replacement order of a set changed.
    Rank,
    /// A store that missed under write-no-allocate wrote memory only.
    MemWrite,
    /// The CPU waited for memory, in timed mode.
    Stall,
    /// A load's word reached its register, or an instruction without a memory
    /// access ran.
    Complete,
    /// The student edited a way or a memory word.
    Edit,
    /// A fault, the end of the instruction budget or of the session's room,
    /// or an internal error ended the op.
    Error,
    /// The victim's line left the cache. Not one of the spec's kinds: it gives
    /// the moment between the old line leaving and the new one arriving its
    /// own step.
    Evict,
    /// A new run started, or a program was loaded. Not one of the spec's
    /// kinds: it gives those ops a place in the log.
    Setup,
}

/// The configuration group responsible for an event, which the log's "why:"
/// link flashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// The line size: how an address splits, and how much a fill brings in.
    LineSize,
    /// The associativity: where a line may go, and so hits, misses and
    /// victims with no choice to make.
    Associativity,
    /// The replacement policy: which way a full set gives up, and the order
    /// it keeps.
    Replacement,
    /// The write policy: what a store does to memory.
    WritePolicy,
    /// The allocate policy: whether a store that misses brings its line in.
    AllocatePolicy,
    /// Timed mode's costs.
    Timing,
}

/// A cell or counter an event changed: one thing the page can highlight.
///
/// Sorted by variant, then by field, so a list of them has one order
/// whatever produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "cell",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChangeRef {
    /// A way's valid bit.
    Valid {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// A way's tag.
    Tag {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// A way's dirty bit.
    Dirty {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// A way's rank in its set's replacement order.
    Rank {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// One word of a way's data.
    Word {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
        /// The word within the line.
        word: WordIndex,
    },
    /// One word of memory.
    Memory {
        /// The word's address, a multiple of the word size.
        addr: Addr,
    },
    /// A register.
    Register {
        /// The register.
        register: Register,
    },
    /// The program counter.
    Pc,
    /// A counter of the Stats region.
    Counter {
        /// The counter.
        counter: Counter,
    },
}

/// One of the run's counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Counter {
    /// Loads and stores looked up in the cache.
    Accesses,
    /// Loads looked up.
    Loads,
    /// Stores looked up.
    Stores,
    /// Accesses that found their line.
    Hits,
    /// Accesses that did not.
    Misses,
    /// Lines that left the cache to make room.
    Evictions,
    /// Dirty lines written back to memory.
    Writebacks,
    /// Stores written through to memory under write-through.
    Writethroughs,
    /// Stores that missed under write-no-allocate and went straight to
    /// memory.
    Writearounds,
    /// Lines brought in from memory.
    Fills,
    /// Cycles, in timed mode.
    Cycles,
    /// Misses on a line never filled in this run.
    Compulsory,
    /// Misses a fully associative cache of the same size would also have.
    Capacity,
    /// The remaining misses, caused by where lines may go.
    Conflict,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use serde_json::json;

    fn set(index: u32) -> SetIndex {
        SetIndex::new_unchecked(index)
    }

    fn way(index: u32) -> WayIndex {
        WayIndex::new_unchecked(index)
    }

    #[test]
    fn a_micro_event_serializes_in_camel_case_without_absent_fields() {
        let event = MicroEvent {
            id: 4,
            op_index: OpIndex::new_unchecked(2),
            run: 1,
            entry: Some(5),
            seq: 3,
            label: "5.3".to_owned(),
            cycle: None,
            kind: EventKind::Victim,
            addr: None,
            set: Some(set(2)),
            way: Some(way(1)),
            changed: Vec::new(),
            explain: String::new(),
            cause: Some(Cause::Replacement),
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "id": 4, "opIndex": 2, "run": 1, "entry": 5, "seq": 3, "label": "5.3",
                "kind": "victim", "set": 2, "way": 1, "changed": [], "explain": "",
                "cause": "replacement"
            })
        );
        let event = MicroEvent {
            entry: None,
            label: String::new(),
            cycle: Some(Cycle::new_unchecked(141)),
            kind: EventKind::Fill,
            addr: Some(Addr::new_unchecked(0x2A0)),
            set: None,
            way: None,
            cause: None,
            ..event
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "id": 4, "opIndex": 2, "run": 1, "seq": 3, "label": "", "cycle": 141,
                "kind": "fill", "addr": 672, "changed": [], "explain": ""
            })
        );
    }

    #[test]
    fn event_kinds_serialize_by_camel_case_name() {
        let kinds = [
            (EventKind::Decode, "decode"),
            (EventKind::Hit, "hit"),
            (EventKind::Miss, "miss"),
            (EventKind::Victim, "victim"),
            (EventKind::WriteBack, "writeBack"),
            (EventKind::WriteThrough, "writeThrough"),
            (EventKind::Fill, "fill"),
            (EventKind::Write, "write"),
            (EventKind::Rank, "rank"),
            (EventKind::MemWrite, "memWrite"),
            (EventKind::Stall, "stall"),
            (EventKind::Complete, "complete"),
            (EventKind::Edit, "edit"),
            (EventKind::Error, "error"),
            (EventKind::Evict, "evict"),
            (EventKind::Setup, "setup"),
        ];
        for (kind, name) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<EventKind>(json!(name)).unwrap(),
                kind
            );
        }
        assert!(serde_json::from_value::<EventKind>(json!("WriteBack")).is_err());
    }

    #[test]
    fn causes_serialize_by_camel_case_name() {
        let causes = [
            (Cause::LineSize, "lineSize"),
            (Cause::Associativity, "associativity"),
            (Cause::Replacement, "replacement"),
            (Cause::WritePolicy, "writePolicy"),
            (Cause::AllocatePolicy, "allocatePolicy"),
            (Cause::Timing, "timing"),
        ];
        for (cause, name) in causes {
            assert_eq!(serde_json::to_value(cause).unwrap(), json!(name));
        }
    }

    #[test]
    fn counters_serialize_by_their_lower_case_names() {
        let counters = [
            (Counter::Accesses, "accesses"),
            (Counter::Loads, "loads"),
            (Counter::Stores, "stores"),
            (Counter::Hits, "hits"),
            (Counter::Misses, "misses"),
            (Counter::Evictions, "evictions"),
            (Counter::Writebacks, "writebacks"),
            (Counter::Writethroughs, "writethroughs"),
            (Counter::Writearounds, "writearounds"),
            (Counter::Fills, "fills"),
            (Counter::Cycles, "cycles"),
            (Counter::Compulsory, "compulsory"),
            (Counter::Capacity, "capacity"),
            (Counter::Conflict, "conflict"),
        ];
        for (counter, name) in counters {
            assert_eq!(serde_json::to_value(counter).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<Counter>(json!(name)).unwrap(),
                counter
            );
        }
    }

    #[test]
    fn change_refs_are_tagged_by_cell() {
        let refs = [
            (
                ChangeRef::Valid {
                    set: set(2),
                    way: way(0),
                },
                json!({"cell": "valid", "set": 2, "way": 0}),
            ),
            (
                ChangeRef::Tag {
                    set: set(2),
                    way: way(0),
                },
                json!({"cell": "tag", "set": 2, "way": 0}),
            ),
            (
                ChangeRef::Dirty {
                    set: set(2),
                    way: way(1),
                },
                json!({"cell": "dirty", "set": 2, "way": 1}),
            ),
            (
                ChangeRef::Rank {
                    set: set(3),
                    way: way(1),
                },
                json!({"cell": "rank", "set": 3, "way": 1}),
            ),
            (
                ChangeRef::Word {
                    set: set(2),
                    way: way(0),
                    word: WordIndex::new_unchecked(1),
                },
                json!({"cell": "word", "set": 2, "way": 0, "word": 1}),
            ),
            (
                ChangeRef::Memory {
                    addr: Addr::new_unchecked(0x1A4),
                },
                json!({"cell": "memory", "addr": 420}),
            ),
            (
                ChangeRef::Register {
                    register: Register::R1,
                },
                json!({"cell": "register", "register": "r1"}),
            ),
            (ChangeRef::Pc, json!({"cell": "pc"})),
            (
                ChangeRef::Counter {
                    counter: Counter::Writearounds,
                },
                json!({"cell": "counter", "counter": "writearounds"}),
            ),
        ];
        for (change, shape) in refs {
            assert_eq!(serde_json::to_value(change).unwrap(), shape);
        }
    }

    #[test]
    fn change_refs_sort_by_cell_then_place() {
        let mut refs = vec![
            ChangeRef::Counter {
                counter: Counter::Fills,
            },
            ChangeRef::Word {
                set: set(1),
                way: way(0),
                word: WordIndex::new_unchecked(0),
            },
            ChangeRef::Tag {
                set: set(1),
                way: way(0),
            },
            ChangeRef::Valid {
                set: set(2),
                way: way(0),
            },
            ChangeRef::Valid {
                set: set(1),
                way: way(1),
            },
            ChangeRef::Counter {
                counter: Counter::Accesses,
            },
        ];
        refs.sort_unstable();
        assert_eq!(
            refs,
            [
                ChangeRef::Valid {
                    set: set(1),
                    way: way(1),
                },
                ChangeRef::Valid {
                    set: set(2),
                    way: way(0),
                },
                ChangeRef::Tag {
                    set: set(1),
                    way: way(0),
                },
                ChangeRef::Word {
                    set: set(1),
                    way: way(0),
                    word: WordIndex::new_unchecked(0),
                },
                ChangeRef::Counter {
                    counter: Counter::Accesses,
                },
                ChangeRef::Counter {
                    counter: Counter::Fills,
                },
            ]
        );
    }
}
