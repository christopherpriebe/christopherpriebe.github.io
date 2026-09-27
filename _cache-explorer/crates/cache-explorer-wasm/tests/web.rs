//! The facade's conversions and error mapping, run in wasm: values built in
//! JavaScript go in, values come out as JavaScript sees them, and every
//! failure is thrown as an `Error` whose message is the core's text, for
//! arguments of the wrong shape as for refusals.
//!
//! The suite has no `run_in_browser`, so it runs under Node as well as in a
//! browser. On a native target `wasm_bindgen_test` compiles each test as a
//! plain function that nothing runs, so `cargo test` passes over it.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

#[cfg(test)]
mod support {
    use js_sys::{BigInt, Reflect, JSON};
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
    use cache_explorer_core::limits::{MAX_ADDRESS_BITS, MAX_SHARE_TEXT_BYTES};
    use cache_explorer_core::{
        self as model, Config, EncodingError, LoadError, OpError, WordBytes, SCHEMA_VERSION,
    };
    use cache_explorer_wasm::{
        build_info, eval_predicate, parse_module, validate_config, ModuleRun, Session, ShareLoad,
    };
    use tsify::{Ts, Tsify};
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::support::{big, get, js, message, text, ts, with};

    /// A configuration with 8-byte words, the widest data values.
    fn eight_byte_words() -> Ts<Config> {
        Config {
            word_bytes: WordBytes::Eight,
            ..Config::default()
        }
        .into_ts()
        .unwrap()
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
        let error = Session::with_config(config.into_ts().unwrap()).unwrap_err();
        assert_eq!(message(error), expected);
    }

    #[wasm_bindgen_test]
    fn an_argument_of_the_wrong_shape_names_its_type_and_passes_serdes_reason() {
        let mut session = Session::new().unwrap();
        let thrown = message(session.apply_op(ts(r#"{"op":"stpe"}"#)).unwrap_err());
        assert!(
            thrown.starts_with("invalid Op: unknown variant `stpe`, expected one of `config`"),
            "{thrown}"
        );
        let thrown = message(Session::with_config(ts(r#""wide""#)).unwrap_err());
        assert!(thrown.starts_with("invalid Config: "), "{thrown}");
        let thrown = message(session.set_memory_watch(vec![ts("1.5")]).unwrap_err());
        assert!(thrown.starts_with("invalid LineAddr: "), "{thrown}");
        let thrown = message(session.seek(ts("-1")).unwrap_err());
        assert!(thrown.starts_with("invalid EventPos: "), "{thrown}");
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn a_refused_op_throws_the_cores_message_and_changes_nothing() {
        let mut session = Session::new().unwrap();
        let before = session.to_json();
        let error = session.apply_op(ts(r#"{"op":"step"}"#)).unwrap_err();
        assert_eq!(message(error), OpError::NoProgram.to_string());
        assert_eq!(session.to_json(), before);
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn words_cross_as_bigints_and_come_back_exact() {
        let mut session = Session::with_config(eight_byte_words()).unwrap();
        let read: JsValue = session.read_word("18446744073709551615").unwrap().into();
        assert_eq!(text(&read, "status"), "valid");
        assert!(get(&read, "value").is_bigint());
        assert_eq!(get(&read, "value"), big(u64::MAX));
        for value in [big(0), big(7), big(u64::MAX), JsValue::from(7)] {
            let op = with(
                js(r#"{"op":"issue","kind":"write","addr":168}"#),
                "value",
                &value,
            );
            session.apply_op(Ts::new_unchecked(op)).unwrap();
        }
        let zero: JsValue = session.read_word("0").unwrap().into();
        assert_eq!(get(&zero, "value"), big(0));
        let seven: JsValue = session.read_word("7").unwrap().into();
        assert_eq!(get(&seven, "value"), big(7));
    }

    #[wasm_bindgen_test]
    fn a_word_above_the_safe_integers_is_refused_as_a_number() {
        let mut session = Session::with_config(eight_byte_words()).unwrap();
        let op = with(
            js(r#"{"op":"issue","kind":"write","addr":168}"#),
            "value",
            &JsValue::from(2_f64.powi(53)),
        );
        let thrown = message(session.apply_op(Ts::new_unchecked(op)).unwrap_err());
        assert!(thrown.starts_with("invalid Op: "), "{thrown}");
        assert_eq!(session.op_count(), 0);
    }

    #[wasm_bindgen_test]
    fn a_link_round_trips_and_a_broken_one_throws_the_load_message() {
        let mut session = Session::first_visit().unwrap();
        session
            .apply_op(ts(r#"{"op":"issue","kind":"read","addr":164}"#))
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
            .apply_op(ts(r#"{"op":"issue","kind":"read","addr":164}"#))
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
        let holds = eval_predicate(
            &session,
            ts(r#"{"counter":"accesses","op":"==","value":0}"#),
        )
        .unwrap();
        assert!(holds);
        let thrown = message(eval_predicate(&session, ts(r#"{"nothing":1}"#)).unwrap_err());
        assert!(thrown.starts_with("invalid Predicate: "), "{thrown}");
        let check: JsValue = validate_config(Config::default().into_ts().unwrap())
            .unwrap()
            .into();
        assert_eq!(text(&check, "status"), "valid");
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
