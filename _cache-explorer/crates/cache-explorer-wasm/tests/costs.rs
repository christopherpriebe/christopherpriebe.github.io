//! What the heaviest logs a session can hold cost in wasm, built and read
//! through the facade as the page builds and reads them: the wasm memory the
//! module grows to, seeks at the session's limit, Space from there, a
//! search over the whole run, a recording replay, the texts Share and Save
//! write, and a link opened a slice at a time.
//!
//! The logs are the heaviest the native measurements found: a loop of hits
//! in a fully associative set of 64 ways, where every access moves every
//! rank, by program and by hand, loads and stores, timed and not; a cycle of
//! 65 lines that misses every time; walks that touch a new line with every
//! access; and a loop without accesses stepped over whole budgets, the ops of
//! the pathological link, whose own document is loaded too.
//!
//! Each log is built op by op, as the page applies ops, to the session's
//! limit, measured and dropped, then opened from its link in slices and from
//! its saved file in one call, on the same page. The logs of accesses issued
//! by hand are also opened from their link on a page of their own, without
//! being built first, and so is the pathological link's document.
//!
//! The measurements are ignored in a normal run: each builds a log of
//! `SESSION_EVENT_LIMIT` events and times the code, so they are run by hand
//! on a release build, one log to a page, since a module's memory never
//! shrinks:
//!
//! ```sh
//! cd web && WASM_BINDGEN_USE_BROWSER=1 WASM_BINDGEN_TEST_TIMEOUT=600 \
//!   npx wasm-pack test --headless --chrome --release ../crates/cache-explorer-wasm \
//!   --test costs -- --include-ignored --nocapture --exact logs::hit_loop
//! ```
//!
//! Each prints its figures and fails if the module's memory passes 64 MiB or
//! a seek, with the snapshot read after it, takes longer than a frame. The
//! test that is not ignored builds every log small and checks that it is the
//! log it is named for.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

/// The logs, and the measurements of each.
#[cfg(test)]
mod logs {
    use std::fmt::Write as _;

    use cache_explorer_core::limits::{
        CHECKPOINT_INTERVAL, MAX_EVENTS_PER_ENTRY, MAX_PROGRAM_INSTRUCTIONS, MAX_RUN_STEPS,
        MAX_SHARE_OPS,
    };
    use cache_explorer_core::{
        AllocatePolicy, CacheConfig, Config, Replacement, Timing, WordBytes, WritePolicy,
    };
    use cache_explorer_wasm::{Session, ShareLoad};
    use js_sys::{Array, ArrayBuffer, Function, Object, Reflect, JSON};
    use tsify::{Ts, Tsify};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_test::{console_log, wasm_bindgen_test};

    /// The most wasm memory a log may make the module hold.
    const MEMORY_BUDGET: f64 = 64.0 * 1024.0 * 1024.0;

    /// The longest a seek may take, with the snapshot read after it: a frame.
    const SEEK_BUDGET_MS: f64 = 16.0;

    /// A loop without an exit and without a memory access.
    const ENDLESS: &str = "add r1, r0, #1\nloop: bnez r1, loop";

    /// How a log is built.
    enum Plan {
        /// Programs loaded one after another, the `k`-th from `source(k)`,
        /// each run in `Run` ops until it stops.
        Programs(fn(u32) -> String),
        /// Accesses issued by hand to `addrs` in turn, stores when `store`.
        ByHand { store: bool, addrs: Vec<u32> },
        /// `ENDLESS` loaded, then executed to an access that never comes,
        /// again and again.
        Pairs,
    }

    /// What every access of a log does, which the small build checks.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Expect {
        /// The first pass over 64 lines misses; every later access hits
        /// the least recently used way and moves all 64 ranks.
        RankHeavyHits,
        /// Every access misses.
        Misses,
        /// Every access misses on a line never touched before.
        NewLines,
        /// There is no access.
        NoAccess,
    }

    /// A log to measure.
    struct Log {
        config: Config,
        plan: Plan,
        expect: Expect,
    }

    /// How far a log is built.
    #[derive(Clone, Copy)]
    struct Scale {
        /// The instructions each `Run` op executes.
        run_steps: u32,
        /// The events after which no op is added; the session's own limit
        /// stops it first at full scale.
        events: u32,
    }

    /// Full scale: up to the session's limit, in the longest `Run`s.
    const FULL: Scale = Scale {
        run_steps: MAX_RUN_STEPS,
        events: u32::MAX,
    };

    /// 8 KiB of 128 B lines, fully associative (64 ways), LRU, write-back,
    /// 8-byte words and 32-bit addresses: every rank moves when the least
    /// recently used way hits.
    fn wide(timed: bool) -> Config {
        Config {
            address_bits: 32,
            word_bytes: WordBytes::Eight,
            cache: CacheConfig {
                size_bytes: 8 * 1024,
                line_bytes: 128,
                associativity: 64,
                replacement: Replacement::Lru,
                write_policy: WritePolicy::WriteBack,
                allocate_policy: AllocatePolicy::WriteAllocate,
            },
            timing: timed.then_some(Timing {
                hit_cycles: 1,
                miss_cycles: 20,
            }),
            advanced: true,
            ..Config::default()
        }
    }

    /// A 1 KiB direct-mapped cache of 128 B lines, write-through and
    /// write-no-allocate, untimed, 8-byte words and 32-bit addresses: the
    /// walks' cache.
    fn walks() -> Config {
        Config {
            cache: CacheConfig {
                size_bytes: 1024,
                associativity: 1,
                write_policy: WritePolicy::WriteThrough,
                allocate_policy: AllocatePolicy::WriteNoAllocate,
                ..wide(false).cache
            },
            ..wide(false)
        }
    }

    /// `lines` lines `line_bytes` apart from 0, each addressed once in turn
    /// by `access` (`ld` or `st`), in a loop without an exit.
    fn cycle(access: &str, lines: u32, line_bytes: u32) -> String {
        let mut source = String::from("add r7, r0, #1\nloop:\n");
        for line in 0..lines {
            let addr = line * line_bytes;
            if access == "st" {
                writeln!(source, "st [0x{addr:X}], r7").unwrap();
            } else {
                writeln!(source, "ld r1, [0x{addr:X}]").unwrap();
            }
        }
        source.push_str("bnez r7, loop\n");
        source
    }

    /// The `k`-th program of a walk: from 1 MiB above the last program's
    /// start, `access` (`ld` or `st`) of a new 128 B line each time round.
    fn walk(access: &str, k: u32) -> String {
        let first = if access == "st" {
            "st [r2], r2"
        } else {
            "ld r1, [r2]"
        };
        format!(
            "add r2, r0, #0x{:X}\nloop: {first}\nadd r2, r2, #128\nbnez r2, loop",
            (k + 1) << 20
        )
    }

    /// The `k`-th program of a literal walk: as many loads as a program may
    /// hold, each of a new 128 B line, from 1 MiB above the last program's
    /// start.
    fn literal(k: u32) -> String {
        let mut source = String::new();
        for index in 0..MAX_PROGRAM_INSTRUCTIONS {
            writeln!(source, "ld r1, [0x{:X}]", ((k + 1) << 20) + index * 128).unwrap();
        }
        source
    }

    /// The log named `name`.
    fn log(name: &str) -> Log {
        let hits = |access: &'static str| {
            let program: fn(u32) -> String = match access {
                "st" => |_| cycle("st", 64, 128),
                _ => |_| cycle("ld", 64, 128),
            };
            Plan::Programs(program)
        };
        let lines =
            |count: u32, bytes: u32| -> Vec<u32> { (0..count).map(|line| line * bytes).collect() };
        let (config, plan, expect) = match name {
            "hit_loop" => (wide(false), hits("ld"), Expect::RankHeavyHits),
            "hit_loop_default_config" => (
                Config {
                    cache: CacheConfig {
                        size_bytes: 1024,
                        line_bytes: 16,
                        associativity: 64,
                        ..Config::default().cache
                    },
                    advanced: true,
                    ..Config::default()
                },
                Plan::Programs(|_| cycle("ld", 64, 16)),
                Expect::RankHeavyHits,
            ),
            "hit_loop_timed" => (wide(true), hits("ld"), Expect::RankHeavyHits),
            "hit_loop_stores" => (wide(false), hits("st"), Expect::RankHeavyHits),
            "hit_loop_stores_write_through_timed" => (
                Config {
                    cache: CacheConfig {
                        write_policy: WritePolicy::WriteThrough,
                        ..wide(true).cache
                    },
                    ..wide(true)
                },
                hits("st"),
                Expect::RankHeavyHits,
            ),
            "loads_by_hand" => (
                wide(false),
                Plan::ByHand {
                    store: false,
                    addrs: lines(64, 128),
                },
                Expect::RankHeavyHits,
            ),
            "timed_stores_by_hand" => (
                wide(true),
                Plan::ByHand {
                    store: true,
                    addrs: lines(64, 128),
                },
                Expect::RankHeavyHits,
            ),
            "literal_walk_64_ways" => (wide(false), Plan::Programs(literal), Expect::NewLines),
            "miss_cycle" => (
                wide(false),
                Plan::Programs(|_| cycle("ld", 65, 128)),
                Expect::Misses,
            ),
            "miss_cycle_fifo" => (
                Config {
                    cache: CacheConfig {
                        replacement: Replacement::Fifo,
                        ..wide(false).cache
                    },
                    ..wide(false)
                },
                Plan::Programs(|_| cycle("ld", 65, 128)),
                Expect::Misses,
            ),
            "miss_cycle_write_backs" => (
                wide(false),
                Plan::Programs(|_| cycle("st", 65, 128)),
                Expect::Misses,
            ),
            "miss_cycle_write_backs_timed" => (
                wide(true),
                Plan::Programs(|_| cycle("st", 65, 128)),
                Expect::Misses,
            ),
            "load_walk" => (walks(), Plan::Programs(|k| walk("ld", k)), Expect::NewLines),
            "store_walk" => (walks(), Plan::Programs(|k| walk("st", k)), Expect::NewLines),
            "literal_walk" => (walks(), Plan::Programs(literal), Expect::NewLines),
            "pathological_ops" => (Config::default(), Plan::Pairs, Expect::NoAccess),
            _ => panic!("no log is named {name}"),
        };
        Log {
            config,
            plan,
            expect,
        }
    }

    /// The name of every log built by ops, in the order they are measured.
    const LOGS: &[&str] = &[
        "hit_loop",
        "hit_loop_default_config",
        "hit_loop_timed",
        "hit_loop_stores",
        "hit_loop_stores_write_through_timed",
        "loads_by_hand",
        "timed_stores_by_hand",
        "literal_walk_64_ways",
        "miss_cycle",
        "miss_cycle_fifo",
        "miss_cycle_write_backs",
        "miss_cycle_write_backs_timed",
        "load_walk",
        "store_walk",
        "literal_walk",
        "pathological_ops",
    ];

    /// The JavaScript value `json` describes.
    fn js(json: &str) -> JsValue {
        JSON::parse(json).unwrap()
    }

    /// Property `key` of `value`.
    fn get(value: &JsValue, key: &str) -> JsValue {
        Reflect::get(value, &JsValue::from_str(key)).unwrap()
    }

    /// Property `key` of `value`, a count.
    fn count(value: &JsValue, key: &str) -> u32 {
        serde_wasm_bindgen::from_value(get(value, key)).unwrap()
    }

    /// The elements of the JavaScript array `value`.
    fn items(value: &JsValue) -> Vec<JsValue> {
        value.dyn_ref::<Array>().unwrap().iter().collect()
    }

    /// Milliseconds from the page's clock.
    fn now() -> f64 {
        let performance = get(&js_sys::global(), "performance");
        let now: Function = get(&performance, "now").dyn_into().unwrap();
        now.call0(&performance).unwrap().as_f64().unwrap()
    }

    /// `f`'s result, and how long it took in milliseconds.
    fn timed<T>(f: impl FnOnce() -> T) -> (T, f64) {
        let start = now();
        let result = f();
        (result, now() - start)
    }

    /// The bytes of the module's memory, which never shrinks: the most it
    /// has held so far.
    fn memory() -> f64 {
        let memory: js_sys::WebAssembly::Memory = wasm_bindgen::memory().dyn_into().unwrap();
        f64::from(
            memory
                .buffer()
                .unchecked_into::<ArrayBuffer>()
                .byte_length(),
        )
    }

    /// `bytes` in MiB.
    fn mib(bytes: f64) -> f64 {
        bytes / (1024.0 * 1024.0)
    }

    /// The events `session` holds up to its end, where every op leaves the
    /// cursor.
    fn end(session: &Session) -> u32 {
        count(&session.position().unwrap().into(), "pos")
    }

    /// Applies the op `op` to `session`; whether it was accepted.
    fn apply(session: &mut Session, op: &JsValue) -> bool {
        session.apply_op(&Ts::new_unchecked(op.clone())).is_ok()
    }

    /// The op that loads the program `source`.
    fn program(source: &str) -> JsValue {
        let op: JsValue = Object::new().into();
        Reflect::set(&op, &"op".into(), &"program".into()).unwrap();
        Reflect::set(&op, &"source".into(), &source.into()).unwrap();
        Reflect::set(&op, &"format".into(), &"asm".into()).unwrap();
        op
    }

    /// The op that issues a load of `addr`, or with `store` a store of 7.
    fn issue(store: bool, addr: u32) -> JsValue {
        if store {
            js(&format!(
                r#"{{"op":"issue","kind":"write","addr":{addr},"value":7}}"#
            ))
        } else {
            js(&format!(r#"{{"op":"issue","kind":"read","addr":{addr}}}"#))
        }
    }

    /// The saved file's JSON of a session on `config` whose log is `ops`.
    fn document(config: &Config, ops: &Array) -> String {
        let session = Session::with_config(&config.into_ts().unwrap()).unwrap();
        let doc = js(&session.to_json());
        Reflect::set(&doc, &"ops".into(), ops).unwrap();
        JSON::stringify(&doc).unwrap().as_string().unwrap()
    }

    /// A session on `log`'s configuration with its log built to `scale`.
    fn build(log: &Log, scale: Scale) -> Session {
        let mut session = Session::with_config(&log.config.into_ts().unwrap()).unwrap();
        let run = js(&format!(r#"{{"op":"run","n":{}}}"#, scale.run_steps));
        let room = |session: &Session| end(session) < scale.events;
        match &log.plan {
            Plan::Programs(source) => {
                let mut k = 0;
                'programs: while room(&session) && apply(&mut session, &program(&source(k))) {
                    k += 1;
                    // Until the program finishes or stops, or the session
                    // is full.
                    while apply(&mut session, &run) {
                        if !room(&session) {
                            break 'programs;
                        }
                    }
                }
            }
            Plan::ByHand { store, addrs } => {
                for addr in addrs.iter().cycle() {
                    if !room(&session) || !apply(&mut session, &issue(*store, *addr)) {
                        break;
                    }
                }
            }
            Plan::Pairs => {
                let step = js(r#"{"op":"stepAccess"}"#);
                while room(&session)
                    && apply(&mut session, &program(ENDLESS))
                    && apply(&mut session, &step)
                {}
            }
        }
        session
    }

    /// The figures of a seek to each position just before a checkpoint
    /// could be taken, each from position 0: the longest seek, the longest
    /// seek with the snapshot read after it, and the longest Space after
    /// that.
    fn seeks(session: &mut Session) -> (f64, f64, f64) {
        let last = end(session);
        let (mut seek, mut with_state, mut space) = (0.0_f64, 0.0_f64, 0.0_f64);
        for interval in 1..=last / CHECKPOINT_INTERVAL {
            let mark = interval * CHECKPOINT_INTERVAL;
            for pos in mark - 1..(mark + MAX_EVENTS_PER_ENTRY).min(last) {
                let pos = Ts::new_unchecked(JsValue::from(pos));
                session.seek(&Ts::new_unchecked(JsValue::from(0))).unwrap();
                let ((), took) = timed(|| session.seek(&pos).unwrap());
                seek = seek.max(took);
                session.seek(&Ts::new_unchecked(JsValue::from(0))).unwrap();
                let (_, took) = timed(|| {
                    session.seek(&pos).unwrap();
                    session.state().unwrap()
                });
                with_state = with_state.max(took);
                let (_, took) = timed(|| session.step_event(None).unwrap());
                space = space.max(took);
            }
        }
        (seek, with_state, space)
    }

    /// A link or a file opened a slice at a time.
    struct Opened {
        /// How many slices it took.
        slices: u32,
        /// The longest slice, in milliseconds.
        longest: f64,
        /// All the slices, in milliseconds.
        total: f64,
        /// The last progress reported: done, or failed.
        last: JsValue,
        /// The session, if it opened.
        session: Option<Session>,
    }

    /// Opens `share`, a link, or a saved file when `file`, a slice at a
    /// time, as the page does.
    fn load(share: &str, file: bool) -> Opened {
        let mut load = if file {
            ShareLoad::from_json(share)
        } else {
            ShareLoad::from_share_string(share)
        };
        let (mut slices, mut longest, mut total) = (0, 0.0_f64, 0.0);
        loop {
            let (progress, took) = timed(|| load.advance().unwrap());
            slices += 1;
            longest = longest.max(took);
            total += took;
            let last: JsValue = progress.into();
            if get(&last, "status").as_string().as_deref() != Some("replaying") {
                return Opened {
                    slices,
                    longest,
                    total,
                    last,
                    session: load.take_session().ok(),
                };
            }
        }
    }

    /// Builds the log `name` to the session's limit and measures it; fails
    /// if the module's memory or a seek passes its budget.
    fn measure(name: &str) {
        let log = log(name);
        let (mut session, build_ms) = timed(|| build(&log, FULL));
        let built = memory();
        let events = end(&session);
        let ops = session.op_count();
        let (_, state_ms) = timed(|| session.state().unwrap());
        let (json, json_ms) = timed(|| session.to_json());
        let (link, link_ms) = timed(|| session.to_share_string());
        let (seek_ms, seek_state_ms, space_ms) = seeks(&mut session);
        let never = Ts::new_unchecked(js(r#"{"counter":"accesses","op":"==","value":4294967295}"#));
        let (_, find_ms) = timed(|| session.find_event(&never).unwrap());
        drop(session);
        // A link too long to write is opened as its saved file.
        let link = link.ok();
        let (share, file) = match &link {
            Some(link) => (link.as_str(), false),
            None => (json.as_str(), true),
        };
        let opened = load(share, file);
        let (slices, slice_ms, load_ms) = (opened.slices, opened.longest, opened.total);
        assert_eq!(end(opened.session.as_ref().unwrap()), events);
        drop(opened);
        let after_load = memory();
        let (opened, replay_ms) = timed(|| Session::from_json(&json).unwrap());
        drop(opened);
        console_log!(
            "{name}: {events} events, {ops} ops, link {} B, file {} B | memory {:.1} MiB built, \
             {:.1} MiB after loading, {:.1} MiB after a replay | build {build_ms:.0} ms | seek \
             {seek_ms:.2} ms, with state() {seek_state_ms:.2} ms, Space {space_ms:.2} ms | \
             state() {state_ms:.2} ms, toJson {json_ms:.1} ms, toShareString {link_ms:.1} ms | \
             findEvent {find_ms:.1} ms | load {slices} slices, longest {slice_ms:.1} ms, \
             {load_ms:.0} ms in all | fromJson {replay_ms:.0} ms",
            if file { 0 } else { share.len() },
            json.len(),
            mib(built),
            mib(after_load),
            mib(memory()),
        );
        assert!(
            memory() <= MEMORY_BUDGET,
            "{name}: {:.1} MiB",
            mib(memory())
        );
        assert!(
            seek_state_ms <= SEEK_BUDGET_MS,
            "{name}: {seek_state_ms:.2} ms"
        );
    }

    #[wasm_bindgen_test]
    fn every_log_is_the_log_it_is_named_for() {
        let small = Scale {
            run_steps: 50,
            events: 1_500,
        };
        for name in LOGS {
            let log = log(name);
            let session = build(&log, small);
            let events = end(&session);
            assert!(events >= small.events, "{name}: {events} events");
            let stats: JsValue = session.stats().unwrap().into();
            let counters = get(&stats, "counters");
            let accesses = count(&counters, "accesses");
            let (hits, misses) = (count(&counters, "hits"), count(&counters, "misses"));
            match log.expect {
                Expect::RankHeavyHits => {
                    assert!(accesses > 64, "{name}");
                    assert_eq!(misses, 64, "{name}");
                    // The last access's Rank event lists every way's rank.
                    let tail: JsValue = session.events_range(events - 10, events).unwrap().into();
                    let widest = items(&tail)
                        .iter()
                        .filter(|event| get(event, "kind").as_string().as_deref() == Some("rank"))
                        .map(|event| items(&get(event, "changed")).len())
                        .max();
                    assert!(widest >= Some(64), "{name}: {widest:?}");
                }
                Expect::Misses => assert_eq!((hits, misses > 64), (0, true), "{name}"),
                Expect::NewLines => {
                    assert_eq!(count(&counters, "compulsory"), accesses, "{name}");
                    assert!(accesses > 64, "{name}");
                }
                Expect::NoAccess => assert_eq!(accesses, 0, "{name}"),
            }
        }
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn hit_loop() {
        measure("hit_loop");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn hit_loop_default_config() {
        measure("hit_loop_default_config");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn hit_loop_timed() {
        measure("hit_loop_timed");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn hit_loop_stores() {
        measure("hit_loop_stores");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn hit_loop_stores_write_through_timed() {
        measure("hit_loop_stores_write_through_timed");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn loads_by_hand() {
        measure("loads_by_hand");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn timed_stores_by_hand() {
        measure("timed_stores_by_hand");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn literal_walk_64_ways() {
        measure("literal_walk_64_ways");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn miss_cycle() {
        measure("miss_cycle");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn miss_cycle_fifo() {
        measure("miss_cycle_fifo");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn miss_cycle_write_backs() {
        measure("miss_cycle_write_backs");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn miss_cycle_write_backs_timed() {
        measure("miss_cycle_write_backs_timed");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn load_walk() {
        measure("load_walk");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn store_walk() {
        measure("store_walk");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn literal_walk() {
        measure("literal_walk");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn pathological_ops() {
        measure("pathological_ops");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn pathological_link() {
        // As many pairs as a document may hold: the session's limit stops
        // the load, which fails with the actions before it on offer.
        let pair = [program(ENDLESS), js(r#"{"op":"stepAccess"}"#)];
        let ops: Array = pair
            .iter()
            .cycle()
            .take(usize::try_from(MAX_SHARE_OPS).unwrap())
            .collect();
        let json = document(&Config::default(), &ops);
        let opened = load(&json, true);
        assert!(opened.session.is_none());
        console_log!(
            "pathological_link: {} pairs, file {} B | memory {:.1} MiB after loading | load \
             {} slices, longest {:.1} ms, {:.0} ms in all",
            MAX_SHARE_OPS / 2,
            json.len(),
            mib(memory()),
            opened.slices,
            opened.longest,
            opened.total,
        );
        assert!(memory() <= MEMORY_BUDGET, "{:.1} MiB", mib(memory()));
    }

    /// Opens, on a page of its own, the link of the log `name`, built by
    /// hand, without building it first: a document of as many of its
    /// accesses as a document may hold stops at the session's limit, and
    /// its failure offers the link of the actions before it, which is the
    /// log built by hand to the limit, and which opens.
    fn open_by_hand(name: &str) {
        let log = log(name);
        let Plan::ByHand { store, addrs } = &log.plan else {
            panic!("{name} is not built by hand");
        };
        let ops: Array = addrs
            .iter()
            .cycle()
            .take(usize::try_from(MAX_SHARE_OPS).unwrap())
            .map(|addr| issue(*store, *addr))
            .collect();
        let failed = load(&document(&log.config, &ops), true);
        assert!(failed.session.is_none());
        let failing = memory();
        let prefix = get(&get(&get(&failed.last, "failure"), "prefix"), "share");
        let link = prefix.as_string().unwrap();
        drop(failed);
        let opened = load(&link, false);
        let session = opened.session.as_ref().unwrap();
        console_log!(
            "{name}_link: {} events, {} ops, link {} B | memory {:.1} MiB after the document \
             stopped at the limit, {:.1} MiB after its prefix's link opened | load {} slices, \
             longest {:.1} ms, {:.0} ms in all",
            end(session),
            session.op_count(),
            link.len(),
            mib(failing),
            mib(memory()),
            opened.slices,
            opened.longest,
            opened.total,
        );
        assert!(
            memory() <= MEMORY_BUDGET,
            "{name}: {:.1} MiB",
            mib(memory())
        );
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn loads_by_hand_link() {
        open_by_hand("loads_by_hand");
    }

    #[wasm_bindgen_test]
    #[ignore = "a measurement at the session's limit; run by hand on a release build"]
    fn timed_stores_by_hand_link() {
        open_by_hand("timed_stores_by_hand");
    }
}
