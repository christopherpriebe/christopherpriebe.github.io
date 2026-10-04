//! The facade, run in wasm: every exported class, method and function, each
//! with values built in JavaScript going in and values coming out as
//! JavaScript sees them. Each output is compared whole with what the core
//! returns for the same call, serialized as the facade serializes it, so an
//! export that calls the wrong method, drops an argument or reads one wrongly
//! fails. Every failure is thrown as an `Error` whose message is the core's
//! `Display` text: arguments of the wrong shape, each named by its type, and
//! the refusals and failures of each area (configurations, ops, loads,
//! links, modules, addresses). Words cross as `bigint` both ways, counts and
//! cycles as numbers, and optional fields that hold nothing are absent.
//! Arguments JavaScript could send to break the module, a predicate nested
//! thousands deep and a list that is not one, are refused and leave it
//! working.
//!
//! The golden fixtures are replayed here too, in wasm, and their events
//! rendered as the canonical text must equal the bodies of the snapshots
//! the native tests commit, which proves that the two builds agree; applied
//! op by op through the facade, they give the same events.
//!
//! The suite has no `run_in_browser`, so it runs under Node as well as in a
//! browser. On a native target `wasm_bindgen_test` compiles each test as a
//! plain function that nothing runs, so `cargo test` passes over it.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::{self as model, Config, MicroEvents, Op, Preset, Timing};
    use cache_explorer_wasm::Session;
    use js_sys::{Array, BigInt, Function, Object, Reflect, JSON};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use tsify::{Ts, Tsify};
    use wasm_bindgen::{JsCast, JsError, JsValue};

    /// Frame 3b's first five accesses, as the core's golden fixtures issue
    /// them (the store to 0x0B0, the load of 0x0A4, the store to 0x1A4, the
    /// load of 0x0A8 and the load of 0x2A4 that evicts the dirty line 0x1A0),
    /// then a program loaded and its first instruction stepped: hits, misses,
    /// a write-back, a program.
    pub(crate) const FRAME_3B: &[&str] = &[
        r#"{"op":"issue","kind":"write","addr":176,"value":5}"#,
        r#"{"op":"issue","kind":"read","addr":164}"#,
        r#"{"op":"issue","kind":"write","addr":420,"value":7}"#,
        r#"{"op":"issue","kind":"read","addr":168}"#,
        r#"{"op":"issue","kind":"read","addr":676}"#,
        r#"{"op":"program","source":"ld r2, [0x0A8]\nst [0x0B4], r2\nadd r3, r2, #1\n","format":"asm"}"#,
        r#"{"op":"step"}"#,
    ];

    /// A load of 0x0A4, which misses on an empty cache.
    pub(crate) const LOAD_0X0A4: &str = r#"{"op":"issue","kind":"read","addr":164}"#;

    /// The JavaScript value `json` describes.
    pub(crate) fn js(json: &str) -> JsValue {
        JSON::parse(json).unwrap()
    }

    /// The JavaScript value `json` describes, passed as a `T`.
    pub(crate) fn ts<T: Tsify>(json: &str) -> Ts<T> {
        Ts::new_unchecked(js(json))
    }

    /// The JavaScript value `json` describes, read by the core as a `T` the
    /// way the facade reads its arguments.
    pub(crate) fn rust<T: DeserializeOwned>(json: &str) -> T {
        serde_wasm_bindgen::from_value(js(json)).unwrap()
    }

    /// `value` passed as a `T`, the way the page passes a value the core
    /// built.
    pub(crate) fn arg<T: Tsify + Serialize>(value: &T) -> Ts<T> {
        value.into_ts().unwrap()
    }

    /// `value`, read back as the `T` it serializes, the way the page would
    /// read a value the facade handed it.
    pub(crate) fn from_ts<T: Tsify + DeserializeOwned>(value: Ts<T>) -> T {
        serde_wasm_bindgen::from_value(value.into()).unwrap()
    }

    /// `object` with `key` set to `value`.
    pub(crate) fn with(object: JsValue, key: &str, value: &JsValue) -> JsValue {
        Reflect::set(&object, &JsValue::from_str(key), value).unwrap();
        object
    }

    /// Property `key` of `value`.
    pub(crate) fn get(value: &JsValue, key: &str) -> JsValue {
        Reflect::get(value, &JsValue::from_str(key)).unwrap()
    }

    /// Whether `value` has a property `key` at all, `undefined` or not.
    pub(crate) fn has(value: &JsValue, key: &str) -> bool {
        Reflect::has(value, &JsValue::from_str(key)).unwrap()
    }

    /// Property `key` of `value`, a string.
    pub(crate) fn text(value: &JsValue, key: &str) -> String {
        get(value, key).as_string().unwrap()
    }

    /// The elements of the JavaScript array `value`.
    pub(crate) fn items(value: &JsValue) -> Vec<JsValue> {
        value.dyn_ref::<Array>().unwrap().iter().collect()
    }

    /// `predicate` inside `depth` objects `{ [key]: … }`, or with `list`,
    /// `{ [key]: [ … ] }`.
    pub(crate) fn nest(predicate: JsValue, key: &str, list: bool, depth: u32) -> JsValue {
        (0..depth).fold(predicate, |inner, _| {
            let inner = if list {
                Array::of1(&inner).into()
            } else {
                inner
            };
            with(Object::new().into(), key, &inner)
        })
    }

    /// `value` as a JavaScript `bigint`.
    pub(crate) fn big(value: u64) -> JsValue {
        BigInt::from(value).into()
    }

    /// The message of the `Error` that `error` throws in JavaScript.
    pub(crate) fn message(error: JsError) -> String {
        let error: js_sys::Error = JsValue::from(error).dyn_into().unwrap();
        String::from(error.message())
    }

    /// `value` as JSON text, a `bigint` written as its digits and `n`, so
    /// that two values can be compared whole. A key whose value is
    /// `undefined` is left out, as `JSON.stringify` leaves it out.
    pub(crate) fn json_text(value: &JsValue) -> String {
        let replacer = Function::new_with_args(
            "key, value",
            "return typeof value === 'bigint' ? `${value}n` : value;",
        );
        JSON::stringify_with_replacer(value, &replacer)
            .unwrap()
            .as_string()
            .unwrap()
    }

    /// Checks that `actual`, a facade's result, is `expected`, the core's,
    /// as the facade serializes it; then `actual`, for further checks.
    #[track_caller]
    pub(crate) fn same<T: Tsify + Serialize>(actual: Ts<T>, expected: &T) -> JsValue {
        let actual: JsValue = actual.into();
        let expected: JsValue = arg(expected).into();
        assert_eq!(json_text(&actual), json_text(&expected));
        actual
    }

    /// `two-way-lru` in Advanced, timed at 1 cycle a hit and 20 a miss.
    pub(crate) fn timed_two_way() -> Config {
        Config {
            timing: Some(Timing {
                hit_cycles: 1,
                miss_cycles: 20,
            }),
            advanced: true,
            ..Preset::TwoWayLru.config()
        }
    }

    /// A session of the core and one of the facade on `config`, each with
    /// the ops `ops` describe applied; the events of each op are checked to
    /// be the same.
    pub(crate) fn twins(config: &Config, ops: &[&str]) -> (model::Session, Session) {
        let mut core = model::Session::with_config(*config).unwrap();
        let mut facade = Session::with_config(&arg(config)).unwrap();
        for op in ops {
            apply_both(&mut core, &mut facade, op);
        }
        (core, facade)
    }

    /// Applies the op `json` describes to both sessions and checks that the
    /// facade returns the core's events; then the facade's events.
    #[track_caller]
    pub(crate) fn apply_both(core: &mut model::Session, facade: &mut Session, op: &str) -> JsValue {
        let expected = core.apply(rust::<Op>(op)).unwrap().to_vec();
        same(facade.apply_op(&ts(op)).unwrap(), &MicroEvents(expected))
    }

    /// Applies `op` to both sessions, as the page applies an op the core
    /// built, and checks that the facade returns the core's events.
    #[track_caller]
    pub(crate) fn apply_op_both(core: &mut model::Session, facade: &mut Session, op: &Op) {
        let expected = core.apply(op.clone()).unwrap().to_vec();
        same(facade.apply_op(&arg(op)).unwrap(), &MicroEvents(expected));
    }
}

/// The golden fixtures in wasm: the core's events as the committed native
/// snapshots hold them, and the same events through the facade.
#[cfg(test)]
mod golden {
    use cache_explorer_core::golden;
    use cache_explorer_core::{MicroEvents, Op};
    use cache_explorer_wasm::Session;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{arg, same};

    /// The committed snapshot of each golden fixture, by name, in the order
    /// `golden::fixtures` lists them.
    macro_rules! snapshots {
        ($($name:literal,)*) => {
            &[$((
                $name,
                include_str!(concat!(
                    "../../cache-explorer-core/tests/snapshots/golden__",
                    $name,
                    ".snap"
                )),
            ),)*]
        };
    }

    const SNAPSHOTS: &[(&str, &str)] = snapshots![
        "flow-a",
        "flow-b-lru",
        "flow-b-fifo",
        "flow-b-random-seed-1",
        "frame-3b-accesses",
        "direct-mapped-conflict",
        "write-back-stores",
        "write-through-stores",
        "write-through-two-way",
        "write-no-allocate-write-back",
        "write-no-allocate-write-through",
        "no-allocate-store-then-load",
        "one-line-cache",
        "address-bits-8",
        "address-bits-12",
        "address-bits-16",
        "address-bits-32",
        "word-bytes-1",
        "word-bytes-2",
        "word-bytes-4",
        "word-bytes-8",
        "frame-3b-accesses-timed",
        "timed-write-through",
        "timed-write-no-allocate",
        "timed-one-cycle-misses",
        "frame-3b-program",
        "frame-3b-program-timed",
        "sample-trace",
        "count-down-loop",
        "register-address-fault",
        "budget-exhaustion",
        "store-truncation",
        "issue-beside-program",
        "edit-invalidate",
        "edit-way",
        "edit-memory-only",
        "edit-memory-clean",
        "edit-memory-dirty",
        "edit-memory-matches-dirty",
    ];

    /// The body of the insta snapshot `snap`: its text after the `---`
    /// header, without the newline insta ends it with.
    fn body(snap: &str) -> &str {
        let (_, body) = snap
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .unwrap();
        body.trim_end_matches('\n')
    }

    #[wasm_bindgen_test]
    fn every_golden_fixture_has_a_committed_snapshot() {
        let fixtures: Vec<&str> = golden::fixtures()
            .iter()
            .map(|fixture| fixture.name)
            .collect();
        let snapshots: Vec<&str> = SNAPSHOTS.iter().map(|(name, _)| *name).collect();
        assert_eq!(fixtures, snapshots);
    }

    #[wasm_bindgen_test]
    fn the_golden_fixtures_replay_in_wasm_as_the_native_snapshots_hold() {
        let mut differ = Vec::new();
        for (fixture, (_, snap)) in golden::fixtures().iter().zip(SNAPSHOTS) {
            let events = fixture.events().unwrap();
            let text = golden::render(&events);
            if text.trim_end_matches('\n') != body(snap) {
                differ.push(fixture.name);
            }
        }
        assert!(differ.is_empty(), "differ from their snapshots: {differ:?}");
    }

    #[wasm_bindgen_test]
    fn the_golden_fixtures_applied_through_the_facade_give_the_same_events() {
        for fixture in golden::fixtures() {
            let mut session = Session::with_config(&arg(&fixture.config)).unwrap();
            for issue in &fixture.issues {
                session.apply_op(&arg(&Op::from(issue))).unwrap();
            }
            same(
                session.events().unwrap(),
                &MicroEvents(fixture.events().unwrap()),
            );
        }
    }
}

/// `Session`: one test per method.
#[cfg(test)]
mod session {
    use cache_explorer_core::limits::{
        MAX_ADDRESS_BITS, MAX_PREDICATE_DEPTH, MAX_SHARE_TEXT_BYTES,
    };
    use cache_explorer_core::{
        self as model, AccessKind, Config, ConfigNumberField, EditOutcome, EditPatch, EditTarget,
        EntryViews, MemoryInit, MemoryLines, MicroEvents, Op, Predicate, ProgramFormat,
        ProgramSource, WayForm, WordBytes,
    };
    use cache_explorer_wasm::{eval_predicate, Session};
    use js_sys::{Array, JSON};
    use tsify::Ts;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{
        apply_both, apply_op_both, arg, big, from_ts, get, has, items, js, json_text, message,
        nest, rust, same, text, timed_two_way, ts, twins, with, FRAME_3B, LOAD_0X0A4,
    };

    /// A configuration with 8-byte words, the widest data values, and
    /// memory holding its pattern, so that writing zero changes a word.
    fn eight_byte_words() -> Config {
        Config {
            word_bytes: WordBytes::Eight,
            memory_init: MemoryInit::Pattern,
            ..Config::default()
        }
    }

    /// The counter predicate `accesses == 0`, true on an empty session.
    fn no_accesses() -> JsValue {
        js(r#"{"counter":"accesses","op":"==","value":0}"#)
    }

    /// The lines `session` is asked to watch, as its memory table marks them.
    fn watched(session: &Session) -> Vec<f64> {
        let rows: JsValue = session.memory_lines(0, 64).unwrap().into();
        items(&rows)
            .iter()
            .filter(|row| get(row, "watched").as_bool() == Some(true))
            .map(|row| get(row, "line").as_f64().unwrap())
            .collect()
    }

    /// The saved file's JSON of the first visit with `ops` in its log, and
    /// `schema` as its format version.
    fn document(ops: &str, schema: u32) -> String {
        let doc = js(&Session::first_visit().unwrap().to_json());
        let doc = with(doc, "ops", &js(ops));
        let doc = with(doc, "schema_version", &JsValue::from(schema));
        JSON::stringify(&doc).unwrap().as_string().unwrap()
    }

    /// Frame 3b's ops on `two-way-lru`, timed, in a session of the core and
    /// one of the facade, the cursor at the end of the log.
    fn frame_3b() -> (model::Session, Session) {
        twins(&timed_two_way(), FRAME_3B)
    }

    /// Checks that the facade's cursor is where the core's is.
    #[track_caller]
    fn same_place(core: &model::Session, facade: &Session) {
        same(facade.position().unwrap(), &core.position());
        assert_eq!(facade.cursor(), core.cursor());
        assert_eq!(facade.op_count(), core.op_count());
    }

    #[wasm_bindgen_test]
    fn the_constructor_opens_the_default_configuration() {
        let session = Session::new().unwrap();
        let core = model::Session::with_config(Config::default()).unwrap();
        let state = same(session.state().unwrap(), &core.state());
        let config = get(&state, "config");
        assert_eq!(
            get(&config, "addressBits").as_f64(),
            Some(f64::from(Config::default().address_bits))
        );
        // Zero memory, the default, and no timing are left out, as
        // `memoryInit?` and `timing?` say.
        assert!(!has(&config, "memoryInit"));
        assert!(!has(&config, "timing"));
        assert_eq!((session.op_count(), session.cursor()), (0, 0));
    }

    #[wasm_bindgen_test]
    fn first_visit_opens_the_first_visits_configuration() {
        let session = Session::first_visit().unwrap();
        let core = model::Session::with_config(Config::first_visit()).unwrap();
        let state = same(session.state().unwrap(), &core.state());
        assert_eq!(text(&get(&state, "config"), "memoryInit"), "pattern");
    }

    #[wasm_bindgen_test]
    fn with_config_opens_a_configuration_and_throws_the_rule_it_breaks() {
        let config = timed_two_way();
        let session = Session::with_config(&arg(&config)).unwrap();
        let state: JsValue = session.state().unwrap().into();
        let expected: JsValue = arg(&config).into();
        assert_eq!(json_text(&get(&state, "config")), json_text(&expected));
        // The seed, a u64, crosses as a number.
        assert!(get(&get(&state, "config"), "seed").as_f64().is_some());
        let invalid = Config {
            address_bits: MAX_ADDRESS_BITS + 1,
            ..Config::default()
        };
        let expected = model::Session::with_config(invalid)
            .unwrap_err()
            .to_string();
        let error = Session::with_config(&arg(&invalid)).unwrap_err();
        assert_eq!(message(error), expected);
        let thrown = message(Session::with_config(&ts("{}")).unwrap_err());
        assert_eq!(thrown, "invalid Config: missing field `addressBits`");
    }

    #[wasm_bindgen_test]
    fn from_share_string_opens_a_link_and_throws_why_one_cannot_be_opened() {
        let (core, facade) = frame_3b();
        let share = facade.to_share_string().unwrap();
        assert!(share.len() <= usize::try_from(MAX_SHARE_TEXT_BYTES).unwrap());
        let opened = Session::from_share_string(&share).unwrap();
        assert_eq!(opened.to_json(), core.to_json());
        same_place(&core, &opened);
        let too_long = "A".repeat(usize::try_from(MAX_SHARE_TEXT_BYTES).unwrap() + 1);
        // Not base64url, not deflate, and too long to be read at all.
        for broken in ["%%", "AAAA", too_long.as_str()] {
            let expected = model::Session::from_share_string(broken)
                .unwrap_err()
                .to_string();
            let error = Session::from_share_string(broken).unwrap_err();
            assert_eq!(
                message(error),
                expected,
                "{}",
                broken.chars().take(20).collect::<String>()
            );
        }
    }

    #[wasm_bindgen_test]
    fn from_json_opens_a_saved_file_and_throws_why_one_cannot_be_opened() {
        let (core, facade) = frame_3b();
        let opened = Session::from_json(&facade.to_json()).unwrap();
        assert_eq!(opened.to_json(), core.to_json());
        same_place(&core, &opened);
        let broken = [
            // Not JSON, a newer format, an op the session refuses.
            "{".to_owned(),
            document("[]", 99),
            document(r#"[{"op":"step"},{"op":"step"}]"#, 1),
        ];
        for broken in broken {
            let expected = model::Session::from_json(&broken).unwrap_err().to_string();
            let error = Session::from_json(&broken).unwrap_err();
            assert_eq!(message(error), expected, "{broken}");
        }
    }

    #[wasm_bindgen_test]
    fn apply_op_returns_the_ops_events_and_throws_why_it_is_refused() {
        // Every op's events are compared with the core's as it goes.
        let (mut core, mut facade) = frame_3b();
        let events = apply_both(&mut core, &mut facade, r#"{"op":"run","n":2}"#);
        let first = &items(&events)[0];
        // Cycles and ids are numbers in a timed run.
        assert!(get(first, "cycle").as_f64().is_some());
        assert!(get(first, "id").as_f64().is_some());
        // Untimed, no event has a cycle, and a decode names no way yet.
        let (_, mut untimed) = twins(&Config::first_visit(), &[]);
        let events: JsValue = untimed.apply_op(&ts(LOAD_0X0A4)).unwrap().into();
        let events = items(&events);
        assert!(events.iter().all(|event| !has(event, "cycle")));
        assert_eq!(text(&events[0], "kind"), "decode");
        assert!(!has(&events[0], "way"));
        let refused = [
            // Past the address width, a store without its value, a value
            // too wide for the word, a program that does not parse, and a
            // run of no instructions.
            r#"{"op":"issue","kind":"read","addr":4096}"#,
            r#"{"op":"issue","kind":"write","addr":164}"#,
            r#"{"op":"issue","kind":"write","addr":164,"value":4294967296}"#,
            r#"{"op":"program","source":"ld r1, [0x0A6]","format":"asm"}"#,
            r#"{"op":"run","n":0}"#,
        ];
        let before = facade.to_json();
        for op in refused {
            let expected = core.apply(rust::<Op>(op)).unwrap_err().to_string();
            let error = facade.apply_op(&ts(op)).unwrap_err();
            assert_eq!(message(error), expected, "{op}");
            assert_eq!(facade.to_json(), before, "{op}");
        }
        let (mut empty, _) = twins(&Config::first_visit(), &[]);
        let expected = empty.apply(Op::Step).unwrap_err().to_string();
        let error = untimed.apply_op(&ts(r#"{"op":"step"}"#)).unwrap_err();
        assert_eq!(message(error), expected);
        let thrown = message(facade.apply_op(&ts(r#"{"op":"stpe"}"#)).unwrap_err());
        assert!(
            thrown.starts_with("invalid Op: unknown variant `stpe`, expected one of `config`"),
            "{thrown}"
        );
        assert_eq!(facade.to_json(), before);
    }

    #[wasm_bindgen_test]
    fn undo_moves_back_an_undo_stop_until_there_is_none() {
        let (mut core, mut facade) = frame_3b();
        loop {
            let moved = facade.undo();
            assert_eq!(moved, core.undo());
            same_place(&core, &facade);
            if !moved {
                break;
            }
        }
        assert_eq!(facade.cursor(), 0);
    }

    #[wasm_bindgen_test]
    fn redo_moves_forward_an_undo_stop_until_there_is_none() {
        let (mut core, mut facade) = frame_3b();
        facade.scrub_to(0);
        core.scrub_to(0);
        loop {
            let moved = facade.redo();
            assert_eq!(moved, core.redo());
            same_place(&core, &facade);
            if !moved {
                break;
            }
        }
        assert_eq!(facade.cursor(), facade.op_count());
    }

    #[wasm_bindgen_test]
    fn scrub_to_moves_to_where_that_many_ops_are_done() {
        let (mut core, mut facade) = frame_3b();
        // Past the log's end, to its end; a u32, so -1 from JavaScript is
        // the largest.
        for ops in [2, 0, 5, u32::MAX] {
            facade.scrub_to(ops);
            core.scrub_to(ops);
            same_place(&core, &facade);
            same(facade.state().unwrap(), &core.state());
        }
        assert_eq!(facade.cursor(), facade.op_count());
    }

    #[wasm_bindgen_test]
    fn seek_moves_to_a_position_and_refuses_what_is_not_one() {
        let (mut core, mut facade) = frame_3b();
        for pos in ["12", "0", "1000000"] {
            facade.seek(&ts(pos)).unwrap();
            core.seek(rust(pos));
            same_place(&core, &facade);
            same(facade.state().unwrap(), &core.state());
        }
        // serde-wasm-bindgen reads every number as a float.
        let refused = [
            ("-1", "invalid type: floating point `-1.0`, expected u32"),
            ("1.5", "invalid type: floating point `1.5`, expected u32"),
            (r#""5""#, r#"invalid type: string "5", expected u32"#),
            ("null", "invalid type: unit value, expected u32"),
        ];
        for (pos, detail) in refused {
            let thrown = message(facade.seek(&ts(pos)).unwrap_err());
            assert_eq!(thrown, format!("invalid EventPos: {detail}"), "{pos}");
            same_place(&core, &facade);
        }
    }

    #[wasm_bindgen_test]
    fn step_event_moves_executes_or_says_why_not() {
        let (mut core, mut facade) = twins(&Config::first_visit(), &[]);
        // No program, and none in the drawer.
        same(facade.step_event(None).unwrap(), &core.step_event(None));
        let program = r#"{"source":"ld r2, [0x0A8]\nadd r3, r2, #1","format":"asm"}"#;
        let source: ProgramSource = rust(program);
        // The drawer's program is loaded, then its first instruction runs.
        let outcome = same(
            facade.step_event(Some(ts(program))).unwrap(),
            &core.step_event(Some(&source)),
        );
        assert_eq!(text(&outcome, "status"), "applied");
        facade.seek(&ts("1")).unwrap();
        core.seek(rust("1"));
        let outcome = same(
            facade.step_event(Some(ts(program))).unwrap(),
            &core.step_event(Some(&source)),
        );
        assert_eq!(text(&outcome, "status"), "moved");
        same_place(&core, &facade);
        let thrown = message(facade.step_event(Some(ts(r#"{"source":1}"#))).unwrap_err());
        assert_eq!(
            thrown,
            "invalid ProgramSource: invalid type: floating point `1.0`, expected a string"
        );
        same_place(&core, &facade);
    }

    #[wasm_bindgen_test]
    fn step_access_moves_to_an_ops_end_executes_or_says_why_not() {
        let (mut core, mut facade) = twins(&Config::first_visit(), &[]);
        same(facade.step_access(None).unwrap(), &core.step_access(None));
        let program =
            r#"{"source":"add r2, r0, #168\nld r1, [r2]\nld r3, [0x0A4]","format":"asm"}"#;
        let source: ProgramSource = rust(program);
        for _ in 0..3 {
            same(
                facade.step_access(Some(ts(program))).unwrap(),
                &core.step_access(Some(&source)),
            );
            same_place(&core, &facade);
        }
        facade.scrub_to(1);
        core.scrub_to(1);
        let outcome = same(facade.step_access(None).unwrap(), &core.step_access(None));
        assert_eq!(text(&outcome, "status"), "moved");
        let thrown = message(facade.step_access(Some(ts("[]"))).unwrap_err());
        assert_eq!(thrown, "invalid ProgramSource: missing field `source`");
    }

    #[wasm_bindgen_test]
    fn set_advanced_switches_the_mode_or_asks_first() {
        let (mut core, mut facade) = twins(&Config::first_visit(), &[LOAD_0X0A4]);
        same(facade.set_advanced(true).unwrap(), &core.set_advanced(true));
        let state = same(facade.state().unwrap(), &core.state());
        assert_eq!(get(&state, "advanced"), JsValue::TRUE);
        // A timed run needs Advanced. With an access in it, Basics asks
        // before it starts a new run.
        let timed = Config {
            timing: timed_two_way().timing,
            advanced: true,
            ..Config::first_visit()
        };
        apply_op_both(&mut core, &mut facade, &Op::Config { config: timed });
        apply_both(&mut core, &mut facade, LOAD_0X0A4);
        let change = same(
            facade.set_advanced(false).unwrap(),
            &core.set_advanced(false),
        );
        assert_eq!(text(&change, "status"), "needsReset");
        same(facade.state().unwrap(), &core.state());
        // With no entries, there is nothing to lose: the Basics run starts at
        // once.
        apply_op_both(&mut core, &mut facade, &Op::Config { config: timed });
        let change = same(
            facade.set_advanced(false).unwrap(),
            &core.set_advanced(false),
        );
        assert_eq!(text(&change, "status"), "applied");
        let state = same(facade.state().unwrap(), &core.state());
        assert_eq!(get(&state, "advanced"), JsValue::FALSE);
    }

    #[wasm_bindgen_test]
    fn propose_config_applies_at_once_or_asks_first_and_throws_the_rule_broken() {
        let (mut core, mut facade) = twins(&Config::first_visit(), &[]);
        let two_way = model::Preset::TwoWayLru.config();
        // On an empty run, at once; after an access, a question.
        let change = same(
            facade.propose_config(&arg(&two_way)).unwrap(),
            &core.propose_config(two_way).unwrap(),
        );
        assert_eq!(text(&change, "status"), "applied");
        apply_both(&mut core, &mut facade, LOAD_0X0A4);
        let back = Config::first_visit();
        let change = same(
            facade.propose_config(&arg(&back)).unwrap(),
            &core.propose_config(back).unwrap(),
        );
        assert_eq!(text(&change, "status"), "needsReset");
        same_place(&core, &facade);
        let invalid = Config {
            address_bits: MAX_ADDRESS_BITS + 1,
            ..Config::default()
        };
        let expected = core.propose_config(invalid).unwrap_err().to_string();
        let error = facade.propose_config(&arg(&invalid)).unwrap_err();
        assert_eq!(message(error), expected);
        let thrown = message(facade.propose_config(&ts("7")).unwrap_err());
        assert_eq!(
            thrown,
            "invalid Config: invalid type: floating point `7.0`, expected struct Config"
        );
        same_place(&core, &facade);
    }

    #[wasm_bindgen_test]
    fn config_controls_describe_the_bar_for_the_configuration_shown() {
        let (core, facade) = frame_3b();
        let shown = timed_two_way();
        same(
            facade.config_controls(&arg(&shown)).unwrap(),
            &core.config_controls(&shown),
        );
        // A configuration that breaks a rule is shown with its error.
        let invalid = Config {
            address_bits: MAX_ADDRESS_BITS + 1,
            ..Config::default()
        };
        let controls = same(
            facade.config_controls(&arg(&invalid)).unwrap(),
            &core.config_controls(&invalid),
        );
        assert!(has(&controls, "error"));
        let valid: JsValue = facade.config_controls(&arg(&shown)).unwrap().into();
        assert!(!has(&valid, "error"));
        let thrown = message(facade.config_controls(&ts("null")).unwrap_err());
        assert_eq!(
            thrown,
            "invalid Config: invalid type: unit value, expected struct Config"
        );
    }

    #[wasm_bindgen_test]
    fn read_config_number_reads_a_cycle_count_into_the_configuration_shown() {
        let (core, facade) = frame_3b();
        let shown = timed_two_way();
        for (field, typed) in [
            ("\"missCycles\"", "40"),
            ("\"hitCycles\"", "0"),
            ("\"hitCycles\"", ""),
        ] {
            same(
                facade
                    .read_config_number(&ts(field), typed, &arg(&shown))
                    .unwrap(),
                &core.read_config_number(rust::<ConfigNumberField>(field), typed, &shown),
            );
        }
        let thrown = message(
            facade
                .read_config_number(&ts("\"cycles\""), "4", &arg(&shown))
                .unwrap_err(),
        );
        assert_eq!(
            thrown,
            "invalid ConfigNumberField: unknown variant `cycles`, expected `hitCycles` or \
             `missCycles`"
        );
        let thrown = message(
            facade
                .read_config_number(&ts("\"hitCycles\""), "4", &ts("[]"))
                .unwrap_err(),
        );
        assert_eq!(thrown, "invalid Config: missing field `addressBits`");
    }

    #[wasm_bindgen_test]
    fn state_is_the_snapshot_at_the_cursor() {
        let (mut core, mut facade) = frame_3b();
        let state = same(facade.state().unwrap(), &core.state());
        assert!(has(&state, "event"));
        assert!(has(&state, "program"));
        // Mid-access, then at position 0, where there is no event and no
        // program yet.
        let middle = (core.events().len() / 2).to_string();
        for pos in [middle.as_str(), "0"] {
            facade.seek(&ts(pos)).unwrap();
            core.seek(rust(pos));
            same(facade.state().unwrap(), &core.state());
        }
        let start: JsValue = facade.state().unwrap().into();
        assert!(!has(&start, "event"));
        assert!(!has(&start, "program"));
    }

    #[wasm_bindgen_test]
    fn events_are_every_event_up_to_the_cursor() {
        let (mut core, mut facade) = frame_3b();
        same(
            facade.events().unwrap(),
            &MicroEvents(core.events().to_vec()),
        );
        facade.scrub_to(3);
        core.scrub_to(3);
        let events = same(
            facade.events().unwrap(),
            &MicroEvents(core.events().to_vec()),
        );
        assert_eq!(items(&events).len(), core.events().len());
        facade.scrub_to(0);
        let events: JsValue = facade.events().unwrap().into();
        assert!(items(&events).is_empty());
    }

    #[wasm_bindgen_test]
    fn events_range_is_a_window_of_the_log_the_undone_events_included() {
        let (mut core, mut facade) = frame_3b();
        facade.scrub_to(2);
        core.scrub_to(2);
        for (start, end) in [(3, 9), (0, u32::MAX), (9, 3), (1_000, 2_000)] {
            same(
                facade.events_range(start, end).unwrap(),
                &MicroEvents(core.events_range(start..end).to_vec()),
            );
        }
        let all: JsValue = facade.events_range(0, u32::MAX).unwrap().into();
        assert!(items(&all).len() > core.events().len());
    }

    #[wasm_bindgen_test]
    fn timeline_describes_the_displayed_run() {
        let (mut core, mut facade) = frame_3b();
        same(facade.timeline().unwrap(), &core.timeline());
        facade.seek(&ts("7")).unwrap();
        core.seek(rust("7"));
        let timeline = same(facade.timeline().unwrap(), &core.timeline());
        assert!(has(&timeline, "current"));
    }

    #[wasm_bindgen_test]
    fn entries_are_a_window_of_the_displayed_runs_entries() {
        let (core, facade) = frame_3b();
        for (first, count) in [(0, 3), (2, 100), (100, 5), (0, 0)] {
            same(
                facade.entries(first, count).unwrap(),
                &EntryViews(core.entries(first, count)),
            );
        }
    }

    #[wasm_bindgen_test]
    fn ticks_are_one_byte_per_step() {
        let (mut core, mut facade) = frame_3b();
        assert_eq!(facade.ticks(), core.ticks());
        facade.scrub_to(0);
        core.scrub_to(0);
        assert_eq!(facade.ticks(), core.ticks());
        assert!(!facade.ticks().is_empty());
    }

    #[wasm_bindgen_test]
    fn memory_lines_are_a_window_of_the_memory_table() {
        let (core, facade) = frame_3b();
        for (first, count) in [(0, 64), (1, 2), (64, 1)] {
            same(
                facade.memory_lines(first, count).unwrap(),
                &MemoryLines(core.memory_lines(first, count)),
            );
        }
    }

    #[wasm_bindgen_test]
    fn set_memory_watch_reads_the_list_whole_and_anything_else_throws_and_changes_nothing() {
        let (mut core, mut facade) = frame_3b();
        facade.set_memory_watch(js("[160, 176, 1024]")).unwrap();
        core.set_memory_watch(&rust::<Vec<model::LineAddr>>("[160, 176, 1024]"));
        same(
            facade.memory_lines(0, 64).unwrap(),
            &MemoryLines(core.memory_lines(0, 64)),
        );
        // The state names the row of the lowest watched line, which is the
        // first row here.
        let state = same(facade.state().unwrap(), &core.state());
        assert_eq!(get(&get(&state, "memory"), "watchRow").as_f64(), Some(0.0));
        let mut session = Session::new().unwrap();
        session.set_memory_watch(js("[160, 176]")).unwrap();
        assert_eq!(watched(&session), [160.0, 176.0]);
        // A sparse array longer than wasm's memory could hold as a list of
        // handles is read one element at a time, and the first is a hole.
        let sparse = Array::new();
        sparse.set_length(2_u32.pow(30) + 1);
        let values = [
            JsValue::from(5),
            JsValue::TRUE,
            big(7),
            js("{}"),
            JsValue::from_str(""),
            JsValue::NULL,
            JsValue::UNDEFINED,
            js("[1.5]"),
            js("[160, -1]"),
            sparse.into(),
        ];
        // Named by place: the sparse array is too long to print.
        for (place, value) in values.into_iter().enumerate() {
            let thrown = message(session.set_memory_watch(value).unwrap_err());
            assert!(
                thrown.starts_with("invalid LineAddr[]: "),
                "{place}: {thrown}"
            );
            assert_eq!(watched(&session), [160.0, 176.0], "{place}");
        }
        session.set_memory_watch(js("[]")).unwrap();
        assert!(watched(&session).is_empty());
    }

    #[wasm_bindgen_test]
    fn stats_are_the_stats_region_at_the_cursor() {
        let (mut core, mut facade) = twins(&Config::first_visit(), &[]);
        let stats = same(facade.stats().unwrap(), &core.stats());
        // No access yet: no hit rate, no AMAT, no recent outcomes.
        for key in ["hitRate", "amat", "measured", "recentText"] {
            assert!(!has(&stats, key), "{key}");
        }
        apply_both(&mut core, &mut facade, LOAD_0X0A4);
        let stats = same(facade.stats().unwrap(), &core.stats());
        assert!(has(&stats, "hitRate"));
        let (core, facade) = frame_3b();
        let stats = same(facade.stats().unwrap(), &core.stats());
        let cycles = get(&get(&stats, "counters"), "cycles");
        assert!(cycles.as_f64().unwrap() > 0.0);
        assert!(has(&stats, "amat"));
    }

    #[wasm_bindgen_test]
    fn decode_splits_an_address_and_throws_one_that_does_not_fit() {
        let (core, facade) = frame_3b();
        for addr in [0x0A4, 0, 0xFFC] {
            same(facade.decode(addr).unwrap(), &core.decode(addr).unwrap());
        }
        // Past the 12-bit width, and -1 from JavaScript.
        for addr in [0x1000, u32::MAX] {
            let expected = core.decode(addr).unwrap_err().to_string();
            assert_eq!(
                message(facade.decode(addr).unwrap_err()),
                expected,
                "{addr:#x}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn read_address_reads_typed_text_against_the_geometry() {
        let (core, facade) = frame_3b();
        let statuses = [
            ("", "empty"),
            ("0x0A4", "valid"),
            ("0x0A6", "misaligned"),
            ("0x1000", "invalid"),
            ("zz", "invalid"),
        ];
        for (typed, status) in statuses {
            let read = same(
                facade.read_address(typed).unwrap(),
                &core.read_address(typed),
            );
            assert_eq!(text(&read, "status"), status, "{typed}");
        }
    }

    #[wasm_bindgen_test]
    fn read_word_reads_a_value_as_a_bigint() {
        let (core, facade) = twins(&eight_byte_words(), &[]);
        let read = same(
            facade.read_word("18446744073709551615").unwrap(),
            &core.read_word("18446744073709551615"),
        );
        assert_eq!(text(&read, "status"), "valid");
        assert!(get(&read, "value").is_bigint());
        assert_eq!(get(&read, "value"), big(u64::MAX));
        for typed in ["", "0", "0x7", "-1", "18446744073709551616", "seven"] {
            same(facade.read_word(typed).unwrap(), &core.read_word(typed));
        }
    }

    #[wasm_bindgen_test]
    fn draft_issue_is_what_the_address_and_value_boxes_would_issue() {
        let (core, facade) = frame_3b();
        let drafts = [
            ("\"read\"", "0x0A4", ""),
            ("\"write\"", "0x0A4", "7"),
            ("\"write\"", "0x0A4", ""),
            ("\"read\"", "0x0A6", ""),
            ("\"read\"", "", ""),
        ];
        for (kind, addr, value) in drafts {
            same(
                facade.draft_issue(&ts(kind), addr, value).unwrap(),
                &core.draft_issue(rust::<AccessKind>(kind), addr, value),
            );
        }
        let read: JsValue = facade
            .draft_issue(&ts("\"read\""), "0x0A4", "")
            .unwrap()
            .into();
        assert!(!has(&read, "value"));
        let write: JsValue = facade
            .draft_issue(&ts("\"write\""), "0x0A4", "7")
            .unwrap()
            .into();
        assert_eq!(get(&get(&write, "op"), "value"), big(7));
        let thrown = message(
            facade
                .draft_issue(&ts("\"fetch\""), "0x0A4", "")
                .unwrap_err(),
        );
        assert_eq!(
            thrown,
            "invalid AccessKind: unknown variant `fetch`, expected `read` or `write`"
        );
    }

    #[wasm_bindgen_test]
    fn plan_edit_plans_an_edit_or_says_why_it_is_refused() {
        let (core, facade) = frame_3b();
        let edits = [
            (
                r#"{"kind":"way","set":2,"way":0}"#,
                r#"{"kind":"way","valid":false}"#,
            ),
            (
                r#"{"kind":"way","set":2,"way":1}"#,
                r#"{"kind":"way","dirty":true}"#,
            ),
            (
                r#"{"kind":"memory","addr":420}"#,
                r#"{"kind":"memory","value":9}"#,
            ),
            (
                r#"{"kind":"memory","addr":420}"#,
                r#"{"kind":"memory","value":7}"#,
            ),
        ];
        let mut outcomes = Vec::new();
        for (target, patch) in edits {
            let expected = EditOutcome::from(
                core.plan_edit(&rust::<EditTarget>(target), &rust::<EditPatch>(patch)),
            );
            let outcome = same(
                facade.plan_edit(&ts(target), &ts(patch)).unwrap(),
                &expected,
            );
            outcomes.push(text(&outcome, "outcome"));
        }
        // Memory already holds 7 at 0x1A4, so writing it changes nothing.
        assert_eq!(outcomes, ["plan", "plan", "plan", "refusal"]);
        let thrown = message(
            facade
                .plan_edit(&ts(r#"{"kind":"cell"}"#), &ts(r#"{"kind":"way"}"#))
                .unwrap_err(),
        );
        assert_eq!(
            thrown,
            "invalid EditTarget: unknown variant `cell`, expected `way` or `memory`"
        );
        let thrown = message(
            facade
                .plan_edit(
                    &ts(r#"{"kind":"memory","addr":420}"#),
                    &ts(r#"{"kind":"memory"}"#),
                )
                .unwrap_err(),
        );
        assert_eq!(
            thrown,
            "invalid EditPatch: missing field `value` for patch `memory`"
        );
    }

    #[wasm_bindgen_test]
    fn words_cross_as_bigints_and_come_back_exact() {
        let session = Session::with_config(&arg(&eight_byte_words())).unwrap();
        // A memory edit's plan gives back the word it was sent, as the
        // patch and as the value memory would then hold.
        let target = ts(r#"{"kind":"memory","addr":168}"#);
        let cases = [
            (big(0), 0),
            (big(7), 7),
            (big(u64::MAX), u64::MAX),
            (JsValue::from(7), 7),
        ];
        for (value, expected) in cases {
            let patch = with(js(r#"{"kind":"memory"}"#), "value", &value);
            let outcome: JsValue = session
                .plan_edit(&target, &Ts::new_unchecked(patch))
                .unwrap()
                .into();
            assert_eq!(text(&outcome, "outcome"), "plan", "{expected}");
            assert_eq!(
                get(&get(&outcome, "patch"), "value"),
                big(expected),
                "{expected}"
            );
            let consequences = items(&get(&outcome, "consequences"));
            let written = consequences
                .iter()
                .find(|consequence| text(consequence, "consequence") == "memoryWrite")
                .unwrap();
            assert!(get(written, "to").is_bigint(), "{expected}");
            assert_eq!(get(written, "to"), big(expected), "{expected}");
        }
    }

    #[wasm_bindgen_test]
    fn a_word_above_the_safe_integers_is_refused_as_a_number() {
        let mut session = Session::with_config(&arg(&eight_byte_words())).unwrap();
        let op = with(
            js(r#"{"op":"issue","kind":"write","addr":168}"#),
            "value",
            &JsValue::from(2_f64.powi(53)),
        );
        let thrown = message(session.apply_op(&Ts::new_unchecked(op)).unwrap_err());
        assert_eq!(
            thrown,
            "invalid Op: invalid type: floating point `9007199254740992.0`, expected u64"
        );
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn draft_edit_is_the_way_editors_draft_of_its_form() {
        let (core, facade) = frame_3b();
        let forms = [
            r#"{"valid":true,"lineText":"0x0A0","wordTexts":["1","","",""],"dirty":true}"#,
            r#"{"valid":false,"lineText":"","wordTexts":[],"dirty":false}"#,
            r#"{"valid":true,"lineText":"0x0A4","wordTexts":["","","",""],"dirty":false,"rank":1}"#,
        ];
        for form in forms {
            same(
                facade.draft_edit(&ts("2"), &ts("0"), &ts(form)).unwrap(),
                &core.draft_edit(rust("2"), rust("0"), &rust::<WayForm>(form)),
            );
        }
        let form = forms[1];
        let thrown = message(
            facade
                .draft_edit(&ts("\"2\""), &ts("0"), &ts(form))
                .unwrap_err(),
        );
        assert_eq!(
            thrown,
            r#"invalid SetIndex: invalid type: string "2", expected u32"#
        );
        let thrown = message(
            facade
                .draft_edit(&ts("2"), &ts("-1"), &ts(form))
                .unwrap_err(),
        );
        assert_eq!(
            thrown,
            "invalid WayIndex: invalid type: floating point `-1.0`, expected u32"
        );
        let thrown = message(
            facade
                .draft_edit(&ts("2"), &ts("0"), &ts("{}"))
                .unwrap_err(),
        );
        assert_eq!(thrown, "invalid WayForm: missing field `valid`");
    }

    #[wasm_bindgen_test]
    fn draft_memory_edit_is_the_memory_editors_draft_of_its_value() {
        let (core, facade) = frame_3b();
        for (addr, typed) in [("420", "9"), ("420", ""), ("420", "x"), ("4092", "1")] {
            same(
                facade.draft_memory_edit(&ts(addr), typed).unwrap(),
                &core.draft_memory_edit(rust(addr), typed),
            );
        }
        // A typed address goes through `readAddress`; an `Addr` is a number.
        let thrown = message(facade.draft_memory_edit(&ts("\"0x1A4\""), "9").unwrap_err());
        assert_eq!(
            thrown,
            r#"invalid Addr: invalid type: string "0x1A4", expected u32"#
        );
    }

    #[wasm_bindgen_test]
    fn parse_program_parses_a_program_or_a_trace_or_lists_its_errors() {
        let (core, facade) = frame_3b();
        let sources = [
            ("ld r1, [0x0A4]\nst [0x0A8], #7", "\"asm\""),
            ("lw r1, [0x0A4]\nld r8, [0x0A6]", "\"asm\""),
            ("R 0x0A4\nW 0x0A8 7", "\"trace\""),
            ("", "\"trace\""),
        ];
        for (source, format) in sources {
            same(
                facade.parse_program(source, &ts(format)).unwrap(),
                &core.parse_program(source, rust::<ProgramFormat>(format)),
            );
        }
        let thrown = message(facade.parse_program("R 0x0A4", &ts("\"csv\"")).unwrap_err());
        assert_eq!(
            thrown,
            "invalid ProgramFormat: unknown variant `csv`, expected `asm` or `trace`"
        );
    }

    /// The position `findEvent` gives for `predicate`, read back as a number
    /// the way the page reads it.
    #[track_caller]
    fn found_at(session: &Session, predicate: &Ts<Predicate>) -> Option<u32> {
        let found = session.find_event(predicate).unwrap();
        found.map(|pos| from_ts::<model::EventPos>(pos).get())
    }

    #[wasm_bindgen_test]
    fn find_event_finds_where_a_predicate_first_holds() {
        let (core, facade) = frame_3b();
        let predicates = [
            r#"{"counter":"misses","op":">=","value":2}"#,
            r#"{"event":{"kind":"writeBack"}}"#,
            r#"{"counter":"accesses","op":"==","value":1000}"#,
        ];
        for predicate in predicates {
            let found = found_at(&facade, &ts(predicate));
            let expected = core
                .find_event(&rust::<Predicate>(predicate))
                .map(model::EventPos::get);
            assert_eq!(found, expected, "{predicate}");
        }
        assert_eq!(found_at(&facade, &ts(predicates[2])), None);
        let thrown = message(facade.find_event(&ts(r#"{"nothing":1}"#)).unwrap_err());
        assert_eq!(
            thrown,
            "invalid Predicate: a predicate needs exactly one of `way`, `line`, `memory`, \
             `counter`, `register`, `all`, `any`, `not`, `event`, `lastAccess`, `config`; found \
             none"
        );
    }

    #[wasm_bindgen_test]
    fn the_position_find_event_gives_is_one_seek_takes() {
        let (mut core, mut facade) = frame_3b();
        let predicate = r#"{"event":{"kind":"writeBack"}}"#;
        let end = facade.cursor();
        let found = facade.find_event(&ts(predicate)).unwrap().unwrap();
        facade.seek(&found).unwrap();
        let expected = core.find_event(&rust::<Predicate>(predicate)).unwrap();
        core.seek(expected);
        assert_ne!(facade.cursor(), end);
        same_place(&core, &facade);
        same(facade.state().unwrap(), &core.state());
    }

    #[wasm_bindgen_test]
    fn a_predicate_nested_too_deep_throws_and_leaves_the_module_working() {
        let mut session = Session::new().unwrap();
        let other = Session::first_visit().unwrap();
        let expected = format!(
            "invalid Predicate: predicates nest at most {MAX_PREDICATE_DEPTH} deep in `all`, \
             `any` and `not`"
        );
        for (key, list) in [("not", false), ("all", true), ("any", true)] {
            // The deepest predicate the core reads is read and evaluated.
            let deepest = Ts::new_unchecked(nest(no_accesses(), key, list, MAX_PREDICATE_DEPTH));
            assert!(eval_predicate(&session, &deepest).unwrap(), "{key}");
            assert_eq!(found_at(&session, &deepest), Some(0), "{key}");
            // Far deeper than the stack could follow, it is refused before
            // it is read that deep, by both calls, and again.
            let deep = Ts::new_unchecked(nest(no_accesses(), key, list, 10_000));
            for _ in 0..2 {
                let thrown = message(eval_predicate(&session, &deep).unwrap_err());
                assert_eq!(thrown, expected, "{key}");
                let thrown = message(session.find_event(&deep).unwrap_err());
                assert_eq!(thrown, expected, "{key}");
            }
        }
        session.apply_op(&ts(LOAD_0X0A4)).unwrap();
        assert_eq!(session.op_count(), 1);
        assert!(Session::from_json(&other.to_json()).is_ok());
    }

    #[wasm_bindgen_test]
    fn share_summary_describes_the_run_for_share() {
        let (mut core, mut facade) = frame_3b();
        same(facade.share_summary().unwrap(), &core.share_summary());
        facade.scrub_to(2);
        core.scrub_to(2);
        same(facade.share_summary().unwrap(), &core.share_summary());
    }

    #[wasm_bindgen_test]
    fn to_share_string_writes_a_link_or_throws_when_none_could_open_the_run() {
        let (core, facade) = frame_3b();
        let share = facade.to_share_string().unwrap();
        assert_eq!(share, core.to_share_string().unwrap());
        // Programs of text deflate cannot shorten: letters and digits from
        // xorshift64, too many for a link but not for a file.
        let (mut core, mut facade) = twins(&Config::first_visit(), &[]);
        let mut state: u64 = 1;
        for _ in 0..7 {
            let mut source = String::from("add r1, r0, #1\\n");
            for _ in 0..150 {
                source.push_str("; ");
                for _ in 0..96 {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let pick = usize::try_from(state >> 58).unwrap() % 36;
                    source.push(char::from(b"abcdefghijklmnopqrstuvwxyz0123456789"[pick]));
                }
                source.push_str("\\n");
            }
            let op = format!(r#"{{"op":"program","source":"{source}","format":"asm"}}"#);
            apply_both(&mut core, &mut facade, &op);
        }
        let expected = core.to_share_string().unwrap_err().to_string();
        assert_eq!(message(facade.to_share_string().unwrap_err()), expected);
        // The saved file still holds the run.
        assert_eq!(
            Session::from_json(&facade.to_json()).unwrap().to_json(),
            core.to_json()
        );
    }

    #[wasm_bindgen_test]
    fn to_json_is_the_saved_file_which_opens_again() {
        let (mut core, mut facade) = frame_3b();
        facade.scrub_to(3);
        core.scrub_to(3);
        let json = facade.to_json();
        assert_eq!(json, core.to_json());
        let opened = Session::from_json(&json).unwrap();
        same_place(&core, &opened);
        assert_eq!(opened.to_json(), json);
    }

    #[wasm_bindgen_test]
    fn op_count_counts_the_ops_of_the_log_the_undone_ones_included() {
        let (mut core, mut facade) = frame_3b();
        let ops = u32::try_from(FRAME_3B.len()).unwrap();
        assert_eq!(facade.op_count(), ops);
        facade.undo();
        core.undo();
        assert_eq!(facade.op_count(), core.op_count());
        assert_eq!(facade.op_count(), ops);
        // A new op drops the undone ones.
        apply_both(&mut core, &mut facade, LOAD_0X0A4);
        assert_eq!(facade.op_count(), core.op_count());
    }

    #[wasm_bindgen_test]
    fn cursor_counts_the_ops_done() {
        let (mut core, mut facade) = frame_3b();
        for pos in ["0", "3", "13", "1000"] {
            facade.seek(&ts(pos)).unwrap();
            core.seek(rust(pos));
            assert_eq!(facade.cursor(), core.cursor(), "{pos}");
        }
    }

    #[wasm_bindgen_test]
    fn position_is_the_cursors_place_in_the_run() {
        let (mut core, mut facade) = frame_3b();
        let position = same(facade.position().unwrap(), &core.position());
        assert_eq!(text(&position, "boundary"), "logEnd");
        facade.seek(&ts("0")).unwrap();
        core.seek(rust("0"));
        let position = same(facade.position().unwrap(), &core.position());
        // No op is done at position 0.
        assert!(!has(&position, "op"));
        assert!(!has(&position, "entry"));
    }
}

/// `ShareLoad`: a link or a file opened a slice at a time.
#[cfg(test)]
mod share_load {
    use cache_explorer_core::limits::LOAD_SLICE_EVENTS;
    use cache_explorer_core::{self as model, Config, LoadError};
    use cache_explorer_wasm::ShareLoad;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{message, same, text, twins, FRAME_3B, LOAD_0X0A4};

    /// A loop without an exit and without a memory access, stepped to an
    /// access that never comes: one event an instruction for a whole
    /// budget, several slices of a load.
    const ENDLESS: &[&str] = &[
        r#"{"op":"program","source":"add r1, r0, #1\nloop: bnez r1, loop","format":"asm"}"#,
        r#"{"op":"stepAccess"}"#,
        r#"{"op":"issue","kind":"read","addr":164}"#,
    ];

    /// Advances both loads until they are done or have failed, checking
    /// that each slice reports the same progress; then the slices.
    fn advance_both(core: &mut model::ShareLoad, facade: &mut ShareLoad) -> Vec<JsValue> {
        let mut slices = Vec::new();
        loop {
            let progress = same(facade.advance().unwrap(), &core.advance(LOAD_SLICE_EVENTS));
            let replaying = text(&progress, "status") == "replaying";
            slices.push(progress);
            if !replaying {
                return slices;
            }
        }
    }

    #[wasm_bindgen_test]
    fn from_share_string_opens_a_link_or_reports_why_it_cannot() {
        let (core, facade) = twins(&super::support::timed_two_way(), FRAME_3B);
        let share = facade.to_share_string().unwrap();
        let mut load = ShareLoad::from_share_string(&share);
        let slices = advance_both(&mut model::ShareLoad::from_share_string(&share), &mut load);
        assert_eq!(text(slices.last().unwrap(), "status"), "done");
        assert_eq!(load.take_session().unwrap().to_json(), core.to_json());
        let mut broken = ShareLoad::from_share_string("%%");
        let slices = advance_both(&mut model::ShareLoad::from_share_string("%%"), &mut broken);
        assert_eq!(text(&slices[0], "status"), "failed");
    }

    #[wasm_bindgen_test]
    fn from_json_opens_a_saved_file_or_reports_why_it_cannot() {
        let (core, facade) = twins(&Config::first_visit(), &[LOAD_0X0A4]);
        let json = facade.to_json();
        let mut load = ShareLoad::from_json(&json);
        advance_both(&mut model::ShareLoad::from_json(&json), &mut load);
        assert_eq!(load.take_session().unwrap().to_json(), core.to_json());
        for broken in ["{", "[]", ""] {
            let mut load = ShareLoad::from_json(broken);
            let slices = advance_both(&mut model::ShareLoad::from_json(broken), &mut load);
            assert_eq!(text(&slices[0], "status"), "failed", "{broken}");
        }
    }

    #[wasm_bindgen_test]
    fn advance_replays_a_slice_at_a_time_and_then_answers_the_same() {
        let (core, facade) = twins(&Config::first_visit(), ENDLESS);
        let json = facade.to_json();
        let mut model_load = model::ShareLoad::from_json(&json);
        let mut load = ShareLoad::from_json(&json);
        let slices = advance_both(&mut model_load, &mut load);
        assert!(slices.len() > 2, "{} slices", slices.len());
        // Once done, every call answers the same.
        advance_both(&mut model_load, &mut load);
        assert_eq!(load.take_session().unwrap().to_json(), core.to_json());
    }

    #[wasm_bindgen_test]
    fn take_session_hands_over_the_session_once_or_throws_why_not() {
        let (_, facade) = twins(&Config::first_visit(), &[LOAD_0X0A4]);
        let mut load = ShareLoad::from_share_string(&facade.to_share_string().unwrap());
        let early = load.take_session().unwrap_err();
        assert_eq!(message(early), LoadError::Unfinished.to_string());
        let progress: JsValue = load.advance().unwrap().into();
        assert_eq!(text(&progress, "status"), "done");
        let taken = load.take_session().unwrap();
        assert_eq!(taken.to_json(), facade.to_json());
        let again = load.take_session().unwrap_err();
        assert_eq!(message(again), LoadError::Taken.to_string());
        // A load that failed throws its failure's error.
        let broken = r#"{"schema_version":1,"config":{},"ops":[]}"#;
        let mut core = model::ShareLoad::from_json(broken);
        let mut load = ShareLoad::from_json(broken);
        advance_both(&mut core, &mut load);
        let expected = core.take_session().unwrap_err().to_string();
        assert_eq!(message(load.take_session().unwrap_err()), expected);
    }
}

/// `ModuleRun`: a guided module on a session.
#[cfg(test)]
mod module_run {
    use cache_explorer_core::limits::MAX_SHARE_OPS;
    use cache_explorer_core::{
        self as model, EditOutcome, EditPatch, EditTarget, MicroEvents, Module,
    };
    use cache_explorer_wasm::{ModuleRun, Session};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{apply_both, get, has, items, message, rust, same, text, ts, LOAD_0X0A4};

    /// A module with every step kind: explain, instruct, observe, predict.
    const MISS_THEN_HIT: &str = include_str!(
        "../../cache-explorer-core/tests/fixtures/modules/01-fixture-miss-then-hit.json"
    );

    /// A load of an address no fixture step's predicate checks, so filling a
    /// running module's session with it does not complete a step.
    const LOAD_0X400: &str = r#"{"op":"issue","kind":"read","addr":1024}"#;

    /// A module with a program and a prediction whose answer applies a
    /// configuration.
    const SWITCH_CONFIG: &str = include_str!(
        "../../cache-explorer-core/tests/fixtures/modules/02-fixture-switch-config.json"
    );

    /// The module run and session of the core, and of the facade, for the
    /// module `json`, started on the first visit.
    fn started(json: &str) -> (model::ModuleRun, model::Session, ModuleRun, Session) {
        let mut core_run = model::ModuleRun::from_json(json).unwrap();
        let mut core = model::Session::with_config(model::Config::first_visit()).unwrap();
        let mut run = ModuleRun::from_json(json).unwrap();
        let mut session = Session::first_visit().unwrap();
        same(
            run.start(&mut session).unwrap(),
            &MicroEvents(core_run.start(&mut core).unwrap().to_vec()),
        );
        (core_run, core, run, session)
    }

    #[wasm_bindgen_test]
    fn from_json_reads_a_module_and_throws_where_it_breaks_its_format() {
        let run = ModuleRun::from_json(MISS_THEN_HIT).unwrap();
        same(
            run.module().unwrap(),
            &Module::parse(MISS_THEN_HIT).unwrap(),
        );
        for broken in ["[]", "{", r#"{"id":"01-x"}"#] {
            let expected = model::ModuleRun::from_json(broken).unwrap_err().to_string();
            assert_eq!(
                message(ModuleRun::from_json(broken).unwrap_err()),
                expected,
                "{broken}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn module_is_the_module_read_with_its_words_as_bigints() {
        let run = ModuleRun::from_json(MISS_THEN_HIT).unwrap();
        let module = same(
            run.module().unwrap(),
            &Module::parse(MISS_THEN_HIT).unwrap(),
        );
        // The fifth step expects r1 to equal 66, a word.
        let steps = items(&get(&module, "steps"));
        let checks = items(&get(&get(&steps[4], "expect"), "all"));
        let register = checks.iter().find(|check| has(check, "register")).unwrap();
        assert!(get(register, "equals").is_bigint());
        assert!(!has(&module, "program"));
        let other = ModuleRun::from_json(SWITCH_CONFIG).unwrap();
        let module: JsValue = other.module().unwrap().into();
        assert_eq!(text(&get(&module, "program"), "format"), "trace");
    }

    #[wasm_bindgen_test]
    fn start_applies_the_modules_opening_ops_and_returns_their_events() {
        let (core_run, core, run, session) = started(SWITCH_CONFIG);
        assert_eq!(session.to_json(), core.to_json());
        same(run.view(&session).unwrap(), &core_run.view(&core));
    }

    #[wasm_bindgen_test]
    fn view_is_the_module_panel_as_the_session_is() {
        let (core_run, core, run, session) = started(MISS_THEN_HIT);
        let view = same(run.view(&session).unwrap(), &core_run.view(&core));
        assert_eq!(text(&view, "kind"), "explain");
        assert!(!has(&view, "options"));
        assert!(!has(&view, "feedback"));
    }

    #[wasm_bindgen_test]
    fn sync_completes_a_step_once_its_condition_holds() {
        let (mut core_run, mut core, mut run, mut session) = started(MISS_THEN_HIT);
        same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        apply_both(&mut core, &mut session, LOAD_0X0A4);
        let view = same(run.sync(&mut session).unwrap(), &core_run.sync(&mut core));
        assert_eq!(text(&view, "kind"), "observe");
        same(session.state().unwrap(), &core.state());
    }

    #[wasm_bindgen_test]
    fn select_check_show_me_and_next_answer_a_prediction() {
        let (mut core_run, mut core, mut run, mut session) = started(MISS_THEN_HIT);
        same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        let view = same(run.sync(&mut session).unwrap(), &core_run.sync(&mut core));
        assert_eq!(text(&view, "kind"), "predict");
        // A prediction gates the session until it is answered.
        let state: JsValue = session.state().unwrap().into();
        assert!(has(&get(&state, "controls"), "seekLimit"));
        // An option past the last is ignored; a wrong answer, then Show me.
        for option in [7, 1] {
            run.select(option);
            core_run.select(option);
            same(run.view(&session).unwrap(), &core_run.view(&core));
        }
        let view = same(run.check(&mut session).unwrap(), &core_run.check(&mut core));
        assert_eq!(text(&get(&view, "feedback"), "tone"), "incorrect");
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        let view = same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        assert_eq!(text(&view, "kind"), "instruct");
        same(session.state().unwrap(), &core.state());
    }

    #[wasm_bindgen_test]
    fn check_answers_a_prediction_and_lifts_its_gate() {
        let (mut core_run, mut core, mut run, mut session) = started(SWITCH_CONFIG);
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        same(run.sync(&mut session).unwrap(), &core_run.sync(&mut core));
        run.select(0);
        core_run.select(0);
        let view = same(run.check(&mut session).unwrap(), &core_run.check(&mut core));
        assert_eq!(text(&get(&view, "feedback"), "tone"), "correct");
        // The answer's `then` applied its configuration.
        assert_eq!(session.to_json(), core.to_json());
        let state: JsValue = session.state().unwrap().into();
        assert!(!has(&get(&state, "controls"), "seekLimit"));
    }

    #[wasm_bindgen_test]
    fn show_me_does_an_instruct_steps_ops_for_the_student() {
        let (mut core_run, mut core, mut run, mut session) = started(SWITCH_CONFIG);
        let ops = session.op_count();
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        // The step's `run 4` is applied, and the step then completes.
        assert_eq!(session.op_count(), ops + 1);
        assert_eq!(session.to_json(), core.to_json());
        let view = same(run.sync(&mut session).unwrap(), &core_run.sync(&mut core));
        assert_eq!(text(&view, "kind"), "predict");
        let options = items(&get(&view, "options"));
        assert_eq!(options.len(), 2);
    }

    #[wasm_bindgen_test]
    fn show_me_refused_at_the_link_limit_carries_a_notice() {
        let (mut core_run, mut core, mut run, mut session) = started(MISS_THEN_HIT);
        same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        while core.op_count() < MAX_SHARE_OPS {
            apply_both(&mut core, &mut session, LOAD_0X400);
        }
        let ops = core.op_count();
        let view = same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        assert_eq!(session.op_count(), ops);
        assert_eq!(text(&view, "notice"), model::OpError::LinkFull.to_string());
    }

    #[wasm_bindgen_test]
    fn next_goes_on_from_an_explanation() {
        let (mut core_run, mut core, mut run, mut session) = started(MISS_THEN_HIT);
        let view = same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        assert_eq!(text(&view, "kind"), "instruct");
        assert!(has(&view, "hint"));
    }

    #[wasm_bindgen_test]
    fn exit_lifts_the_gate_and_leaves_the_workbench_as_it_is() {
        let (mut core_run, mut core, mut run, mut session) = started(MISS_THEN_HIT);
        same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        same(run.sync(&mut session).unwrap(), &core_run.sync(&mut core));
        let before = session.to_json();
        run.exit(&mut session);
        core_run.exit(&mut core);
        assert_eq!(session.to_json(), before);
        let state = same(session.state().unwrap(), &core.state());
        assert!(!has(&get(&state, "controls"), "seekLimit"));
    }

    #[wasm_bindgen_test]
    fn plan_edit_and_draft_edit_refuse_under_the_gate() {
        let (mut core_run, mut core, mut run, mut session) = started(MISS_THEN_HIT);
        same(run.next(&mut session).unwrap(), &core_run.next(&mut core));
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        same(
            run.show_me(&mut session).unwrap(),
            &core_run.show_me(&mut core),
        );
        same(run.sync(&mut session).unwrap(), &core_run.sync(&mut core));
        let state: JsValue = session.state().unwrap().into();
        assert!(has(&get(&state, "controls"), "seekLimit"));

        let target = r#"{"kind":"memory","addr":0}"#;
        let patch = r#"{"kind":"memory","value":0}"#;
        let expected = EditOutcome::from(
            core.plan_edit(&rust::<EditTarget>(target), &rust::<EditPatch>(patch)),
        );
        let outcome = same(
            session.plan_edit(&ts(target), &ts(patch)).unwrap(),
            &expected,
        );
        assert_eq!(text(&outcome, "outcome"), "refusal");
        assert_eq!(text(&outcome, "reason"), "gated");
        assert!(!has(&outcome, "suggestion"));

        let set = "0";
        let way = "0";
        let form = r#"{"valid":false,"lineText":"","wordTexts":[],"dirty":false}"#;
        let expected = core.draft_edit(rust(set), rust(way), &rust(form));
        let draft = same(
            session.draft_edit(&ts(set), &ts(way), &ts(form)).unwrap(),
            &expected,
        );
        let draft_outcome = get(&draft, "outcome");
        assert_eq!(text(&draft_outcome, "outcome"), "refusal");
        assert_eq!(text(&draft_outcome, "reason"), "gated");
    }
}

/// The free functions.
#[cfg(test)]
mod functions {
    use cache_explorer_core::limits::MAX_ADDRESS_BITS;
    use cache_explorer_core::{self as model, Config, Module, Predicate, SCHEMA_VERSION};
    use cache_explorer_wasm::{
        build_info, eval_predicate, parse_module, parse_module_index, run_file_too_large,
        trace_file_too_large, validate_config,
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{arg, get, message, rust, same, text, ts, twins, FRAME_3B};

    /// A module with every step kind.
    const MISS_THEN_HIT: &str = include_str!(
        "../../cache-explorer-core/tests/fixtures/modules/01-fixture-miss-then-hit.json"
    );

    /// A module index of two rows.
    const INDEX: &str = r#"[
        {"id":"01-fixture-miss-then-hit","title":"Fixture: miss then hit",
         "description":"A first access *misses*.","advanced":false},
        {"id":"02-fixture-switch-config","title":"Fixture: conflicts, then two ways",
         "description":"`0x000` and `0x080` conflict.","advanced":false}
    ]"#;

    #[wasm_bindgen_test]
    fn parse_module_reads_a_module_and_throws_where_it_breaks_its_format() {
        same(
            parse_module(MISS_THEN_HIT).unwrap(),
            &Module::parse(MISS_THEN_HIT).unwrap(),
        );
        let broken = MISS_THEN_HIT.replacen("\"answer\": 0", "\"answer\": 5", 1);
        for broken in ["[]", "{", broken.as_str()] {
            let expected = Module::parse(broken).unwrap_err().to_string();
            assert_eq!(
                message(parse_module(broken).unwrap_err()),
                expected,
                "{}",
                broken.chars().take(20).collect::<String>()
            );
        }
    }

    #[wasm_bindgen_test]
    fn parse_module_index_reads_the_picker_and_throws_where_it_breaks_its_format() {
        let index = same(
            parse_module_index(INDEX).unwrap(),
            &model::parse_module_index(INDEX).unwrap(),
        );
        assert_eq!(
            text(&index, "summary"),
            model::parse_module_index(INDEX).unwrap().summary
        );
        let duplicate = INDEX.replace("02-fixture-switch-config", "01-fixture-miss-then-hit");
        for broken in ["{}", duplicate.as_str()] {
            let expected = model::parse_module_index(broken).unwrap_err().to_string();
            assert_eq!(
                message(parse_module_index(broken).unwrap_err()),
                expected,
                "{broken}"
            );
        }
    }

    #[wasm_bindgen_test]
    fn eval_predicate_says_whether_a_predicate_holds_at_the_cursor() {
        let (core, facade) = twins(&super::support::timed_two_way(), FRAME_3B);
        let predicates = [
            r#"{"counter":"accesses","op":">=","value":5}"#,
            r#"{"register":"r2","equals":66}"#,
            r#"{"memory":{"addr":"0x1A4"},"equals":7}"#,
            r#"{"not":{"line":{"addr":"0x1A0"}}}"#,
            r#"{"config":{"timed":false}}"#,
        ];
        for predicate in predicates {
            let holds = eval_predicate(&facade, &ts(predicate)).unwrap();
            assert_eq!(
                holds,
                core.eval_predicate(&rust::<Predicate>(predicate)),
                "{predicate}"
            );
        }
        let thrown = message(eval_predicate(&facade, &ts(r#"{"counter":"acc"}"#)).unwrap_err());
        assert!(
            thrown.starts_with(
                "invalid Predicate: unknown variant `acc`, expected one of `accesses`, `loads`"
            ),
            "{thrown}"
        );
    }

    #[wasm_bindgen_test]
    fn validate_config_checks_a_configuration_on_its_own() {
        let valid = super::support::timed_two_way();
        let check = same(
            validate_config(&arg(&valid)).unwrap(),
            &model::validate_config(&valid),
        );
        assert_eq!(text(&check, "status"), "valid");
        let invalid = Config {
            address_bits: MAX_ADDRESS_BITS + 1,
            ..Config::default()
        };
        let check = same(
            validate_config(&arg(&invalid)).unwrap(),
            &model::validate_config(&invalid),
        );
        assert_eq!(text(&check, "status"), "invalid");
        let thrown = message(validate_config(&ts("true")).unwrap_err());
        assert_eq!(
            thrown,
            "invalid Config: invalid type: boolean `true`, expected struct Config"
        );
    }

    #[wasm_bindgen_test]
    fn build_info_reports_the_version_and_share_format() {
        let info = same(build_info().unwrap(), &model::build_info());
        assert_eq!(text(&info, "version"), env!("CARGO_PKG_VERSION"));
        assert_eq!(
            get(&info, "schemaVersion").as_f64(),
            Some(f64::from(SCHEMA_VERSION))
        );
    }

    #[wasm_bindgen_test]
    fn a_file_too_large_to_open_is_refused_in_the_cores_words() {
        assert_eq!(run_file_too_large(1_048_576), None);
        assert_eq!(
            run_file_too_large(1_048_577),
            model::run_file_too_large(1_048_577)
        );
        assert!(run_file_too_large(1_048_577).is_some());
        assert_eq!(trace_file_too_large(16_384), None);
        assert_eq!(
            trace_file_too_large(16_385),
            model::trace_file_too_large(16_385)
        );
        assert!(trace_file_too_large(16_385).is_some());
    }
}
