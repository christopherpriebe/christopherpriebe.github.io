//! The pins of sharing on the golden fixtures: each fixture's run as a share
//! string and as a saved file, and at every cursor of it, the share string
//! and the state the page draws, each as a fingerprint. A change to the
//! format, to the encoding or to what a cursor shows is a reviewed snapshot
//! diff; and a share string opens to the state it was written from.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use std::fmt::Write as _;

use cache_explorer_core::golden::{self, Fixture};
use cache_explorer_core::{Op, Session};

use common::golden_settings;

#[cfg(test)]
mod common;

/// The 64-bit FNV-1a hash of `text`, as 16 hexadecimal digits. The hash is
/// fixed by its definition, so a fingerprint written today is the one any
/// build writes.
fn fingerprint(text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let hash = text.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    });
    format!("{hash:016x}")
}

/// The fixture named `name`.
fn fixture(name: &str) -> Result<Fixture, String> {
    golden::fixtures()
        .into_iter()
        .find(|fixture| fixture.name == name)
        .ok_or_else(|| format!("no fixture is named {name}"))
}

/// The session `fixture`'s issues make, each applied as an op of its own.
fn run(fixture: &Fixture) -> Result<Session, String> {
    let mut session = Session::with_config(fixture.config).map_err(|error| error.to_string())?;
    for issue in &fixture.issues {
        session
            .apply(Op::from(issue))
            .map_err(|error| error.to_string())?;
    }
    Ok(session)
}

/// The fingerprint of what the page draws at `session`'s cursor.
fn state_fingerprint(session: &Session) -> Result<String, String> {
    let state = serde_json::to_string(&session.state()).map_err(|error| error.to_string())?;
    Ok(fingerprint(&state))
}

/// `session`'s share string.
fn share(session: &Session) -> Result<String, String> {
    session.to_share_string().map_err(|error| error.to_string())
}

/// What the pins hold for `fixture`: the share string and the saved file of
/// its run with the cursor at the end, then one line for each cursor, from
/// position 0 to the end of the log, with the cursor's place in the
/// document and the fingerprints of its share string and its state.
fn pins(fixture: &Fixture) -> Result<String, String> {
    let mut session = run(fixture)?;
    let mut text = format!(
        "share: {}\nfile: {}\ncursors:\n",
        share(&session)?,
        session.to_json()
    );
    for done in 0..=session.op_count() {
        session.scrub_to(done);
        let at =
            serde_json::to_string(&session.share_doc().at).map_err(|error| error.to_string())?;
        writeln!(
            text,
            "{done} | {at} | {} | {}",
            fingerprint(&share(&session)?),
            state_fingerprint(&session)?
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(text)
}

/// Compares the pins of the fixture named `name` with its snapshot, whose
/// header carries the fixture's description.
fn check(name: &str) -> Result<(), String> {
    let fixture = fixture(name)?;
    let text = pins(&fixture)?;
    golden_settings(fixture.description).bind(|| {
        insta::assert_snapshot!(name, text);
    });
    Ok(())
}

/// One test per fixture, and the list of the fixtures they cover.
macro_rules! snapshots {
    ($($test:ident => $name:literal,)*) => {
        /// The fixtures the tests pin, in the order `golden::fixtures` lists
        /// them.
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
    edit_invalidate => "edit-invalidate",
    edit_way => "edit-way",
    edit_memory_only => "edit-memory-only",
    edit_memory_clean => "edit-memory-clean",
    edit_memory_dirty => "edit-memory-dirty",
    edit_memory_matches_dirty => "edit-memory-matches-dirty",
}

#[test]
fn every_fixture_has_a_snapshot() {
    let names: Vec<&str> = golden::fixtures()
        .iter()
        .map(|fixture| fixture.name)
        .collect();
    assert_eq!(names, SNAPSHOTS);
}

#[test]
fn every_share_string_opens_to_the_state_it_was_written_from() -> Result<(), String> {
    for fixture in golden::fixtures() {
        let session = run(&fixture)?;
        let opened = Session::from_share_string(&share(&session)?)
            .map_err(|error| format!("{}: {error}", fixture.name))?;
        assert_eq!(
            state_fingerprint(&opened)?,
            state_fingerprint(&session)?,
            "{}",
            fixture.name
        );
        assert_eq!(opened.to_json(), session.to_json(), "{}", fixture.name);
    }
    Ok(())
}
