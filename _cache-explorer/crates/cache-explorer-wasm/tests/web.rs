//! The facade's conversions and error mapping, run in wasm: values built in
//! JavaScript go in, values come out as JavaScript sees them, and every
//! failure is thrown as an `Error` whose message is the core's text, for
//! arguments of the wrong shape as for refusals. Arguments JavaScript could
//! send to break the module, a predicate nested thousands deep and a list
//! that is not one, are refused and leave it working.
//!
//! The suite has no `run_in_browser`, so it runs under Node as well as in a
//! browser. On a native target `wasm_bindgen_test` compiles each test as a
//! plain function that nothing runs, so `cargo test` passes over it.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

#[cfg(test)]
mod support {
    use js_sys::{Array, BigInt, Object, Reflect, JSON};
    use tsify::{Ts, Tsify};
    use wasm_bindgen::{JsCast, JsError, JsValue};

    /// The JavaScript value `json` describes.
    pub(crate) fn js(json: &str) -> JsValue {
        JSON::parse(json).unwrap()
    }

    /// The JavaScript value `json` describes, passed as a `T`.
    pub(crate) fn ts<T: Tsify>(json: &str) -> Ts<T> {
        Ts::new_unchecked(js(json))
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
}

#[cfg(test)]
mod web {
    use cache_explorer_core::limits::{
        MAX_ADDRESS_BITS, MAX_PREDICATE_DEPTH, MAX_SHARE_TEXT_BYTES,
    };
    use cache_explorer_core::{
        self as model, Config, EncodingError, LoadError, MemoryInit, OpError, WordBytes,
        SCHEMA_VERSION,
    };
    use cache_explorer_wasm::{
        build_info, eval_predicate, parse_module, validate_config, ModuleRun, Session, ShareLoad,
    };
    use js_sys::Array;
    use tsify::{Ts, Tsify};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{big, get, items, js, message, nest, text, ts, with};

    /// A configuration with 8-byte words, the widest data values, and
    /// memory holding its pattern, so that writing zero changes a word.
    fn eight_byte_words() -> Ts<Config> {
        Config {
            word_bytes: WordBytes::Eight,
            memory_init: MemoryInit::Pattern,
            ..Config::default()
        }
        .into_ts()
        .unwrap()
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

    #[wasm_bindgen_test]
    fn the_constructors_open_the_default_and_the_first_visit() {
        let session = Session::new().unwrap();
        let state: JsValue = session.state().unwrap().into();
        let config = get(&state, "config");
        assert_eq!(
            get(&config, "addressBits").as_f64(),
            Some(f64::from(Config::default().address_bits))
        );
        // Zero memory, the default, is left out, as `memoryInit?` says.
        assert!(get(&config, "memoryInit").is_undefined());
        let first: JsValue = Session::first_visit().unwrap().state().unwrap().into();
        assert_eq!(text(&get(&first, "config"), "memoryInit"), "pattern");
    }

    #[wasm_bindgen_test]
    fn an_invalid_configuration_throws_the_rule_it_breaks() {
        let config = Config {
            address_bits: MAX_ADDRESS_BITS + 1,
            ..Config::default()
        };
        let expected = model::Session::with_config(config.clone())
            .unwrap_err()
            .to_string();
        let error = Session::with_config(&config.into_ts().unwrap()).unwrap_err();
        assert_eq!(message(error), expected);
    }

    #[wasm_bindgen_test]
    fn an_argument_of_the_wrong_shape_names_its_type_and_passes_serdes_reason() {
        let mut session = Session::new().unwrap();
        let thrown = message(session.apply_op(&ts(r#"{"op":"stpe"}"#)).unwrap_err());
        assert!(
            thrown.starts_with("invalid Op: unknown variant `stpe`, expected one of `config`"),
            "{thrown}"
        );
        let thrown = message(Session::with_config(&ts(r#""wide""#)).unwrap_err());
        assert!(thrown.starts_with("invalid Config: "), "{thrown}");
        let thrown = message(session.set_memory_watch(js("[1.5]")).unwrap_err());
        assert!(thrown.starts_with("invalid LineAddr[]: "), "{thrown}");
        let thrown = message(session.seek(&ts("-1")).unwrap_err());
        assert!(thrown.starts_with("invalid EventPos: "), "{thrown}");
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn a_refused_op_throws_the_cores_message_and_changes_nothing() {
        let mut session = Session::new().unwrap();
        let before = session.to_json();
        let error = session.apply_op(&ts(r#"{"op":"step"}"#)).unwrap_err();
        assert_eq!(message(error), OpError::NoProgram.to_string());
        assert_eq!(session.to_json(), before);
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn words_cross_as_bigints_and_come_back_exact() {
        let session = Session::with_config(&eight_byte_words()).unwrap();
        let read: JsValue = session.read_word("18446744073709551615").unwrap().into();
        assert_eq!(text(&read, "status"), "valid");
        assert!(get(&read, "value").is_bigint());
        assert_eq!(get(&read, "value"), big(u64::MAX));
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
            assert_eq!(get(&get(&outcome, "patch"), "value"), big(expected));
            let consequences = items(&get(&outcome, "consequences"));
            let written = consequences
                .iter()
                .find(|consequence| text(consequence, "consequence") == "memoryWrite")
                .unwrap();
            assert!(get(written, "to").is_bigint());
            assert_eq!(get(written, "to"), big(expected));
        }
    }

    #[wasm_bindgen_test]
    fn a_word_above_the_safe_integers_is_refused_as_a_number() {
        let mut session = Session::with_config(&eight_byte_words()).unwrap();
        let op = with(
            js(r#"{"op":"issue","kind":"write","addr":168}"#),
            "value",
            &JsValue::from(2_f64.powi(53)),
        );
        let thrown = message(session.apply_op(&Ts::new_unchecked(op)).unwrap_err());
        assert!(thrown.starts_with("invalid Op: "), "{thrown}");
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn a_link_round_trips_and_a_broken_one_throws_the_load_message() {
        let mut session = Session::first_visit().unwrap();
        session
            .apply_op(&ts(r#"{"op":"issue","kind":"read","addr":164}"#))
            .unwrap();
        let share = session.to_share_string().unwrap();
        assert!(share.len() <= usize::try_from(MAX_SHARE_TEXT_BYTES).unwrap());
        let opened = Session::from_share_string(&share).unwrap();
        assert_eq!(opened.to_json(), session.to_json());
        let reopened = Session::from_json(&session.to_json()).unwrap();
        assert_eq!(reopened.cursor(), session.cursor());
        let error = Session::from_share_string("%%").unwrap_err();
        assert_eq!(
            message(error),
            LoadError::Encoding(EncodingError::Base64).to_string()
        );
    }

    #[wasm_bindgen_test]
    fn a_share_load_hands_over_its_session_once() {
        let mut session = Session::first_visit().unwrap();
        session
            .apply_op(&ts(r#"{"op":"issue","kind":"read","addr":164}"#))
            .unwrap();
        let mut load = ShareLoad::from_share_string(&session.to_share_string().unwrap());
        let early = load.take_session().unwrap_err();
        assert_eq!(message(early), LoadError::Unfinished.to_string());
        let progress: JsValue = load.advance().unwrap().into();
        assert_eq!(text(&progress, "status"), "done");
        let taken = load.take_session().unwrap();
        assert_eq!(taken.to_json(), session.to_json());
        let again = load.take_session().unwrap_err();
        assert_eq!(message(again), LoadError::Taken.to_string());
        let mut broken = ShareLoad::from_json("{");
        let failed: JsValue = broken.advance().unwrap().into();
        assert_eq!(text(&failed, "status"), "failed");
        assert!(broken.take_session().is_err());
    }

    #[wasm_bindgen_test]
    fn a_module_that_breaks_its_format_throws_where_and_how() {
        let expected = model::Module::parse("[]").unwrap_err().to_string();
        assert_eq!(message(parse_module("[]").unwrap_err()), expected);
        assert_eq!(message(ModuleRun::from_json("[]").unwrap_err()), expected);
    }

    #[wasm_bindgen_test]
    fn predicates_and_configurations_are_read_before_the_core_sees_them() {
        let session = Session::new().unwrap();
        let holds = eval_predicate(&session, &Ts::new_unchecked(no_accesses())).unwrap();
        assert!(holds);
        let thrown = message(eval_predicate(&session, &ts(r#"{"nothing":1}"#)).unwrap_err());
        assert!(thrown.starts_with("invalid Predicate: "), "{thrown}");
        let check: JsValue = validate_config(&Config::default().into_ts().unwrap())
            .unwrap()
            .into();
        assert_eq!(text(&check, "status"), "valid");
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
            assert_eq!(session.find_event(&deepest).unwrap(), Some(0), "{key}");
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
        session
            .apply_op(&ts(r#"{"op":"issue","kind":"read","addr":164}"#))
            .unwrap();
        assert_eq!(session.op_count(), 1);
        assert!(Session::from_json(&other.to_json()).is_ok());
    }

    #[wasm_bindgen_test]
    fn a_watch_list_is_read_whole_and_anything_else_throws_and_changes_nothing() {
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
    fn the_build_reports_its_version_and_share_format() {
        let info: JsValue = build_info().unwrap().into();
        assert_eq!(text(&info, "version"), env!("CARGO_PKG_VERSION"));
        assert_eq!(
            get(&info, "schemaVersion").as_f64(),
            Some(f64::from(SCHEMA_VERSION))
        );
    }
}
