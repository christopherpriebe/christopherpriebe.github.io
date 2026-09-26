//! Golden snapshots: the events of every golden fixture, rendered as the
//! canonical text and compared with the committed snapshot, so that a change
//! of wording, order, changed cells or counters is reviewed deliberately;
//! and a check that the fixtures cover the ten access cases.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::golden::{self, Fixture};
use cache_explorer_core::{AllocatePolicy, EventKind, MicroEvent, WritePolicy};
use insta::{Comparator, DefaultComparator, Snapshot};

/// Renders the events of the fixture named `name` and compares them with its
/// snapshot, whose header carries the fixture's description.
fn check(name: &str) -> Result<(), String> {
    let fixture = golden::fixtures()
        .into_iter()
        .find(|fixture| fixture.name == name)
        .ok_or_else(|| format!("no fixture is named {name}"))?;
    let events = fixture.events().map_err(|error| error.to_string())?;
    insta::with_settings!({
        description => fixture.description,
        omit_expression => true,
        comparator => Box::new(WithHeader),
    }, {
        insta::assert_snapshot!(name, golden::render(&events));
    });
    Ok(())
}

/// Compares a snapshot's header as well as its events. insta compares only
/// the events by default, so an edited description would leave the committed
/// header stale while every test passed; with the header compared, the
/// snapshot is reviewed again.
#[derive(Clone)]
struct WithHeader;

impl Comparator for WithHeader {
    fn matches(&self, reference: &Snapshot, test: &Snapshot) -> bool {
        DefaultComparator.matches_fully(reference, test)
    }

    fn dyn_clone(&self) -> Box<dyn Comparator> {
        Box::new(self.clone())
    }
}

/// One test per fixture, and the list of the fixtures they cover.
macro_rules! snapshots {
    ($($test:ident => $name:literal,)*) => {
        /// The fixtures the tests snapshot, in the order `golden::fixtures`
        /// lists them.
        const SNAPSHOTS: &[&str] = &[$($name),*];

        $(
            #[test]
            fn $test() -> Result<(), String> {
                check($name)
            }
        )*
    };
}

snapshots! {
    flow_a => "flow-a",
    flow_b_lru => "flow-b-lru",
    flow_b_fifo => "flow-b-fifo",
    flow_b_random_seed_1 => "flow-b-random-seed-1",
    frame_3b_accesses => "frame-3b-accesses",
    direct_mapped_conflict => "direct-mapped-conflict",
    write_back_stores => "write-back-stores",
    write_through_stores => "write-through-stores",
    write_through_two_way => "write-through-two-way",
    write_no_allocate_write_back => "write-no-allocate-write-back",
    write_no_allocate_write_through => "write-no-allocate-write-through",
    no_allocate_store_then_load => "no-allocate-store-then-load",
    one_line_cache => "one-line-cache",
    address_bits_8 => "address-bits-8",
    address_bits_12 => "address-bits-12",
    address_bits_16 => "address-bits-16",
    address_bits_32 => "address-bits-32",
    word_bytes_1 => "word-bytes-1",
    word_bytes_2 => "word-bytes-2",
    word_bytes_4 => "word-bytes-4",
    word_bytes_8 => "word-bytes-8",
    frame_3b_accesses_timed => "frame-3b-accesses-timed",
    timed_write_through => "timed-write-through",
    timed_write_no_allocate => "timed-write-no-allocate",
    timed_one_cycle_misses => "timed-one-cycle-misses",
    frame_3b_program => "frame-3b-program",
    frame_3b_program_timed => "frame-3b-program-timed",
    sample_trace => "sample-trace",
    count_down_loop => "count-down-loop",
    register_address_fault => "register-address-fault",
    budget_exhaustion => "budget-exhaustion",
    store_truncation => "store-truncation",
    issue_beside_program => "issue-beside-program",
}

#[test]
fn every_fixture_has_a_snapshot() {
    let names: Vec<&str> = golden::fixtures()
        .iter()
        .map(|fixture| fixture.name)
        .collect();
    assert_eq!(names, SNAPSHOTS);
}

/// Which of the ten access cases an access's events are, under `fixture`'s
/// write policy, or `None` if they are none of them.
fn case(fixture: &Fixture, events: &[MicroEvent]) -> Option<u8> {
    let has = |kind: EventKind| events.iter().any(|event| event.kind == kind);
    let write_through = fixture.config.cache.write_policy == WritePolicy::WriteThrough;
    let case = match (has(EventKind::Hit), has(EventKind::Complete)) {
        (true, true) => 1,
        (false, true) if has(EventKind::WriteBack) => 4,
        (false, true) if has(EventKind::Evict) => 3,
        (false, true) => 2,
        (true, false) if write_through => 6,
        (true, false) => 5,
        (false, false) if has(EventKind::MemWrite) && write_through => 10,
        (false, false) if has(EventKind::MemWrite) => 9,
        (false, false) if write_through => 8,
        (false, false) => 7,
    };
    // A store's case also fixes its allocate policy and its result.
    let allocates = fixture.config.cache.allocate_policy == AllocatePolicy::WriteAllocate;
    let consistent = match case {
        5..=8 => has(EventKind::Write) && (case < 7 || allocates),
        9 | 10 => !allocates,
        _ => true,
    };
    consistent.then_some(case)
}

#[test]
fn the_fixtures_cover_the_ten_access_cases() {
    let mut seen = [false; 10];
    for fixture in golden::fixtures() {
        let events = fixture.events().unwrap();
        // Every access starts with its Decode; a program load, an add, a
        // bnez and an error are entries of other kinds.
        let accesses = events
            .chunk_by(|a, b| a.entry == b.entry)
            .filter(|entry| entry.first().map(|event| event.kind) == Some(EventKind::Decode));
        for access in accesses {
            let case = case(&fixture, access);
            assert!(case.is_some(), "{}: {:?}", fixture.name, access);
            if let Some(case) = case {
                seen[usize::from(case) - 1] = true;
            }
        }
    }
    assert_eq!(seen, [true; 10]);
}
