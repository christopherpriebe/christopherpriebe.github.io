//! Sharing: share strings and saved files round-tripping a session, its
//! whole log with the undone ops, the mode chosen and the cursor, position 0
//! included; a saved file as the compact JSON its link holds, and a pretty
//! one still opening; the document's shape; a newer format refused; each
//! load failure with what the page shows for it, its message segments,
//! contents, repair and the actions before it; each size limit one byte or
//! one op over its constant; a session as long as a link can hold opening
//! from its link, and a run too long for one offered as a file; the runs
//! that once made a file too large to open, configuration changes and
//! programs loaded again and again, now stopped at the file's limit, saved
//! and opened; the session's limit stopping a pathological link; loading
//! in slices giving the session one slice gives, with the loading state's
//! summary; the share summary and its texts; and a trace file loaded as a
//! program.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::{
    INSTRUCTION_BUDGET, LOAD_SLICE_EVENTS, MAX_ADDRESS_BITS, MAX_CYCLES, MAX_EVENTS_PER_ENTRY,
    MAX_LINES, MAX_PROGRAM_BYTES, MAX_SEED, MAX_SHARE_JSON_BYTES, MAX_SHARE_OPS,
    MAX_SHARE_TEXT_BYTES, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS, MIN_CYCLES, PARSE_BYTES_PER_EVENT,
    SESSION_EVENT_LIMIT,
};
use cache_explorer_core::{
    CacheConfig, Config, ConfigChange, ConfigError, ConfigNumberField, EncodingError, LinkTooLong,
    LoadError, LoadFailure, LoadProgress, Op, OpError, Preset, ProgramFormat, Session, ShareAt,
    ShareDoc, ShareLoad, ShareSummary, TextSegment, TooLarge, WordBytes, SCHEMA_VERSION,
};
use serde_json::{json, Value};

use support::{
    apply_all, as_long_as_a_link, code, config, counted, failure_of, file_failure, grouped, link,
    link_failure, link_of, load, loaded, long_program, pathological, plain, pos, pretty, program,
    same, session, unpacked, varied, with_ops,
};

/// A loop without an exit and without a memory access: one event per
/// instruction until the budget or the session's limit stops it.
const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use cache_explorer_core::limits::{
        LOAD_SLICE_EVENTS, MAX_PROGRAM_BYTES, MAX_PROGRAM_LINES, MAX_SHARE_OPS,
    };
    use cache_explorer_core::{
        AccessKind, Config, EventPos, LoadError, LoadFailure, LoadProgress, Op, Preset,
        ProgramFormat, Session, ShareDoc, ShareLoad, TextSegment, TextStyle, Word,
    };
    use serde_json::{json, Value};

    use super::ENDLESS;

    /// A loop that loads and stores a line at a time and counts `r3` down
    /// from 40, then finishes.
    const WALK: &str = concat!(
        "        add r3, r0, #40\n",
        "        add r2, r0, #0x0A0\n",
        "loop:   ld r1, [r2]\n",
        "        st [r2], r3\n",
        "        add r2, r2, #16\n",
        "        add r3, r3, #-1\n",
        "        bnez r3, loop",
    );

    /// The tests' configuration: `direct-mapped-8`.
    pub(crate) fn config() -> Config {
        Preset::DirectMapped8.config()
    }

    /// A session with an empty log on `config()`.
    pub(crate) fn session() -> Session {
        Session::with_config(config()).unwrap()
    }

    /// A session whose log holds as many ops as a link can: loads by hand
    /// of one word, a miss and then hits.
    pub(crate) fn as_long_as_a_link() -> Session {
        let mut session = session();
        apply_all(&mut session, (0..MAX_SHARE_OPS).map(|_| load(0x0A4)));
        session
    }

    /// Position `value`, as one read from JSON is.
    pub(crate) fn pos(value: u32) -> EventPos {
        serde_json::from_value(value.into()).unwrap()
    }

    /// A load of `at` issued by hand.
    pub(crate) fn load(at: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: serde_json::from_value(at.into()).unwrap(),
            value: None,
        }
    }

    /// A store of `value` to `at` issued by hand.
    pub(crate) fn store(at: u32, value: u64) -> Op {
        let value: Word = serde_json::from_value(value.into()).unwrap();
        Op::Issue {
            kind: AccessKind::Write,
            addr: serde_json::from_value(at.into()).unwrap(),
            value: Some(value),
        }
    }

    /// A load of the assembly program `source`.
    pub(crate) fn program(source: &str) -> Op {
        Op::Program {
            source: source.to_owned(),
            format: ProgramFormat::Asm,
        }
    }

    /// An assembly program as long as a program may be: one instruction,
    /// then comment lines up to `MAX_PROGRAM_BYTES`, each long enough that
    /// there are fewer than `MAX_PROGRAM_LINES`. With a seed, the comments
    /// are pseudo-random text that deflate cannot shorten much, a different
    /// text for each seed; without one, the same text on every line.
    pub(crate) fn long_program(seed: Option<u64>) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let width = (MAX_PROGRAM_BYTES / MAX_PROGRAM_LINES + 1).max(32) as usize;
        let mut state = seed.unwrap_or_default() | 1;
        let mut source = String::from("add r1, r0, #1\n");
        while source.len() + width + 3 <= MAX_PROGRAM_BYTES as usize {
            source.push_str("; ");
            for index in 0..width {
                let pick = if seed.is_some() {
                    // xorshift64: six bits of each state.
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 58) as usize
                } else {
                    index % ALPHABET.len()
                };
                source.push(char::from(ALPHABET[pick]));
            }
            source.push('\n');
        }
        source
    }

    /// An edit of the memory word at 0x0A4 to 1.
    pub(crate) fn edit_word() -> Op {
        serde_json::from_value(json!({
            "op": "edit",
            "target": {"kind": "memory", "addr": 0x0A4},
            "patch": {"kind": "memory", "value": 1}
        }))
        .unwrap()
    }

    /// Applies each of `ops`, which must all be accepted.
    pub(crate) fn apply_all(session: &mut Session, ops: impl IntoIterator<Item = Op>) {
        for op in ops {
            let accepted = session.apply(op.clone());
            assert!(accepted.is_ok(), "{op:?}: {accepted:?}");
        }
    }

    /// A log of every kind of op across two runs, a timed one among them:
    /// loads and stores by hand with a dirty eviction, an edit, a program
    /// stepped, run and stepped to an access, and a new run that keeps it.
    pub(crate) fn varied() -> Session {
        let mut session = session();
        let timed: Config = serde_json::from_value(json!({
            "addressBits": 12, "wordBytes": 4,
            "cache": {"sizeBytes": 128, "lineBytes": 16, "associativity": 2,
                      "replacement": "fifo", "writePolicy": "writeBack",
                      "allocatePolicy": "writeAllocate"},
            "timing": {"hitCycles": 1, "missCycles": 20},
            "seed": 7, "memoryInit": "pattern"
        }))
        .unwrap();
        apply_all(
            &mut session,
            [
                load(0x0A4),
                store(0x0A8, 7),
                load(0x1A4),
                edit_word(),
                program(WALK),
                Op::Step,
                Op::Run { n: 3 },
                Op::StepAccess,
                Op::Config { config: timed },
                Op::Run { n: 12 },
                store(0x2A4, 9),
                Op::StepAccess,
            ],
        );
        session
    }

    /// A document of `ops` on `config()`, as JSON.
    pub(crate) fn with_ops(ops: &[Value]) -> Value {
        json!({"schema_version": 1, "config": config(), "ops": ops})
    }

    /// `text` as a share string: deflated and in base64url, whatever it is.
    pub(crate) fn link(text: &str) -> String {
        link_of(text.as_bytes())
    }

    /// `bytes` as a share string, whether or not they are text.
    pub(crate) fn link_of(bytes: &[u8]) -> String {
        URL_SAFE_NO_PAD.encode(miniz_oxide::deflate::compress_to_vec(bytes, 9))
    }

    /// The text the share string `s` holds: decoded and inflated, unchecked.
    pub(crate) fn unpacked(s: &str) -> String {
        let deflated = URL_SAFE_NO_PAD.decode(s).unwrap();
        String::from_utf8(miniz_oxide::inflate::decompress_to_vec(&deflated).unwrap()).unwrap()
    }

    /// `doc` as pretty JSON, as "Show link contents" shows a document read.
    pub(crate) fn pretty(doc: &ShareDoc) -> String {
        serde_json::to_string_pretty(doc).unwrap()
    }

    /// A link of `pairs` loads of a loop without an exit, each followed by a
    /// `StepAccess` that runs it to the end of its budget.
    pub(crate) fn pathological(pairs: usize) -> String {
        let pair = [
            json!({"op": "program", "source": ENDLESS, "format": "asm"}),
            json!({"op": "stepAccess"}),
        ];
        let ops: Vec<Value> = pair.iter().cycle().take(pairs * 2).cloned().collect();
        link(&with_ops(&ops).to_string())
    }

    /// Advances `load` in slices of `max_events` until it is done or has
    /// failed: every progress it reported, in order.
    pub(crate) fn loaded(load: &mut ShareLoad, max_events: u32) -> Vec<LoadProgress> {
        let mut progress = Vec::new();
        loop {
            let next = load.advance(max_events);
            let replaying = matches!(next, LoadProgress::Replaying { .. });
            progress.push(next);
            if !replaying {
                return progress;
            }
        }
    }

    /// Checks that `loaded` is the session `original` is, as the page sees
    /// it: the initial configuration and the mode chosen, the log, the undone
    /// ops included, the cursor, the snapshot there, the timeline and its
    /// ticks, and the document. The document keeps the mode chosen as its
    /// configuration's `advanced`, a flag no run reads, so that flag of the
    /// initial configuration is the mode once the session is opened. The
    /// live region's sentence is left out: an opened session announces its
    /// entry whole, whatever the step that led there.
    pub(crate) fn same(loaded: &Session, original: &Session, context: &str) {
        let flagged = |config: &Config| Config {
            advanced: false,
            ..config.clone()
        };
        assert_eq!(
            flagged(loaded.initial()),
            flagged(original.initial()),
            "{context}"
        );
        assert_eq!(loaded.advanced(), original.advanced(), "{context}");
        assert_eq!(loaded.ops(), original.ops(), "{context}");
        assert_eq!(loaded.position(), original.position(), "{context}");
        let (mut state, mut expected) = (loaded.state(), original.state());
        state.config = flagged(&state.config);
        expected.config = flagged(&expected.config);
        assert_eq!(state, expected, "{context}");
        assert_eq!(
            loaded.events_range(0..u32::MAX),
            original.events_range(0..u32::MAX),
            "{context}"
        );
        let mut timeline = loaded.timeline();
        let mut expected = original.timeline();
        timeline.live_text.clear();
        expected.live_text.clear();
        assert_eq!(timeline, expected, "{context}");
        assert_eq!(loaded.ticks(), original.ticks(), "{context}");
        assert_eq!(loaded.share_doc(), original.share_doc(), "{context}");
        assert_eq!(
            loaded.share_summary(),
            original.share_summary(),
            "{context}"
        );
    }

    /// The failure a load of `s` reports on its first advance.
    pub(crate) fn link_failure(s: &str) -> LoadFailure {
        match ShareLoad::from_share_string(s).advance(LOAD_SLICE_EVENTS) {
            LoadProgress::Failed { failure } => failure,
            progress => panic!("the link opens: {progress:?}"),
        }
    }

    /// The failure a load of the saved file `json` reports on its first
    /// advance.
    pub(crate) fn file_failure(json: &str) -> LoadFailure {
        match ShareLoad::from_json(json).advance(LOAD_SLICE_EVENTS) {
            LoadProgress::Failed { failure } => failure,
            progress => panic!("the file opens: {progress:?}"),
        }
    }

    /// The failure a load of the share string `s` reaches, however many
    /// slices that takes, with the error `take_session` then gives.
    pub(crate) fn failure_of(s: &str) -> (LoadFailure, LoadError) {
        let mut load = ShareLoad::from_share_string(s);
        let progress = loaded(&mut load, LOAD_SLICE_EVENTS);
        let Some(LoadProgress::Failed { failure }) = progress.last() else {
            panic!("the link opens: {:?}", progress.last());
        };
        (failure.clone(), load.take_session().unwrap_err())
    }

    /// Running text.
    pub(crate) fn plain(text: &str) -> TextSegment {
        TextSegment {
            text: text.to_owned(),
            style: TextStyle::Plain,
        }
    }

    /// Text in mono.
    pub(crate) fn code(text: &str) -> TextSegment {
        TextSegment {
            text: text.to_owned(),
            style: TextStyle::Code,
        }
    }

    /// Comma-grouped from 10,000 on: how messages print counts.
    pub(crate) fn counted(n: u32) -> String {
        if n < 10_000 {
            n.to_string()
        } else {
            grouped(n)
        }
    }

    /// Comma-grouped from 1,000 on: how messages print limits.
    pub(crate) fn grouped(n: impl Into<u64>) -> String {
        let digits = n.into().to_string();
        let mut text = String::new();
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index) % 3 == 0 {
                text.push(',');
            }
            text.push(digit);
        }
        text
    }
}

/// The text of `segments`, joined.
fn text(segments: &[TextSegment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

/// The lead of a link that cannot be opened.
const LINK_LEAD: &str =
    "This link doesn't describe a cache the Interactive Cache Explorer can build.";

/// The lead of a file that cannot be opened.
const FILE_LEAD: &str =
    "This file doesn't describe a cache the Interactive Cache Explorer can build.";

#[test]
fn a_share_string_reopens_the_session_wherever_its_cursor_is() {
    let mut original = varied();
    let end = original.position().pos.get();
    let mut places: Vec<u32> = (0..=end).step_by(23).collect();
    places.extend([0, 1, end - 1, end]);
    for place in places {
        original.seek(pos(place));
        let link = original.to_share_string().unwrap();
        let loaded = Session::from_share_string(&link).unwrap();
        same(&loaded, &original, &format!("at {place}"));
        // The same document gives the same string.
        assert_eq!(loaded.to_share_string(), Ok(link), "at {place}");
    }
}

#[test]
fn a_saved_file_reopens_the_session() {
    let mut original = varied();
    original.seek(pos(40));
    let json = original.to_json();
    let loaded = Session::from_json(&json).unwrap();
    same(&loaded, &original, "the file");
    assert_eq!(loaded.to_json(), json);
    // Compact JSON, the very text the session's link holds, so a file holds
    // whatever a link can; its size is known without writing it.
    assert!(json.starts_with("{\"schema_version\":1,\"config\":{\""));
    assert_eq!(json, serde_json::to_string(&original.share_doc()).unwrap());
    let link = original.to_share_string().unwrap();
    assert_eq!(unpacked(&link), json);
    assert_eq!(original.to_json_len() as usize, json.len());
    let doc = ShareDoc::from_share_string(&link).unwrap();
    assert_eq!(doc, ShareDoc::from_json(&json).unwrap());
    assert_eq!(doc, original.share_doc());
    assert_eq!(doc.to_json(), json);
}

#[test]
fn a_pretty_printed_file_still_opens() {
    let mut original = varied();
    original.seek(pos(40));
    let file = pretty(&original.share_doc());
    assert!(file.starts_with("{\n  \"schema_version\": 1,\n  \"config\": {\n"));
    assert!(file.len() > original.to_json().len());
    let opened = Session::from_json(&file).unwrap();
    same(&opened, &original, "a pretty file");
    assert_eq!(ShareDoc::from_json(&file).unwrap(), original.share_doc());
    // Saved again, it is compact.
    assert_eq!(opened.to_json(), original.to_json());
    let mut load = ShareLoad::from_json(&file);
    let progress = loaded(&mut load, LOAD_SLICE_EVENTS);
    assert!(matches!(progress.last(), Some(LoadProgress::Done { .. })));
    same(
        &load.take_session().unwrap(),
        &original,
        "a pretty file in slices",
    );
}

#[test]
fn the_document_keeps_its_keys_as_the_spec_spells_them() {
    let mut session = session();
    apply_all(
        &mut session,
        [
            load(0x0A4),
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
        ],
    );
    session.undo();
    let value: Value = serde_json::from_str(&session.to_json()).unwrap();
    let two_way = serde_json::to_value(Preset::TwoWayLru.config()).unwrap();
    assert_eq!(
        value,
        json!({
            "schema_version": SCHEMA_VERSION,
            "config": {
                "addressBits": Config::default().address_bits,
                "wordBytes": 4,
                "cache": {
                    "sizeBytes": 128,
                    "lineBytes": 16,
                    "associativity": 1,
                    "replacement": "lru",
                    "writePolicy": "writeBack",
                    "allocatePolicy": "writeAllocate"
                },
                "seed": 12345,
                "advanced": false,
                "memoryInit": "pattern"
            },
            "ops": [
                {"op": "issue", "kind": "read", "addr": 0x0A4},
                {"op": "config", "config": two_way}
            ],
            // Undone to the end of the load, the op's fifth event.
            "at": {"op": 0, "event": 5}
        })
    );
}

#[test]
fn the_document_keeps_the_undone_ops_and_position_0() {
    let mut original = session();
    apply_all(&mut original, [load(0x0A4), load(0x0A8), load(0x1A4)]);
    while original.undo() {}
    assert_eq!(original.position().pos, pos(0));
    let doc = original.share_doc();
    assert_eq!(doc.ops.len(), 3);
    assert_eq!(doc.at, Some(ShareAt { op: 0, event: 0 }));
    let loaded = Session::from_share_string(&original.to_share_string().unwrap()).unwrap();
    same(&loaded, &original, "position 0");
    assert_eq!(loaded.op_count(), 3);
    let mut redone = loaded.clone();
    assert!(redone.redo());
    assert_eq!(redone.position().pos, pos(5));
    // An empty log has no op to place the cursor by, and opens at 0.
    let empty = session();
    assert_eq!(empty.share_doc().at, None);
    same(
        &Session::from_share_string(&empty.to_share_string().unwrap()).unwrap(),
        &empty,
        "an empty log",
    );
}

#[test]
fn a_place_beyond_an_op_or_the_log_is_clamped_and_none_opens_at_the_end() {
    let mut original = session();
    apply_all(&mut original, [load(0x0A4), load(0x0A8), load(0x1A4)]);
    let end = original.position().pos;
    let mut doc = original.share_doc();
    let open = |doc: &ShareDoc| Session::from_json(&doc.to_json()).unwrap().position().pos;
    // The first load has five events, the second three.
    doc.at = Some(ShareAt { op: 1, event: 99 });
    assert_eq!(open(&doc), pos(8));
    doc.at = Some(ShareAt { op: 1, event: 0 });
    assert_eq!(open(&doc), pos(5));
    doc.at = Some(ShareAt { op: 7, event: 1 });
    assert_eq!(open(&doc), end);
    doc.at = None;
    assert_eq!(open(&doc), end);
}

#[test]
fn the_mode_chosen_is_kept() {
    let mut original = session();
    assert_eq!(original.set_advanced(true), ConfigChange::Applied);
    apply_all(&mut original, [load(0x0A4)]);
    assert!(original.share_doc().config.advanced);
    let loaded = Session::from_share_string(&original.to_share_string().unwrap()).unwrap();
    assert!(loaded.advanced());
    assert!(loaded.state().advanced);
    same(&loaded, &original, "advanced");
    // The document keeps the mode as its configuration's flag, which is the
    // opened session's initial mode.
    assert!(loaded.initial().advanced);
    assert_eq!(
        Config {
            advanced: false,
            ..loaded.initial().clone()
        },
        config()
    );
    // Back in Basics, the flag follows.
    let mut basics = loaded.clone();
    assert_eq!(basics.set_advanced(false), ConfigChange::Applied);
    assert!(!basics.share_doc().config.advanced);
    assert!(
        !Session::from_share_string(&basics.to_share_string().unwrap())
            .unwrap()
            .advanced()
    );
}

#[test]
fn a_share_string_is_padding_tolerant_base64url() {
    let mut original = varied();
    original.seek(pos(3));
    let link = original.to_share_string().unwrap();
    assert!(link
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
    let padding = (4 - link.len() % 4) % 4;
    let padded = format!("{link}{}", "=".repeat(padding));
    same(
        &Session::from_share_string(&padded).unwrap(),
        &original,
        "padded",
    );
}

#[test]
fn a_newer_format_is_refused_whatever_its_shape() {
    let newer = SCHEMA_VERSION + 1;
    let json = json!({"schema_version": newer, "config": {"ways": 3}, "cursor": 9}).to_string();
    let message = format!(
        "This run was saved by a newer version of the Interactive Cache Explorer (format {newer}); \
         this page reads format {SCHEMA_VERSION}. Reload the page to update."
    );
    let error = Session::from_share_string(&link(&json)).unwrap_err();
    assert_eq!(
        error,
        LoadError::NewerSchema {
            found: newer,
            supported: SCHEMA_VERSION
        }
    );
    assert_eq!(error.to_string(), message);
    assert_eq!(Session::from_json(&json).unwrap_err(), error);
    let failure = link_failure(&link(&json));
    assert_eq!(failure.title, "Could not open link");
    assert_eq!(failure.lead, LINK_LEAD);
    assert_eq!(failure.message, [plain(&message)]);
    // Its contents are shown as they are, pretty.
    let contents: Value = serde_json::from_str(failure.contents.as_deref().unwrap()).unwrap();
    assert_eq!(
        contents.to_string(),
        serde_json::from_str::<Value>(&json).unwrap().to_string()
    );
    assert_eq!(failure.repair, None);
    assert_eq!(failure.prefix, None);
}

#[test]
fn a_format_below_1_or_none_is_not_a_document() {
    let zero = json!({"schema_version": 0, "config": config(), "ops": []}).to_string();
    let LoadError::Json { line, message, .. } = Session::from_json(&zero).unwrap_err() else {
        panic!("format 0");
    };
    assert_eq!(line, 1);
    assert_eq!(
        message,
        "invalid value: integer `0`, expected a format version from 1"
    );
    let missing = json!({"config": config(), "ops": []}).to_string();
    let LoadError::Json { message, .. } = Session::from_json(&missing).unwrap_err() else {
        panic!("no format");
    };
    assert_eq!(message, "missing field `schema_version`");
}

#[test]
fn text_that_is_not_a_share_string_fails_to_decode() {
    let base64 = "not a link!";
    let deflate = "AAECAwQFBgcICQ";
    let utf8 = link_of(&[0xFF, 0xFE]);
    for (s, error, message) in [
        (
            base64,
            EncodingError::Base64,
            "The link's text is not valid base64url; it may have been cut short or changed.",
        ),
        (
            deflate,
            EncodingError::Deflate,
            "The link's data does not unpack; it may have been cut short or changed.",
        ),
        (
            utf8.as_str(),
            EncodingError::Utf8,
            "The link's data is not text; it may have been cut short or changed.",
        ),
    ] {
        let expected = LoadError::Encoding(error);
        assert_eq!(Session::from_share_string(s).unwrap_err(), expected, "{s}");
        assert_eq!(ShareDoc::from_share_string(s).unwrap_err(), expected, "{s}");
        assert_eq!(expected.to_string(), message);
        let failure = link_failure(s);
        assert_eq!(failure.title, "Could not open link");
        assert_eq!(failure.lead, LINK_LEAD);
        assert_eq!(failure.message, [plain(message)]);
        assert_eq!(failure.contents, None, "{s}");
        assert_eq!((failure.repair, failure.prefix), (None, None), "{s}");
    }
    // A link cut short.
    let whole = varied().to_share_string().unwrap();
    let cut = whole.get(..whole.len() / 2).unwrap();
    assert!(matches!(
        Session::from_share_string(cut).unwrap_err(),
        LoadError::Encoding(EncodingError::Base64 | EncodingError::Deflate)
    ));
}

#[test]
fn json_that_is_not_a_document_names_its_line_and_column() {
    let unknown = "{\"schema_version\":1,\"config\":{\"ways\":3},\"ops\":[]}";
    let error = Session::from_share_string(&link(unknown)).unwrap_err();
    let LoadError::Json {
        line,
        column,
        message,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!((*line, *column), (1, 36));
    assert!(message.starts_with("unknown field `ways`, expected one of `addressBits`, "));
    assert!(error
        .to_string()
        .starts_with("Line 1, column 36: unknown field `ways`"));
    let failure = link_failure(&link(unknown));
    assert_eq!(
        failure.message.get(..4).unwrap(),
        [
            plain("Line 1, column 36: unknown field "),
            code("ways"),
            plain(", expected one of "),
            code("addressBits"),
        ]
    );
    assert_eq!(text(&failure.message), error.to_string().replace('`', ""));
    // JSON, shown pretty; a document it is not, so there is nothing to
    // repair or replay.
    let contents = failure.contents.unwrap();
    assert!(
        contents.contains("\n  \"config\": {\n    \"ways\": 3\n  }"),
        "{contents}"
    );
    assert_eq!((failure.repair, failure.prefix), (None, None));
    // An op of no kind, on its own line of a file.
    let file = "{\n\"schema_version\": 1,\n\"config\": CONFIG,\n\"ops\": [{\"op\": \"stpe\"}]\n}"
        .replace("CONFIG", &serde_json::to_string(&config()).unwrap());
    let LoadError::Json { line, message, .. } = Session::from_json(&file).unwrap_err() else {
        panic!("an unknown op");
    };
    assert_eq!(line, 4);
    assert!(message.starts_with("unknown variant `stpe`, expected one of `config`, "));
    // Text that is not JSON is shown as it came.
    let broken = "{\"schema_version\":1,";
    let failure = file_failure(broken);
    assert_eq!(failure.title, "Could not open file");
    assert_eq!(failure.contents.as_deref(), Some(broken));
    // An empty file, or a link that unpacks to nothing, ends before its
    // first character, which is column 1 as columns count.
    let empty = LoadError::Json {
        line: 1,
        column: 1,
        message: "EOF while parsing a value".to_owned(),
    };
    assert_eq!(Session::from_json("").unwrap_err(), empty);
    assert_eq!(Session::from_share_string(&link("")).unwrap_err(), empty);
    assert_eq!(
        empty.to_string(),
        "Line 1, column 1: EOF while parsing a value"
    );
    assert_eq!(
        file_failure("").message,
        [plain("Line 1, column 1: EOF while parsing a value")]
    );
    // Text that ends just after a line break ends in the next line's first
    // column.
    let LoadError::Json { line, column, .. } = Session::from_json("{\n").unwrap_err() else {
        panic!("an object cut short");
    };
    assert_eq!((line, column), (2, 1));
    // A backtick in a name serde quotes stays in the name, which is still
    // set in mono, as are the names after it.
    let quoted = "{\"schema_version\":1,\"a`b\":2}";
    let error = Session::from_json(quoted).unwrap_err();
    let LoadError::Json { line, column, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(
        file_failure(quoted).message.get(..4).unwrap(),
        [
            plain(&format!("Line {line}, column {column}: unknown field ")),
            code("a`b"),
            plain(", expected one of "),
            code("schema_version"),
        ]
    );
}

#[test]
fn the_frame_5c_link_offers_its_repair() {
    // The design's 5c: a link whose cache has three ways in eight lines,
    // with six actions after it.
    let bad = Config {
        cache: CacheConfig {
            associativity: 3,
            ..config().cache
        },
        ..config()
    };
    let ops: Vec<Op> = [0x0A4, 0x0A8, 0x1A4, 0x0A4, 0x2A4, 0x0B0]
        .into_iter()
        .map(load)
        .collect();
    let doc = ShareDoc {
        schema_version: SCHEMA_VERSION,
        config: bad.clone(),
        ops: ops.clone(),
        at: Some(ShareAt { op: 3, event: 2 }),
    };
    let (failure, error) = failure_of(&doc.to_share_string());
    assert_eq!(
        error,
        LoadError::InitialConfig {
            error: ConfigError::AssociativityNotPowerOfTwo { got: 3, lines: 8 },
            total_ops: 6
        }
    );
    assert_eq!(
        error.to_string(),
        "Associativity 3 is not a power of two. It must be 1, 2, 4 or 8 (fully associative) \
         for an 8-line cache. The 6 actions after it were not replayed."
    );
    assert_eq!(failure.title, "Could not open link");
    assert_eq!(failure.lead, LINK_LEAD);
    assert_eq!(
        failure.message,
        [
            code("associativity: 3"),
            plain(
                " is not a power of two. It must be 1, 2, 4 or 8 (fully associative) for an \
                 8-line cache. The 6 actions after it were not replayed."
            ),
        ]
    );
    assert_eq!(failure.contents, Some(pretty(&doc)));
    assert_eq!(failure.prefix, None);
    // "Open with 2-way instead" opens the same document with the cache
    // repaired, at the same place.
    let repair = failure.repair.unwrap();
    assert_eq!(repair.label, "Open with 2-way instead");
    let repaired = Session::from_share_string(&repair.share).unwrap();
    assert_eq!(repaired.initial().cache.associativity, 2);
    assert_eq!(repaired.ops(), ops);
    assert_eq!(repaired.share_doc().at, doc.at);
    // "Start fresh" is the first visit's session.
    assert_eq!(Config::first_visit(), config());
}

#[test]
fn a_config_op_that_fails_offers_its_repair_and_the_actions_before_it() {
    let wide = Config {
        address_bits: MAX_ADDRESS_BITS + 8,
        ..Preset::TwoWayLru.config()
    };
    let mut ops: Vec<Op> = [0x0A4, 0x0A8, 0x1A4, 0x0A4, 0x2A4, 0x0B0, 0x0A4, 0x1A4]
        .into_iter()
        .map(load)
        .collect();
    ops.push(Op::Config { config: wide });
    ops.extend([0x0A4, 0x0A8, 0x1A4, 0x0A4, 0x2A4, 0x0B0].map(load));
    let doc = ShareDoc {
        schema_version: SCHEMA_VERSION,
        config: config(),
        ops: ops.clone(),
        at: None,
    };
    let (failure, error) = failure_of(&doc.to_share_string());
    assert_eq!(
        error,
        LoadError::Op {
            index: 8,
            error: OpError::Config(ConfigError::AddressBits {
                got: MAX_ADDRESS_BITS + 8
            }),
            not_replayed: 6
        }
    );
    assert_eq!(
        failure.message,
        [
            code(&format!("addressBits: {}", MAX_ADDRESS_BITS + 8)),
            plain(&format!(
                " must be between {MIN_ADDRESS_BITS} and {MAX_ADDRESS_BITS} bits. The 6 actions \
                 after it were not replayed."
            )),
        ]
    );
    let repair = failure.repair.unwrap();
    assert_eq!(
        repair.label,
        format!("Open with {MAX_ADDRESS_BITS}-bit addresses instead")
    );
    let repaired = Session::from_share_string(&repair.share).unwrap();
    assert_eq!(repaired.op_count(), 15);
    assert_eq!(repaired.config().address_bits, MAX_ADDRESS_BITS);
    // "Open the first 8 actions" opens the ops before it, at their end.
    let prefix = failure.prefix.unwrap();
    assert_eq!(prefix.label, "Open the first 8 actions");
    let before = Session::from_share_string(&prefix.share).unwrap();
    assert_eq!(before.ops(), ops.get(..8).unwrap());
    let mut expected = session();
    apply_all(&mut expected, ops.get(..8).unwrap().iter().cloned());
    same(&before, &expected, "the prefix");
}

#[test]
fn an_op_the_machine_refuses_offers_the_actions_before_it() {
    let mut ops: Vec<Op> = [0x0A4, 0x0A8, 0x1A4, 0x0A4, 0x2A4, 0x0B0, 0x0A4, 0x1A4]
        .into_iter()
        .map(load)
        .collect();
    ops.push(load(0x0A6));
    ops.extend([0x0A4, 0x0A8, 0x1A4, 0x0A4, 0x2A4, 0x0B0].map(load));
    let doc = ShareDoc {
        schema_version: SCHEMA_VERSION,
        config: config(),
        ops,
        at: Some(ShareAt { op: 2, event: 1 }),
    };
    let (failure, error) = failure_of(&doc.to_share_string());
    let LoadError::Op {
        index: 8,
        error: OpError::Access(_),
        not_replayed: 6,
    } = error
    else {
        panic!("{error:?}");
    };
    let message = "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8. The 6 actions after \
                   it were not replayed.";
    assert_eq!(error.to_string(), message);
    assert_eq!(failure.message, [plain(message)]);
    assert_eq!(failure.repair, None);
    assert_eq!(failure.contents, Some(pretty(&doc)));
    let prefix = failure.prefix.unwrap();
    assert_eq!(prefix.label, "Open the first 8 actions");
    let prefix = ShareDoc::from_share_string(&prefix.share).unwrap();
    assert_eq!(prefix.ops, doc.ops.get(..8).unwrap());
    assert_eq!(prefix.at, None);
    // A message that quotes code sets it in mono.
    let unparsed = with_ops(&[json!({"op": "program", "source": "lw r1", "format": "asm"})]);
    let (failure, _) = failure_of(&link(&unparsed.to_string()));
    assert!(
        failure.message.contains(&code("lw")),
        "{:?}",
        failure.message
    );
}

#[test]
fn the_actions_not_replayed_are_counted_in_words() {
    let failing = |before: usize, after: usize| {
        let mut ops = vec![json!({"op": "issue", "kind": "read", "addr": 0x0A4}); before];
        ops.push(json!({"op": "step"}));
        ops.extend(vec![
            json!({"op": "issue", "kind": "read", "addr": 0x0A4});
            after
        ]);
        failure_of(&link(&with_ops(&ops).to_string()))
    };
    let no_program = OpError::NoProgram.to_string();
    // The first op: nothing replayed before it, so no prefix.
    let (failure, error) = failing(0, 0);
    assert_eq!(error.to_string(), no_program);
    assert_eq!(failure.prefix, None);
    let (failure, error) = failing(1, 1);
    assert_eq!(
        error.to_string(),
        format!("{no_program} The action after it was not replayed.")
    );
    assert_eq!(failure.prefix.unwrap().label, "Open the first action");
    let (_, error) = failing(2, 12);
    assert_eq!(
        error.to_string(),
        format!("{no_program} The 12 actions after it were not replayed.")
    );
}

#[test]
fn a_file_that_cannot_be_opened_says_it_is_a_file() {
    let narrow = Config {
        address_bits: MIN_ADDRESS_BITS - 4,
        ..config()
    };
    let json = json!({"schema_version": 1, "config": narrow, "ops": []}).to_string();
    let failure = file_failure(&json);
    assert_eq!(failure.title, "Could not open file");
    assert_eq!(failure.lead, FILE_LEAD);
    assert_eq!(
        failure.message,
        [
            code(&format!("addressBits: {}", MIN_ADDRESS_BITS - 4)),
            plain(&format!(
                " must be between {MIN_ADDRESS_BITS} and {MAX_ADDRESS_BITS} bits."
            ))
        ]
    );
    let repair = failure.repair.unwrap();
    assert_eq!(
        repair.label,
        format!("Open with {MIN_ADDRESS_BITS}-bit addresses instead")
    );
    // The repair is a link, whatever it came from.
    assert_eq!(
        Session::from_share_string(&repair.share)
            .unwrap()
            .initial()
            .address_bits,
        MIN_ADDRESS_BITS
    );
}

#[test]
fn a_share_string_one_byte_over_its_limit_is_too_large() {
    let at_limit = "A".repeat(MAX_SHARE_TEXT_BYTES as usize);
    assert!(!matches!(
        Session::from_share_string(&at_limit).unwrap_err(),
        LoadError::Encoding(EncodingError::TooLarge(_))
    ));
    let over = "A".repeat(MAX_SHARE_TEXT_BYTES as usize + 1);
    let error = Session::from_share_string(&over).unwrap_err();
    assert_eq!(
        error,
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Text {
            bytes: MAX_SHARE_TEXT_BYTES + 1
        }))
    );
    assert_eq!(
        error.to_string(),
        format!(
            "The link is {} bytes long; at most {} bytes can be opened.",
            grouped(MAX_SHARE_TEXT_BYTES + 1),
            grouped(MAX_SHARE_TEXT_BYTES)
        )
    );
    // Nothing was decoded, so there is nothing to show, repair or replay.
    assert_eq!(
        link_failure(&over),
        LoadFailure {
            title: "Could not open link".to_owned(),
            lead: LINK_LEAD.to_owned(),
            message: vec![plain(&error.to_string())],
            contents: None,
            repair: None,
            prefix: None,
        }
    );
}

#[test]
fn json_one_byte_over_its_limit_is_too_large() {
    let mut original = session();
    apply_all(&mut original, [load(0x0A4)]);
    let json = original.to_json();
    // Padded with spaces, which JSON allows after a value, to a size.
    let padded = |size: u32| format!("{json}{}", " ".repeat(size as usize - json.len()));
    let at_limit = padded(MAX_SHARE_JSON_BYTES);
    same(
        &Session::from_json(&at_limit).unwrap(),
        &original,
        "a file at the limit",
    );
    let link_at_limit = link(&at_limit);
    assert!(link_at_limit.len() < MAX_SHARE_TEXT_BYTES as usize);
    same(
        &Session::from_share_string(&link_at_limit).unwrap(),
        &original,
        "a link at the limit",
    );
    let over = padded(MAX_SHARE_JSON_BYTES + 1);
    let error = Session::from_json(&over).unwrap_err();
    assert_eq!(
        error,
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Json {
            bytes: Some(MAX_SHARE_JSON_BYTES + 1)
        }))
    );
    assert_eq!(
        error.to_string(),
        format!(
            "The saved run is {} bytes of JSON; at most {} bytes can be opened.",
            grouped(MAX_SHARE_JSON_BYTES + 1),
            grouped(MAX_SHARE_JSON_BYTES)
        )
    );
    // The file is not read, so there is nothing to show, repair or replay.
    assert_eq!(
        file_failure(&over),
        LoadFailure {
            title: "Could not open file".to_owned(),
            lead: FILE_LEAD.to_owned(),
            message: vec![plain(&error.to_string())],
            contents: None,
            repair: None,
            prefix: None,
        }
    );
    // A link unpacks no further than the limit.
    let error = Session::from_share_string(&link(&over)).unwrap_err();
    assert_eq!(
        error,
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Json { bytes: None }))
    );
    assert_eq!(
        error.to_string(),
        format!(
            "The link unpacks to more than the {} bytes of JSON that can be opened.",
            grouped(MAX_SHARE_JSON_BYTES)
        )
    );
    assert_eq!(
        link_failure(&link(&over)),
        LoadFailure {
            title: "Could not open link".to_owned(),
            lead: LINK_LEAD.to_owned(),
            message: vec![plain(&error.to_string())],
            contents: None,
            repair: None,
            prefix: None,
        }
    );
}

#[test]
fn a_log_one_op_over_its_limit_is_too_large() {
    let steps = |count: u32| {
        let ops = vec![json!({"op": "step"}); count as usize];
        with_ops(&ops).to_string()
    };
    // At the limit the document reads; replaying it is another matter.
    let at_limit = ShareDoc::from_json(&steps(MAX_SHARE_OPS)).unwrap();
    assert_eq!(at_limit.ops.len(), MAX_SHARE_OPS as usize);
    let over = steps(MAX_SHARE_OPS + 1);
    let error = Session::from_share_string(&link(&over)).unwrap_err();
    assert_eq!(
        error,
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Ops {
            ops: MAX_SHARE_OPS + 1
        }))
    );
    assert_eq!(
        error.to_string(),
        format!(
            "The saved run has {} actions; at most {} can be opened.",
            grouped(MAX_SHARE_OPS + 1),
            grouped(MAX_SHARE_OPS)
        )
    );
    assert_eq!(Session::from_json(&over).unwrap_err(), error);
    // The document was read, so its contents are there to show; nothing
    // replayed, so there is nothing to repair or to open before it.
    let read: ShareDoc = serde_json::from_str(&over).unwrap();
    let failure = |title: &str, lead: &str| LoadFailure {
        title: title.to_owned(),
        lead: lead.to_owned(),
        message: vec![plain(&error.to_string())],
        contents: Some(pretty(&read)),
        repair: None,
        prefix: None,
    };
    assert_eq!(
        link_failure(&link(&over)),
        failure("Could not open link", LINK_LEAD)
    );
    assert_eq!(
        file_failure(&over),
        failure("Could not open file", FILE_LEAD)
    );
}

#[test]
fn a_program_one_byte_over_its_limit_is_too_large() {
    let source = |bytes: u32| format!("{}\n", ";".repeat(bytes as usize - 1));
    let doc = |bytes: u32| {
        with_ops(&[
            json!({"op": "issue", "kind": "read", "addr": 0x0A4}),
            json!({"op": "program", "source": source(bytes), "format": "trace"}),
        ])
        .to_string()
    };
    assert_eq!(
        ShareDoc::from_json(&doc(MAX_PROGRAM_BYTES))
            .unwrap()
            .ops
            .len(),
        2
    );
    let error = Session::from_share_string(&link(&doc(MAX_PROGRAM_BYTES + 1))).unwrap_err();
    assert_eq!(
        error,
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Program {
            op: 1,
            bytes: MAX_PROGRAM_BYTES + 1
        }))
    );
    assert_eq!(
        error.to_string(),
        format!(
            "A program in the saved run is {} bytes; programs of at most {} bytes can be \
             opened.",
            grouped(MAX_PROGRAM_BYTES + 1),
            grouped(MAX_PROGRAM_BYTES)
        )
    );
    // The document was read, the program too long included, so its
    // contents are there to show; nothing replayed, so there is nothing to
    // repair or to open before it.
    let over = doc(MAX_PROGRAM_BYTES + 1);
    let read: ShareDoc = serde_json::from_str(&over).unwrap();
    let failure = |title: &str, lead: &str| LoadFailure {
        title: title.to_owned(),
        lead: lead.to_owned(),
        message: vec![plain(&error.to_string())],
        contents: Some(pretty(&read)),
        repair: None,
        prefix: None,
    };
    assert_eq!(
        link_failure(&link(&over)),
        failure("Could not open link", LINK_LEAD)
    );
    assert_eq!(Session::from_json(&over).unwrap_err(), error);
    assert_eq!(
        file_failure(&over),
        failure("Could not open file", FILE_LEAD)
    );
}

#[test]
fn a_session_as_long_as_a_link_can_hold_opens_from_its_link() {
    let original = as_long_as_a_link();
    let link = original.to_share_string().unwrap();
    assert!(link.len() <= MAX_SHARE_TEXT_BYTES as usize);
    same(
        &Session::from_share_string(&link).unwrap(),
        &original,
        "the link",
    );
    same(
        &Session::from_json(&original.to_json()).unwrap(),
        &original,
        "the file",
    );
    // The session refuses one op more, which no link could open.
    let mut longer = original.share_doc();
    longer.ops.push(load(0x0A4));
    assert_eq!(
        ShareDoc::from_share_string(&longer.to_share_string()),
        Err(LoadError::Encoding(EncodingError::TooLarge(
            TooLarge::Ops {
                ops: MAX_SHARE_OPS + 1
            }
        )))
    );
}

#[test]
fn a_run_too_long_for_a_link_offers_a_file() {
    // Programs of text deflate cannot shorten much, until the run is more
    // than a link can hold, though not more than a file can.
    let mut original = session();
    let mut seed = 0;
    let error = loop {
        match original.to_share_string() {
            Ok(link) => assert!(link.len() <= MAX_SHARE_TEXT_BYTES as usize),
            Err(error) => break error,
        }
        seed += 1;
        apply_all(&mut original, [program(&long_program(Some(seed)))]);
    };
    // The link it would have written is one no link could open.
    let unopenable = original.share_doc().to_share_string();
    let bytes = u32::try_from(unopenable.len()).unwrap();
    assert!(bytes > MAX_SHARE_TEXT_BYTES);
    assert_eq!(
        Session::from_share_string(&unopenable).unwrap_err(),
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Text { bytes }))
    );
    assert_eq!(error, LinkTooLong { bytes });
    assert_eq!(
        error.to_string(),
        format!(
            "This run is too long for a link: its link would be {} bytes long, and at most {} \
             bytes can be opened. Save it as a file instead.",
            grouped(bytes),
            grouped(MAX_SHARE_TEXT_BYTES)
        )
    );
    // The file, the JSON the link would have unpacked to, opens.
    let json = original.to_json();
    assert_eq!(unpacked(&unopenable), json);
    assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize);
    same(&Session::from_json(&json).unwrap(), &original, "the file");
}

/// Applies each of `ops` in turn until the session refuses one: the error.
fn until_refused(session: &mut Session, ops: impl IntoIterator<Item = Op>) -> Option<OpError> {
    ops.into_iter().find_map(|op| session.apply(op).err())
}

#[test]
fn configuration_changes_stop_at_the_file_limit() {
    // Configuration changes, which a link shortens well and which once
    // made a link unpack to more JSON than can be opened: the session
    // refuses the one that would take its saved file past the limit, well
    // before the ops a link can hold, and its file and its link open. Each
    // pair of changes adds the same JSON, so their count is measured, not
    // searched for.
    let mut original = session();
    let pair = [
        Op::Config {
            config: Preset::TwoWayLru.config(),
        },
        Op::Config { config: config() },
    ];
    apply_all(&mut original, pair.clone());
    let before = original.to_json_len();
    apply_all(&mut original, pair.clone());
    let added = original.to_json_len() - before;
    let pairs = (MAX_SHARE_JSON_BYTES - original.to_json_len()) / added + 1;
    let ops = pair.iter().cycle().take(pairs as usize * 2).cloned();
    assert_eq!(until_refused(&mut original, ops), Some(OpError::FileFull));
    assert!(original.op_count() < MAX_SHARE_OPS);
    let json = original.to_json();
    assert_eq!(original.to_json_len() as usize, json.len());
    assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize);
    same(&Session::from_json(&json).unwrap(), &original, "the file");
    let link = original.to_share_string().unwrap();
    assert!(link.len() <= MAX_SHARE_TEXT_BYTES as usize);
    assert_eq!(unpacked(&link), json);
    same(
        &Session::from_share_string(&link).unwrap(),
        &original,
        "the link",
    );
}

#[test]
fn one_program_loaded_again_and_again_stops_at_the_file_limit() {
    // The same long program loaded again and again, which a link shortens
    // well, and which once made a link unpack to more JSON than can be
    // opened: the session refuses the load that would take its saved file
    // past the limit, and its file and its link open.
    let source = long_program(None);
    let mut original = session();
    let loads = std::iter::repeat_with(|| program(&source));
    assert_eq!(until_refused(&mut original, loads), Some(OpError::FileFull));
    let json = original.to_json();
    assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize);
    same(&Session::from_json(&json).unwrap(), &original, "the file");
    let link = original.to_share_string().unwrap();
    assert!(link.len() <= MAX_SHARE_TEXT_BYTES as usize);
    assert_eq!(unpacked(&link), json);
    same(
        &Session::from_share_string(&link).unwrap(),
        &original,
        "the link",
    );
    // With one more load, which no session writes, the document is more
    // JSON than a file or a link may hold.
    let mut longer = original.share_doc();
    longer.ops.push(program(&source));
    let bytes = u32::try_from(longer.to_json().len()).unwrap();
    assert!(bytes > MAX_SHARE_JSON_BYTES);
    assert_eq!(
        Session::from_json(&longer.to_json()).unwrap_err(),
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Json {
            bytes: Some(bytes)
        }))
    );
    assert_eq!(
        Session::from_share_string(&longer.to_share_string()).unwrap_err(),
        LoadError::Encoding(EncodingError::TooLarge(TooLarge::Json { bytes: None }))
    );
}

#[test]
fn a_run_too_long_for_a_link_still_saves_once_its_file_is_full() {
    // Programs of text deflate cannot shorten much, until the link is too
    // long; then configuration changes, which once took the saved file past
    // what can be opened too, so that neither could be. The session refuses
    // the change that would take the file past its limit: the link is too
    // long, and the file, the JSON it would unpack to, opens.
    let mut original = session();
    let mut seed = 0;
    while original.to_share_string().is_ok() {
        seed += 1;
        apply_all(&mut original, [program(&long_program(Some(seed)))]);
    }
    let pair = [
        Op::Config {
            config: Preset::TwoWayLru.config(),
        },
        Op::Config { config: config() },
    ];
    let changes = pair.iter().cycle().take(MAX_SHARE_OPS as usize).cloned();
    assert_eq!(
        until_refused(&mut original, changes),
        Some(OpError::FileFull)
    );
    let json = original.to_json();
    assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize);
    let unopenable = original.share_doc().to_share_string();
    assert_eq!(unpacked(&unopenable), json);
    assert_eq!(
        original.to_share_string(),
        Err(LinkTooLong {
            bytes: u32::try_from(unopenable.len()).unwrap()
        })
    );
    same(&Session::from_json(&json).unwrap(), &original, "the file");
}

#[test]
fn the_session_limit_stops_a_pathological_link() {
    // Thousands of ops, each pair able to execute a whole budget.
    let pairs = MAX_SHARE_OPS / 2;
    let link = pathological(pairs as usize);
    assert!(link.len() < MAX_SHARE_TEXT_BYTES as usize);
    let mut load = ShareLoad::from_share_string(&link);
    let progress = loaded(&mut load, LOAD_SLICE_EVENTS);
    // Bounded work: every slice but the last replays all but an entry's
    // worth of its events, and together they replay no more than the
    // session holds.
    let slices = SESSION_EVENT_LIMIT / (LOAD_SLICE_EVENTS - MAX_EVENTS_PER_ENTRY) + 2;
    assert!(
        progress.len() <= slices as usize,
        "{} slices",
        progress.len()
    );
    let Some(LoadProgress::Failed { failure }) = progress.last() else {
        panic!("the link opens: {:?}", progress.last());
    };
    let error = load.take_session().unwrap_err();
    let LoadError::Op {
        index,
        error: OpError::SessionFull,
        not_replayed,
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(index + 1 + not_replayed, pairs * 2);
    assert!(index > 2 && index % 2 == 0, "a program load fails: {index}");
    assert_eq!(
        text(&failure.message),
        format!(
            "This session has reached its limit of {} steps. Undo to make room, or save it and \
             start fresh. The {} actions after it were not replayed.",
            grouped(SESSION_EVENT_LIMIT),
            counted(not_replayed)
        )
    );
    assert_eq!(failure.repair, None);
    // The actions before it open, and fill the session to its limit.
    let prefix = failure.prefix.clone().unwrap();
    assert_eq!(prefix.label, format!("Open the first {index} actions"));
    let before = Session::from_share_string(&prefix.share).unwrap();
    assert_eq!(before.op_count(), index);
    let events = u32::try_from(before.events_range(0..u32::MAX).len()).unwrap();
    assert!(events <= SESSION_EVENT_LIMIT);
    assert!(events + MAX_EVENTS_PER_ENTRY > SESSION_EVENT_LIMIT);
}

#[test]
fn a_slice_counts_the_programs_its_ops_parse() {
    // A program as long as a program may be, then configuration changes that
    // each parse it again under the new configuration: one event an op, but
    // a program's parsing each.
    let source = long_program(None);
    let changes: u32 = 64;
    let mut original = Session::with_config(Preset::TwoWayLru.config()).unwrap();
    apply_all(&mut original, [program(&source)]);
    for change in 0..changes {
        let preset = if change % 2 == 0 {
            Preset::FourWayWriteBack
        } else {
            Preset::TwoWayLru
        };
        apply_all(
            &mut original,
            [Op::Config {
                config: preset.config(),
            }],
        );
    }
    assert!(original.state().program.is_some(), "every run keeps it");
    let link = original.to_share_string().unwrap();
    let mut load = ShareLoad::from_share_string(&link);
    let progress = loaded(&mut load, LOAD_SLICE_EVENTS);
    // Each slice parses no more than its events' worth of source, so the
    // program and its parses again take a slice for every
    // `LOAD_SLICE_EVENTS` of them.
    let parses = u64::from(changes + 1) * (source.len() as u64 / u64::from(PARSE_BYTES_PER_EVENT));
    assert!(
        progress.len() as u64 >= parses / u64::from(LOAD_SLICE_EVENTS),
        "{} slices for {parses} events of parsing",
        progress.len()
    );
    assert!(matches!(progress.last(), Some(LoadProgress::Done { .. })));
    same(
        &load.take_session().unwrap(),
        &original,
        "sliced by parsing",
    );
}

#[test]
fn the_loading_state_opens_at_the_step_named_once_the_replay_passes_it() {
    // A place in the middle of one long op, which slices end inside.
    let mut original = Session::with_config(Preset::TwoWayLru.config()).unwrap();
    apply_all(&mut original, [program(ENDLESS), Op::StepAccess]);
    original.seek(pos(1 + INSTRUCTION_BUDGET / 2));
    let opening = original.share_summary().opening_step;
    assert!(opening > 0 && opening < original.share_summary().step_count);
    let link = original.to_share_string().unwrap();
    let mut passed = false;
    for max_events in [1, MAX_EVENTS_PER_ENTRY, 100, LOAD_SLICE_EVENTS] {
        let mut load = ShareLoad::from_share_string(&link);
        let progress = loaded(&mut load, max_events);
        let Some((LoadProgress::Done { summary }, replaying)) = progress.split_last() else {
            panic!("{max_events}: {:?}", progress.last());
        };
        assert_eq!(summary.opening_step, opening, "{max_events}");
        for progress in replaying {
            let LoadProgress::Replaying { summary: so_far } = progress else {
                panic!("{progress:?}");
            };
            // The steps so far until the replay has passed the place, then
            // the place's step, even while its op is still replaying.
            assert_eq!(
                so_far.opening_step,
                so_far.step_count.min(opening),
                "{max_events}: {} steps so far",
                so_far.step_count
            );
            passed |= so_far.step_count > opening;
        }
        same(
            &load.take_session().unwrap(),
            &original,
            &format!("slices of {max_events}"),
        );
    }
    assert!(passed, "a slice ends inside the op, after the place");
}

#[test]
fn a_file_too_large_for_a_link_offers_no_link() {
    // Programs of text deflate cannot shorten much, until the document is
    // more than a link can hold, though not more than a file can.
    let mut doc = ShareDoc {
        schema_version: SCHEMA_VERSION,
        config: config(),
        ops: Vec::new(),
        at: None,
    };
    let mut seed = 0;
    while doc.to_share_string().len() <= MAX_SHARE_TEXT_BYTES as usize {
        seed += 1;
        doc.ops.push(program(&long_program(Some(seed))));
    }
    assert!(doc.to_json().len() <= MAX_SHARE_JSON_BYTES as usize);
    let programs = doc.ops.len();
    // A first run whose cache fails: its repair is a link too long to open.
    let three = Config {
        cache: CacheConfig {
            associativity: 3,
            ..config().cache
        },
        ..config()
    };
    let bad = ShareDoc {
        config: three.clone(),
        ..doc.clone()
    };
    let failure = file_failure(&bad.to_json());
    assert_eq!(failure.title, "Could not open file");
    assert!(matches!(
        Session::from_json(&bad.to_json()).unwrap_err(),
        LoadError::InitialConfig { .. }
    ));
    assert_eq!(failure.contents, Some(pretty(&bad)));
    assert_eq!(failure.repair, None);
    assert_eq!(failure.prefix, None);
    // A configuration change that fails after the programs: its repair and
    // the ops before it are links too long to open.
    let mut bad = doc.clone();
    bad.ops.push(Op::Config { config: three });
    bad.ops.push(load(0x0A4));
    let error = Session::from_json(&bad.to_json()).unwrap_err();
    assert!(
        matches!(error, LoadError::Op { index, error: OpError::Config(_), .. }
            if index as usize == programs),
        "{error:?}"
    );
    let failure = file_failure(&bad.to_json());
    assert_eq!(failure.repair, None);
    assert_eq!(failure.prefix, None);
    // A file whose repair fits a link still offers it.
    let small = ShareDoc {
        config: Config {
            cache: CacheConfig {
                associativity: 3,
                ..config().cache
            },
            ..config()
        },
        ops: doc.ops.get(..1).unwrap().to_vec(),
        ..doc
    };
    let repair = file_failure(&small.to_json()).repair.unwrap();
    assert!(Session::from_share_string(&repair.share).is_ok());
}

#[test]
fn loading_in_slices_gives_the_session_one_slice_gives() {
    let mut original = varied();
    original.seek(pos(61));
    let link = original.to_share_string().unwrap();
    let whole = Session::from_share_string(&link).unwrap();
    same(&whole, &original, "one slice");
    for max_events in [1, 2, MAX_EVENTS_PER_ENTRY, 100, LOAD_SLICE_EVENTS, u32::MAX] {
        let mut load = ShareLoad::from_share_string(&link);
        let progress = loaded(&mut load, max_events);
        let Some((LoadProgress::Done { summary }, replaying)) = progress.split_last() else {
            panic!("{max_events}: {:?}", progress.last());
        };
        assert_eq!(summary, &whole.share_summary(), "{max_events}");
        // The loading state's ticks fill as the run replays.
        let mut ticks: &[u8] = &[];
        for progress in replaying {
            let LoadProgress::Replaying { summary: so_far } = progress else {
                panic!("{progress:?}");
            };
            assert!(so_far.ticks.starts_with(ticks), "{max_events}");
            assert!(summary.ticks.starts_with(&so_far.ticks), "{max_events}");
            assert_eq!(so_far.step_count as usize, so_far.ticks.len());
            assert_eq!(
                so_far.opening_step,
                so_far.step_count.min(summary.opening_step),
                "{max_events}"
            );
            assert_eq!(so_far.runs, summary.runs);
            assert_eq!(so_far.op_count, summary.op_count);
            assert_eq!(so_far.config_summary, summary.config_summary);
            ticks = &so_far.ticks;
        }
        let session = load.take_session().unwrap();
        same(&session, &whole, &format!("slices of {max_events}"));
        assert_eq!(session.timeline().live_text, whole.timeline().live_text);
    }
    // Small slices take many frames; the facade's take few.
    let mut load = ShareLoad::from_share_string(&link);
    assert!(loaded(&mut load, 1).len() > original.op_count() as usize);
}

#[test]
fn the_ticks_a_load_reports_are_the_ticks_of_the_session_it_opens() {
    // The cursor is in the first run, whose entries miss, hit and execute
    // instructions; a second run follows it in the log, so the first run
    // has replayed in full before the last slice.
    let mut original = varied();
    original.seek(pos(3));
    let mut load = ShareLoad::from_share_string(&original.to_share_string().unwrap());

    let progress = loaded(&mut load, 1);

    let ticks = load.take_session().unwrap().ticks();
    for tick in [0, 1, 2, 3] {
        assert!(ticks.contains(&tick), "no tick {tick} in {ticks:?}");
    }
    let Some((LoadProgress::Done { summary: done }, replaying)) = progress.split_last() else {
        panic!("{:?}", progress.last());
    };
    assert_eq!(done.ticks, ticks);
    let Some(LoadProgress::Replaying { summary: last }) = replaying.last() else {
        panic!("{replaying:?}");
    };
    assert_eq!(last.ticks, ticks);
}

#[test]
fn a_session_is_taken_once_the_load_is_done_and_only_once() {
    let link = varied().to_share_string().unwrap();
    let mut load = ShareLoad::from_share_string(&link);
    assert_eq!(load.take_session().unwrap_err(), LoadError::Unfinished);
    assert_eq!(
        LoadError::Unfinished.to_string(),
        "The run has not finished loading."
    );
    let progress = loaded(&mut load, 50);
    assert!(progress.len() > 1);
    assert!(load.take_session().is_ok());
    assert_eq!(load.take_session().unwrap_err(), LoadError::Taken);
    assert_eq!(
        LoadError::Taken.to_string(),
        "The run has already been opened."
    );
    // A done load answers done again.
    assert!(matches!(load.advance(1), LoadProgress::Done { .. }));
    let mut failed = ShareLoad::from_share_string("!");
    assert!(matches!(failed.advance(1), LoadProgress::Failed { .. }));
    assert!(matches!(failed.advance(1), LoadProgress::Failed { .. }));
    assert_eq!(
        failed.take_session().unwrap_err(),
        LoadError::Encoding(EncodingError::Base64)
    );
}

#[test]
fn the_loading_summary_is_the_design_s_frame_5b() {
    let mut original = Session::with_config(Preset::TwoWayLru.config()).unwrap();
    apply_all(&mut original, [load(0x0A4), load(0x0A8), load(0x1A4)]);
    original.seek(pos(9));
    let summary = original.share_summary();
    assert_eq!(
        summary,
        ShareSummary {
            config_summary: "2-way LRU \u{b7} 128 B".to_owned(),
            runs: 1,
            op_count: 3,
            step_count: 14,
            opening_step: 9,
            text: "2-way LRU \u{b7} 128 B \u{b7} 14 steps, opening at step 9.".to_owned(),
            copied_text: "Link copied. It encodes the configuration and all 14 steps, and opens \
                          at step 9."
                .to_owned(),
            ticks: original.ticks(),
        }
    );
    // Misses mark every step of their entry; first steps are tall.
    assert_eq!(summary.ticks, [3, 2, 2, 2, 2, 1, 0, 0, 3, 2, 2, 2, 2, 2]);
    let mut load = ShareLoad::from_share_string(&original.to_share_string().unwrap());
    let progress = loaded(&mut load, 1);
    let Some(LoadProgress::Done { summary: done }) = progress.last() else {
        panic!("{progress:?}");
    };
    assert_eq!(done, &summary);
    // Until the replay passes step 9, the run opens at the step so far.
    let openings: Vec<(u32, u32)> = progress
        .iter()
        .map(|progress| match progress {
            LoadProgress::Replaying { summary } | LoadProgress::Done { summary } => {
                (summary.step_count, summary.opening_step)
            }
            LoadProgress::Failed { failure } => panic!("{failure:?}"),
        })
        .collect();
    assert_eq!(openings, [(5, 5), (8, 8), (14, 9)]);
}

#[test]
fn the_share_summary_counts_the_opening_run() {
    let mut session = session();
    let empty = session.share_summary();
    assert_eq!(
        (empty.text.as_str(), empty.copied_text.as_str()),
        (
            "Direct-mapped \u{b7} 128 B \u{b7} no steps yet.",
            "Link copied. It encodes the configuration; there are no steps yet."
        )
    );
    apply_all(&mut session, [load(0x0A4)]);
    session.seek(pos(0));
    let at_start = session.share_summary();
    assert_eq!(
        (at_start.text.as_str(), at_start.copied_text.as_str()),
        (
            "Direct-mapped \u{b7} 128 B \u{b7} 5 steps, opening before the first.",
            "Link copied. It encodes the configuration and all 5 steps, and opens before the \
             first."
        )
    );
    // Several runs: the one holding the cursor.
    apply_all(
        &mut session,
        [
            Op::Config {
                config: Preset::FourWayWriteBack.config(),
            },
            load(0x0A4),
            load(0x0A8),
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
        ],
    );
    let last = session.share_summary();
    assert_eq!((last.runs, last.step_count, last.opening_step), (3, 0, 0));
    assert_eq!(
        (last.text.as_str(), last.copied_text.as_str()),
        (
            "2-way LRU \u{b7} 128 B \u{b7} 3 runs; this one has no steps yet.",
            "Link copied. It encodes all 3 runs, and opens this one, which has no steps yet."
        )
    );
    session.undo();
    session.seek(pos(session.position().pos.get() - 2));
    let middle = session.share_summary();
    assert_eq!(middle.config_summary, "4-way LRU \u{b7} 256 B");
    assert_eq!(
        (middle.runs, middle.step_count, middle.opening_step),
        (3, 8, 6)
    );
    assert_eq!(
        (middle.text.as_str(), middle.copied_text.as_str()),
        (
            "4-way LRU \u{b7} 256 B \u{b7} 3 runs; this one has 8 steps, opening at step 6.",
            "Link copied. It encodes all 3 runs, and opens this one at step 6 of 8."
        )
    );
    // The loading state agrees once it is done.
    let mut load = ShareLoad::from_share_string(&session.to_share_string().unwrap());
    let progress = loaded(&mut load, 1);
    assert_eq!(
        progress.last(),
        Some(&LoadProgress::Done { summary: middle })
    );
}

#[test]
fn a_trace_file_loads_as_a_program_op_on_the_current_session() {
    let mut session = session();
    apply_all(&mut session, [load(0x0A4)]);
    let trace = "R 0x0A4\nW 0x0A8 7\nR 0x1A4\n";
    let parsed = session.parse_program(trace, ProgramFormat::Trace);
    assert!(
        serde_json::to_value(&parsed).unwrap()["status"] == "ok",
        "{parsed:?}"
    );
    let op = Op::Program {
        source: trace.to_owned(),
        format: ProgramFormat::Trace,
    };
    session.apply(op.clone()).unwrap();
    apply_all(&mut session, [Op::StepAccess, Op::StepAccess]);
    assert_eq!(session.op_count(), 4);
    assert_eq!(session.ops().get(1), Some(&op));
    let value: Value = serde_json::from_str(&session.to_json()).unwrap();
    assert_eq!(
        value["ops"][1],
        json!({"op": "program", "source": trace, "format": "trace"})
    );
    same(
        &Session::from_share_string(&session.to_share_string().unwrap()).unwrap(),
        &session,
        "a trace",
    );
}

#[test]
fn each_rule_of_addresses_sizes_and_lines_opens_with_its_field_in_mono() {
    let cases = [
        (
            ConfigError::AddressBits { got: 40 },
            "addressBits: 40".to_owned(),
            format!(
                " must be between {} and {} bits.",
                grouped(MIN_ADDRESS_BITS),
                grouped(MAX_ADDRESS_BITS)
            ),
        ),
        (
            ConfigError::SizeNotPowerOfTwo { got: 100 },
            "sizeBytes: 100".to_owned(),
            " must be a power of two, such as 64, 128 or 256 bytes.".to_owned(),
        ),
        (
            ConfigError::LineNotPowerOfTwo { got: 12 },
            "lineBytes: 12".to_owned(),
            " must be a power of two, such as 8, 16 or 32 bytes.".to_owned(),
        ),
        (
            ConfigError::LineSmallerThanWord { line: 2, word: 4 },
            "lineBytes: 2".to_owned(),
            " is smaller than one 4 B word. A line must hold at least one word.".to_owned(),
        ),
        (
            ConfigError::LineLargerThanCache { line: 16, size: 8 },
            "sizeBytes: 8".to_owned(),
            " cannot hold one 16 B line. The cache must hold at least one line.".to_owned(),
        ),
        (
            ConfigError::TooManyLines {
                lines: 128,
                size: 2048,
                line: 16,
            },
            "sizeBytes: 2048".to_owned(),
            format!(
                " makes 128 lines of 16 B. Keep the cache to at most {} lines so every set fits \
                 on screen.",
                grouped(MAX_LINES)
            ),
        ),
        (
            ConfigError::TooManyWords {
                words: 64,
                line: 256,
                word: 4,
            },
            "lineBytes: 256".to_owned(),
            format!(
                " holds 64 words of 4 B. A line can hold at most {} words.",
                grouped(MAX_WORDS_PER_LINE)
            ),
        ),
    ];
    for (error, field, rule) in cases {
        assert_eq!(error.segments(), [code(&field), plain(&rule)], "{error:?}");
    }
    // The rules are the ones validation finds.
    let word = Config {
        word_bytes: WordBytes::Eight,
        cache: CacheConfig {
            line_bytes: 4,
            ..config().cache
        },
        ..config()
    };
    assert_eq!(
        word.validate().unwrap_err().segments(),
        [
            code("lineBytes: 4"),
            plain(" is smaller than one 8 B word. A line must hold at least one word.")
        ]
    );
}

#[test]
fn each_rule_of_ways_tags_timing_and_seed_opens_with_its_field_in_mono() {
    let cases = [
        (
            ConfigError::AssociativityZero,
            "associativity: 0".to_owned(),
            " must be at least 1.".to_owned(),
        ),
        (
            ConfigError::AssociativityNotPowerOfTwo { got: 3, lines: 8 },
            "associativity: 3".to_owned(),
            " is not a power of two. It must be 1, 2, 4 or 8 (fully associative) for an 8-line \
             cache."
                .to_owned(),
        ),
        (
            ConfigError::AssociativityTooLarge { got: 16, lines: 8 },
            "associativity: 16".to_owned(),
            " is too many: an 8-line cache can have at most 8 ways.".to_owned(),
        ),
        (
            ConfigError::AssociativityTooLarge { got: 2, lines: 1 },
            "associativity: 2".to_owned(),
            " is too many: a 1-line cache can have at most 1 way.".to_owned(),
        ),
        (
            ConfigError::NoTagBits { bits: 8 },
            "addressBits: 8".to_owned(),
            " leaves no tag bits: one way of this cache would cover all of memory. Use wider \
             addresses or a smaller cache."
                .to_owned(),
        ),
        (
            ConfigError::Timing {
                field: ConfigNumberField::MissCycles,
                got: 1001,
            },
            "missCycles: 1001".to_owned(),
            format!(
                " must be between {} and {} cycles.",
                grouped(MIN_CYCLES),
                grouped(MAX_CYCLES)
            ),
        ),
        (
            ConfigError::Timing {
                field: ConfigNumberField::HitCycles,
                got: 0,
            },
            "hitCycles: 0".to_owned(),
            format!(
                " must be between {} and {} cycles.",
                grouped(MIN_CYCLES),
                grouped(MAX_CYCLES)
            ),
        ),
        (
            ConfigError::SeedTooLarge { got: MAX_SEED + 1 },
            format!("seed: {}", MAX_SEED + 1),
            format!(" must be at most {}.", grouped(MAX_SEED)),
        ),
    ];
    for (error, field, rule) in cases {
        assert_eq!(error.segments(), [code(&field), plain(&rule)], "{error:?}");
    }
}

#[test]
fn load_progress_serializes_with_its_status() {
    let progress = ShareLoad::from_share_string("!").advance(1);
    let value = serde_json::to_value(&progress).unwrap();
    assert_eq!(
        value,
        json!({
            "status": "failed",
            "failure": {
                "title": "Could not open link",
                "lead": LINK_LEAD,
                "message": [{
                    "text": "The link's text is not valid base64url; it may have been cut short \
                             or changed.",
                    "style": "plain"
                }]
            }
        })
    );
    let mut original = session();
    apply_all(&mut original, [load(0x0A4)]);
    let mut load = ShareLoad::from_share_string(&original.to_share_string().unwrap());
    let value = serde_json::to_value(load.advance(u32::MAX)).unwrap();
    assert_eq!(
        value,
        json!({
            "status": "done",
            "summary": {
                "configSummary": "Direct-mapped \u{b7} 128 B",
                "runs": 1,
                "opCount": 1,
                "stepCount": 5,
                "openingStep": 5,
                "text": "Direct-mapped \u{b7} 128 B \u{b7} 5 steps, opening at step 5.",
                "copiedText": "Link copied. It encodes the configuration and all 5 steps, and \
                               opens at step 5.",
                "ticks": [3, 2, 2, 2, 2]
            }
        })
    );
    let bad = with_ops(&[
        json!({"op": "issue", "kind": "read", "addr": 0x0A4}),
        json!({"op": "step"}),
    ]);
    let (failure, _) = failure_of(&link(&bad.to_string()));
    let value = serde_json::to_value(&failure).unwrap();
    assert_eq!(value["prefix"]["label"], "Open the first action");
    assert!(value.get("repair").is_none());
    assert!(value["contents"].is_string());
    let progress = ShareLoad::from_share_string(&pathological(1)).advance(1);
    let LoadProgress::Replaying { summary } = &progress else {
        panic!("{progress:?}");
    };
    assert_eq!(summary.step_count, 0);
    assert_eq!(
        serde_json::to_value(&progress).unwrap()["status"],
        "replaying"
    );
}
