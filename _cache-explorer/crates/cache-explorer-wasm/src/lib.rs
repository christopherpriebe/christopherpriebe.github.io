//! The WebAssembly facade of the Interactive Cache Explorer.
//!
//! This crate marshals values between JavaScript and `cache_explorer_core` and
//! does nothing else: each export is a call into the core plus the conversion
//! of its arguments and result, and every `Err` becomes a `JsError` carrying
//! the core's message. The machine model, its wording and its number
//! formatting all live in the core.
//!
//! Values cross as `tsify::Ts<T>`: serde-wasm-bindgen converts them, and the
//! TypeScript declarations come from the core's types. An argument that does
//! not have its type's shape throws the core's [`ArgumentError`] message. A
//! JavaScript object holds one core object: [`Session`], [`ShareLoad`] or
//! [`ModuleRun`].

#![forbid(unsafe_code)]

use cache_explorer_core::{
    self as model, AccessKind, Addr, AddressInput, ArgKind, ArgumentError, BuildInfo, Config,
    ConfigChange, ConfigCheck, ConfigControls, ConfigNumberField, ConfigNumberInput, Decoded,
    EditDraft, EditOutcome, EditPatch, EditTarget, EntryViews, EventPos, IssueDraft, LineAddr,
    LoadProgress, MemoryEditDraft, MemoryLines, MicroEvent, MicroEvents, Module, ModuleIndexView,
    ModuleView, Op, ParseResult, Position, Predicate, ProgramFormat, ProgramSource, SetIndex,
    ShareSummary, State, Stats, StepOutcome, Timeline, WayForm, WayIndex, WordInput,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Prints a panic's message to the console before the module traps, in debug
/// builds only. A release build aborts with `RuntimeError: unreachable`.
#[cfg(debug_assertions)]
#[wasm_bindgen(start)]
fn install_panic_hook() {
    console_error_panic_hook::set_once();
}

/// A type JavaScript passes as an argument, with the kind an error names when
/// a value is not one.
trait Argument: DeserializeOwned {
    /// The kind the error of a value that is not this type names.
    const KIND: ArgKind;
}

impl Argument for AccessKind {
    const KIND: ArgKind = ArgKind::AccessKind;
}

impl Argument for Addr {
    const KIND: ArgKind = ArgKind::Addr;
}

impl Argument for Config {
    const KIND: ArgKind = ArgKind::Config;
}

impl Argument for ConfigNumberField {
    const KIND: ArgKind = ArgKind::ConfigNumberField;
}

impl Argument for EditPatch {
    const KIND: ArgKind = ArgKind::EditPatch;
}

impl Argument for EditTarget {
    const KIND: ArgKind = ArgKind::EditTarget;
}

impl Argument for EventPos {
    const KIND: ArgKind = ArgKind::EventPos;
}

impl Argument for Vec<LineAddr> {
    const KIND: ArgKind = ArgKind::LineAddrs;
}

impl Argument for Op {
    const KIND: ArgKind = ArgKind::Op;
}

impl Argument for Predicate {
    const KIND: ArgKind = ArgKind::Predicate;
}

impl Argument for ProgramFormat {
    const KIND: ArgKind = ArgKind::ProgramFormat;
}

impl Argument for ProgramSource {
    const KIND: ArgKind = ArgKind::ProgramSource;
}

impl Argument for SetIndex {
    const KIND: ArgKind = ArgKind::SetIndex;
}

impl Argument for WayForm {
    const KIND: ArgKind = ArgKind::WayForm;
}

impl Argument for WayIndex {
    const KIND: ArgKind = ArgKind::WayIndex;
}

/// `value` read as the `T` it was passed as.
///
/// # Errors
///
/// The core's [`ArgumentError`] for `T`'s kind, with serde-wasm-bindgen's
/// reason, when `value` does not have `T`'s shape.
fn arg<T>(value: &Ts<T>) -> Result<T, JsError>
where
    T: Argument + Tsify,
{
    read(value.js_value())
}

/// `value` read as a `T`, for an argument passed as a plain `JsValue`
/// because tsify has no type for it, such as a list.
///
/// # Errors
///
/// As [`arg`].
fn read<T: Argument>(value: JsValue) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(value).map_err(|error| {
        let detail = JsValue::from(error)
            .dyn_into::<js_sys::Error>()
            .map(|error| String::from(error.message()))
            .unwrap_or_default();
        JsError::new(&ArgumentError::new(T::KIND, detail).to_string())
    })
}

/// Serializes `events` to the JavaScript value a [`MicroEvents`] holding
/// them would give, without a copy of them.
///
/// # Errors
///
/// Never in practice: events always convert.
fn events_ts(events: &[MicroEvent]) -> Result<Ts<MicroEvents>, JsError> {
    // Configured as `Tsify::into_js` configures it for a `MicroEvents`, which
    // serializes as its events do.
    let config = <MicroEvents as Tsify>::SERIALIZATION_CONFIG;
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_missing_as_null(config.missing_as_null)
        .serialize_maps_as_objects(config.hashmap_as_object)
        .serialize_large_number_types_as_bigints(config.large_number_types_as_bigints);
    Ok(Ts::new_unchecked(events.serialize(&serializer)?))
}

/// A run of the Interactive Cache Explorer: the log of ops over a first
/// configuration, the events they produced, and the cursor on those events.
#[wasm_bindgen]
#[derive(Debug)]
pub struct Session {
    /// The core's session.
    inner: model::Session,
}

#[wasm_bindgen]
impl Session {
    /// A session on `Config::default()`, with an empty log.
    ///
    /// # Errors
    ///
    /// Never in practice: the default configuration is valid.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Session, JsError> {
        Ok(Session {
            inner: model::Session::with_config(Config::default())?,
        })
    }

    /// A session on `Config::first_visit()`, the default configuration with
    /// pattern memory: the page's first visit, and "Start fresh".
    ///
    /// # Errors
    ///
    /// Never in practice: that configuration is valid.
    #[wasm_bindgen(js_name = firstVisit)]
    pub fn first_visit() -> Result<Session, JsError> {
        Ok(Session {
            inner: model::Session::with_config(Config::first_visit())?,
        })
    }

    /// A session whose first run has `config`.
    ///
    /// # Errors
    ///
    /// Throws when `config` is not a configuration, or the message of the
    /// first rule it breaks.
    #[wasm_bindgen(js_name = withConfig)]
    pub fn with_config(config: &Ts<Config>) -> Result<Session, JsError> {
        Ok(Session {
            inner: model::Session::with_config(arg(config)?)?,
        })
    }

    /// The session a share string holds, replayed at once. The page opens
    /// links with `ShareLoad`, a slice at a time.
    ///
    /// # Errors
    ///
    /// Throws the message of why the link cannot be opened.
    #[wasm_bindgen(js_name = fromShareString)]
    pub fn from_share_string(s: &str) -> Result<Session, JsError> {
        Ok(Session {
            inner: model::Session::from_share_string(s)?,
        })
    }

    /// The session a saved file's JSON holds, replayed at once.
    ///
    /// # Errors
    ///
    /// Throws the message of why the file cannot be opened.
    #[wasm_bindgen(js_name = fromJson)]
    pub fn from_json(json: &str) -> Result<Session, JsError> {
        Ok(Session {
            inner: model::Session::from_json(json)?,
        })
    }

    /// Appends `op` at the cursor, dropping the undone ops after it, and
    /// returns its events; the cursor goes to its last event.
    ///
    /// # Errors
    ///
    /// Throws why the op is refused, leaving the session unchanged.
    #[wasm_bindgen(js_name = applyOp)]
    pub fn apply_op(&mut self, op: &Ts<Op>) -> Result<Ts<MicroEvents>, JsError> {
        events_ts(self.inner.apply(arg(op)?)?)
    }

    /// Moves the cursor back to the previous undo stop; false if there is
    /// none.
    pub fn undo(&mut self) -> bool {
        self.inner.undo()
    }

    /// Moves the cursor forward to the next undo stop; false if there is
    /// none.
    pub fn redo(&mut self) -> bool {
        self.inner.redo()
    }

    /// Moves the cursor to where `opIndex` ops are done: the end of op
    /// `opIndex - 1`, or position 0 for 0; past the log, to its end.
    #[wasm_bindgen(js_name = scrubTo)]
    pub fn scrub_to(&mut self, #[wasm_bindgen(js_name = opIndex)] op_index: u32) {
        self.inner.scrub_to(op_index);
    }

    /// Moves the cursor to the position `pos`, as far as the log and a
    /// module's unanswered prediction allow.
    ///
    /// # Errors
    ///
    /// Throws when `pos` is not a position.
    pub fn seek(&mut self, pos: &Ts<EventPos>) -> Result<(), JsError> {
        self.inner.seek(arg(pos)?);
        Ok(())
    }

    /// Moves the cursor to the state after step `step` of the displayed run,
    /// as the scrubber and the ticks name it: 0 is the run's origin, and a
    /// step past the run's last is its last. A program load is no step. It
    /// moves as far as the log and a module's unanswered prediction allow.
    #[wasm_bindgen(js_name = seekStep)]
    pub fn seek_step(&mut self, step: u32) {
        self.inner.seek_step(step);
    }

    /// Space: moves the cursor one event on, or at the end of the log
    /// executes one instruction and rests on its first event. `program`, the
    /// drawer's text, is loaded first, as an op of its own, when it differs
    /// from the program loaded.
    ///
    /// # Errors
    ///
    /// Throws when `program` is not a program source.
    #[wasm_bindgen(js_name = stepEvent)]
    pub fn step_event(
        &mut self,
        program: Option<Ts<ProgramSource>>,
    ) -> Result<Ts<StepOutcome>, JsError> {
        let program = program.map(|program| arg(&program)).transpose()?;
        Ok(self.inner.step_event(program.as_ref()).into_ts()?)
    }

    /// Shift+Space: moves the cursor to the end of the op it is in, or of
    /// the next one; at the end of the log, loads `program` as `stepEvent`
    /// does, then executes instructions until one memory access completes.
    ///
    /// # Errors
    ///
    /// Throws when `program` is not a program source.
    #[wasm_bindgen(js_name = stepAccess)]
    pub fn step_access(
        &mut self,
        program: Option<Ts<ProgramSource>>,
    ) -> Result<Ts<StepOutcome>, JsError> {
        let program = program.map(|program| arg(&program)).transpose()?;
        Ok(self.inner.step_access(program.as_ref()).into_ts()?)
    }

    /// Switches between Basics and Advanced. Leaving Advanced on a run Basics
    /// cannot show starts the run's Basics form at once when the run has no
    /// entries, and otherwise asks first.
    ///
    /// # Errors
    ///
    /// Never in practice: the result always converts.
    #[wasm_bindgen(js_name = setAdvanced)]
    pub fn set_advanced(&mut self, on: bool) -> Result<Ts<ConfigChange>, JsError> {
        Ok(self.inner.set_advanced(on).into_ts()?)
    }

    /// A change from the configuration bar: applied at once, or the
    /// question "Reset to apply?" with the configuration to apply.
    ///
    /// # Errors
    ///
    /// Throws when `config` is not a configuration, or why the new run's op
    /// would be refused.
    #[wasm_bindgen(js_name = proposeConfig)]
    pub fn propose_config(&mut self, config: &Ts<Config>) -> Result<Ts<ConfigChange>, JsError> {
        Ok(self.inner.propose_config(arg(config)?)?.into_ts()?)
    }

    /// Everything the configuration bar draws for `shown`, the pending
    /// configuration or the displayed run's.
    ///
    /// # Errors
    ///
    /// Throws when `shown` is not a configuration.
    #[wasm_bindgen(js_name = configControls)]
    pub fn config_controls(&self, shown: &Ts<Config>) -> Result<Ts<ConfigControls>, JsError> {
        Ok(self.inner.config_controls(&arg(shown)?).into_ts()?)
    }

    /// A typed hit time or miss penalty, applied to `shown`.
    ///
    /// # Errors
    ///
    /// Throws when `field` or `shown` is not of its type.
    #[wasm_bindgen(js_name = readConfigNumber)]
    pub fn read_config_number(
        &self,
        field: &Ts<ConfigNumberField>,
        text: &str,
        shown: &Ts<Config>,
    ) -> Result<Ts<ConfigNumberInput>, JsError> {
        Ok(self
            .inner
            .read_config_number(arg(field)?, text, &arg(shown)?)
            .into_ts()?)
    }

    /// What the page draws at the cursor.
    ///
    /// # Errors
    ///
    /// Never in practice: the snapshot always converts.
    pub fn state(&self) -> Result<Ts<State>, JsError> {
        Ok(self.inner.state().into_ts()?)
    }

    /// Every event up to the cursor. The log reads windows with
    /// `eventsRange` instead.
    ///
    /// # Errors
    ///
    /// Never in practice: events always convert.
    pub fn events(&self) -> Result<Ts<MicroEvents>, JsError> {
        events_ts(self.inner.events())
    }

    /// The events whose ids run from `start` up to but not including `end`,
    /// the undone ones after the cursor included.
    ///
    /// # Errors
    ///
    /// Never in practice: events always convert.
    #[wasm_bindgen(js_name = eventsRange)]
    pub fn events_range(&self, start: u32, end: u32) -> Result<Ts<MicroEvents>, JsError> {
        events_ts(self.inner.events_range(start..end))
    }

    /// The displayed run's timeline: the scrubber, the log's header and the
    /// live region's sentence.
    ///
    /// # Errors
    ///
    /// Never in practice: the timeline always converts.
    pub fn timeline(&self) -> Result<Ts<Timeline>, JsError> {
        Ok(self.inner.timeline().into_ts()?)
    }

    /// The displayed run's entries from its `first`, at most `count` of
    /// them.
    ///
    /// # Errors
    ///
    /// Never in practice: entries always convert.
    pub fn entries(&self, first: u32, count: u32) -> Result<Ts<EntryViews>, JsError> {
        Ok(EntryViews(self.inner.entries(first, count)).into_ts()?)
    }

    /// One byte per step of the displayed run, in order, for the scrubber's
    /// ticks. Bit 0 (1) is set on an entry's first step and bit 1 (2) on
    /// every step of an entry that missed, so a tick is 0, 1, 2 or 3. The
    /// loading summary's `ticks` hold the same bytes as a `number[]`.
    pub fn ticks(&self) -> Vec<u8> {
        self.inner.ticks()
    }

    /// The memory table's rows from its `first`, at most `count` of them.
    ///
    /// # Errors
    ///
    /// Never in practice: rows always convert.
    #[wasm_bindgen(js_name = memoryLines)]
    pub fn memory_lines(&self, first: u32, count: u32) -> Result<Ts<MemoryLines>, JsError> {
        Ok(MemoryLines(self.inner.memory_lines(first, count)).into_ts()?)
    }

    /// Replaces the lines the student asked to see, the "Jump to" rows.
    ///
    /// # Errors
    ///
    /// Throws when `lines` is not a list of line addresses.
    // Read as one value, as returned lists are built: wasm-bindgen's glue for
    // a `Vec` argument allocates from the array's length before anything
    // checks the array.
    #[wasm_bindgen(js_name = setMemoryWatch)]
    pub fn set_memory_watch(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "LineAddr[]")] lines: JsValue,
    ) -> Result<(), JsError> {
        let lines: Vec<LineAddr> = read(lines)?;
        self.inner.set_memory_watch(&lines);
        Ok(())
    }

    /// The Stats region at the cursor.
    ///
    /// # Errors
    ///
    /// Never in practice: the stats always convert.
    pub fn stats(&self) -> Result<Ts<Stats>, JsError> {
        Ok(self.inner.stats().into_ts()?)
    }

    /// `addr` split into its tag, index and offset under the displayed
    /// run's configuration.
    ///
    /// # Errors
    ///
    /// Throws when `addr` does not fit the address width.
    pub fn decode(&self, addr: u32) -> Result<Ts<Decoded>, JsError> {
        Ok(self.inner.decode(addr)?.into_ts()?)
    }

    /// A typed address, read against the displayed run's geometry.
    ///
    /// # Errors
    ///
    /// Never in practice: the reading always converts.
    #[wasm_bindgen(js_name = readAddress)]
    pub fn read_address(&self, text: &str) -> Result<Ts<AddressInput>, JsError> {
        Ok(self.inner.read_address(text).into_ts()?)
    }

    /// A typed value, read against the displayed run's word size.
    ///
    /// # Errors
    ///
    /// Never in practice: the reading always converts.
    #[wasm_bindgen(js_name = readWord)]
    pub fn read_word(&self, text: &str) -> Result<Ts<WordInput>, JsError> {
        Ok(self.inner.read_word(text).into_ts()?)
    }

    /// What the address and value boxes would issue on the displayed run.
    ///
    /// # Errors
    ///
    /// Throws when `kind` is not an access kind.
    #[wasm_bindgen(js_name = draftIssue)]
    pub fn draft_issue(
        &self,
        kind: &Ts<AccessKind>,
        #[wasm_bindgen(js_name = addrText)] addr_text: &str,
        #[wasm_bindgen(js_name = valueText)] value_text: &str,
    ) -> Result<Ts<IssueDraft>, JsError> {
        Ok(self
            .inner
            .draft_issue(arg(kind)?, addr_text, value_text)
            .into_ts()?)
    }

    /// The plan of `patch` on `target` at the cursor, or why it is refused.
    ///
    /// # Errors
    ///
    /// Throws when `target` or `patch` is not of its type.
    #[wasm_bindgen(js_name = planEdit)]
    pub fn plan_edit(
        &self,
        target: &Ts<EditTarget>,
        patch: &Ts<EditPatch>,
    ) -> Result<Ts<EditOutcome>, JsError> {
        let target = arg(target)?;
        let patch = arg(patch)?;
        Ok(EditOutcome::from(self.inner.plan_edit(&target, &patch)).into_ts()?)
    }

    /// The way editor's draft of `form` for way `way` of set `set`.
    ///
    /// # Errors
    ///
    /// Throws when `set`, `way` or `form` is not of its type.
    #[wasm_bindgen(js_name = draftEdit)]
    pub fn draft_edit(
        &self,
        set: &Ts<SetIndex>,
        way: &Ts<WayIndex>,
        form: &Ts<WayForm>,
    ) -> Result<Ts<EditDraft>, JsError> {
        Ok(self
            .inner
            .draft_edit(arg(set)?, arg(way)?, &arg(form)?)
            .into_ts()?)
    }

    /// The memory editor's draft of `valueText` for the word at `addr`.
    ///
    /// # Errors
    ///
    /// Throws when `addr` is not an address.
    #[wasm_bindgen(js_name = draftMemoryEdit)]
    pub fn draft_memory_edit(
        &self,
        addr: &Ts<Addr>,
        #[wasm_bindgen(js_name = valueText)] value_text: &str,
    ) -> Result<Ts<MemoryEditDraft>, JsError> {
        Ok(self
            .inner
            .draft_memory_edit(arg(addr)?, value_text)
            .into_ts()?)
    }

    /// `source` parsed as a program or a trace under the displayed run's
    /// configuration.
    ///
    /// # Errors
    ///
    /// Throws when `format` is not a program format.
    #[wasm_bindgen(js_name = parseProgram)]
    pub fn parse_program(
        &self,
        source: &str,
        format: &Ts<ProgramFormat>,
    ) -> Result<Ts<ParseResult>, JsError> {
        Ok(self.inner.parse_program(source, arg(format)?).into_ts()?)
    }

    /// The first position of the displayed run where `p` holds.
    ///
    /// # Errors
    ///
    /// Throws when `p` is not a predicate.
    #[wasm_bindgen(js_name = findEvent)]
    pub fn find_event(&self, p: &Ts<Predicate>) -> Result<Option<Ts<EventPos>>, JsError> {
        Ok(self
            .inner
            .find_event(&arg(p)?)
            .map(|pos| pos.into_ts())
            .transpose()?)
    }

    /// The run as Share describes it.
    ///
    /// # Errors
    ///
    /// Never in practice: the summary always converts.
    #[wasm_bindgen(js_name = shareSummary)]
    pub fn share_summary(&self) -> Result<Ts<ShareSummary>, JsError> {
        Ok(self.inner.share_summary().into_ts()?)
    }

    /// The session as a share string, the text after `#s=`.
    ///
    /// # Errors
    ///
    /// Throws, with the message Share shows, when the string is too long
    /// for a link to open; `toJson` still holds the run.
    #[wasm_bindgen(js_name = toShareString)]
    pub fn to_share_string(&self) -> Result<String, JsError> {
        Ok(self.inner.to_share_string()?)
    }

    /// The session as a saved file holds it: compact JSON, the text a share
    /// string deflates, within the size a file may have, so it always opens.
    #[wasm_bindgen(js_name = toJson)]
    pub fn to_json(&self) -> String {
        self.inner.to_json()
    }

    /// How many ops the log holds, the undone ones included.
    #[wasm_bindgen(js_name = opCount)]
    pub fn op_count(&self) -> u32 {
        self.inner.op_count()
    }

    /// How many ops are done at the cursor.
    pub fn cursor(&self) -> u32 {
        self.inner.cursor()
    }

    /// The cursor's position, with its place in the run and the op.
    ///
    /// # Errors
    ///
    /// Never in practice: the position always converts.
    pub fn position(&self) -> Result<Ts<Position>, JsError> {
        Ok(self.inner.position().into_ts()?)
    }
}

/// A share string or a saved file being opened: checked at once, then
/// replayed a slice at a time, so the page can draw between slices.
#[wasm_bindgen]
#[derive(Debug)]
pub struct ShareLoad {
    /// The core's load.
    inner: model::ShareLoad,
}

#[wasm_bindgen]
impl ShareLoad {
    /// Opens a share string. A link that cannot be opened is reported by
    /// `advance`.
    #[wasm_bindgen(js_name = fromShareString)]
    pub fn from_share_string(s: &str) -> ShareLoad {
        ShareLoad {
            inner: model::ShareLoad::from_share_string(s),
        }
    }

    /// Opens a saved file's JSON, as `fromShareString` opens a link.
    #[wasm_bindgen(js_name = fromJson)]
    pub fn from_json(json: &str) -> ShareLoad {
        ShareLoad {
            inner: model::ShareLoad::from_json(json),
        }
    }

    /// Replays the next slice of about `LOAD_SLICE_EVENTS` events, ending at
    /// an entry boundary. Once the load is done or has failed, every call
    /// answers the same.
    ///
    /// # Errors
    ///
    /// Never in practice: the progress always converts.
    pub fn advance(&mut self) -> Result<Ts<LoadProgress>, JsError> {
        Ok(self.inner.advance_slice().into_ts()?)
    }

    /// The loaded session, its cursor where the document placed it.
    ///
    /// # Errors
    ///
    /// Throws why the load failed, before `advance` has returned `"done"`,
    /// and once the session has been taken.
    #[wasm_bindgen(js_name = takeSession)]
    pub fn take_session(&mut self) -> Result<Session, JsError> {
        Ok(Session {
            inner: self.inner.take_session()?,
        })
    }
}

/// A guided module in progress: the module, its step and the student's
/// answers so far. Each call takes the session the module runs on.
#[wasm_bindgen]
#[derive(Debug)]
pub struct ModuleRun {
    /// The core's module run.
    inner: model::ModuleRun,
}

#[wasm_bindgen]
impl ModuleRun {
    /// A run of the module read from `json`, on its first step and not yet
    /// started.
    ///
    /// # Errors
    ///
    /// Throws where the module breaks its format, and how.
    #[wasm_bindgen(js_name = fromJson)]
    pub fn from_json(json: &str) -> Result<ModuleRun, JsError> {
        Ok(ModuleRun {
            inner: model::ModuleRun::from_json(json)?,
        })
    }

    /// The module.
    ///
    /// # Errors
    ///
    /// Never in practice: a module always converts.
    pub fn module(&self) -> Result<Ts<Module>, JsError> {
        Ok(self.inner.module().into_ts()?)
    }

    /// Starts the module on `session`, from its first step, and returns the
    /// events of the ops it appends.
    ///
    /// # Errors
    ///
    /// Throws why the session refuses those ops; the session and the run are
    /// then as they were.
    pub fn start(&mut self, session: &mut Session) -> Result<Ts<MicroEvents>, JsError> {
        events_ts(self.inner.start(&mut session.inner)?)
    }

    /// What the module panel draws, with `session` as it is.
    ///
    /// # Errors
    ///
    /// Never in practice: the view always converts.
    pub fn view(&self, session: &Session) -> Result<Ts<ModuleView>, JsError> {
        Ok(self.inner.view(&session.inner).into_ts()?)
    }

    /// Re-evaluates the current step after any change to `session`, and
    /// sets or lifts its gate; at most one step completes. Then the module
    /// panel.
    ///
    /// # Errors
    ///
    /// Never in practice: the view always converts.
    pub fn sync(&mut self, session: &mut Session) -> Result<Ts<ModuleView>, JsError> {
        Ok(self.inner.sync(&mut session.inner).into_ts()?)
    }

    /// Selects option `option` of the unanswered prediction, from 0.
    pub fn select(&mut self, option: u32) {
        self.inner.select(option);
    }

    /// Checks the option selected, then the module panel. Correct lifts the
    /// gate and applies the step's `then` ops; wrong makes "Show me"
    /// available.
    ///
    /// # Errors
    ///
    /// Never in practice: the view always converts.
    pub fn check(&mut self, session: &mut Session) -> Result<Ts<ModuleView>, JsError> {
        Ok(self.inner.check(&mut session.inner).into_ts()?)
    }

    /// "Show me": applies an instruct step's ops, moves the cursor to where
    /// an observe step's condition holds, or reveals a prediction's answer
    /// after a wrong one. Then the module panel.
    ///
    /// # Errors
    ///
    /// Never in practice: the view always converts.
    #[wasm_bindgen(js_name = showMe)]
    pub fn show_me(&mut self, session: &mut Session) -> Result<Ts<ModuleView>, JsError> {
        Ok(self.inner.show_me(&mut session.inner).into_ts()?)
    }

    /// Next: goes on from an explanation or an answered prediction, or
    /// starts the module again from the done screen. Then the module panel.
    ///
    /// # Errors
    ///
    /// Never in practice: the view always converts.
    pub fn next(&mut self, session: &mut Session) -> Result<Ts<ModuleView>, JsError> {
        Ok(self.inner.next(&mut session.inner).into_ts()?)
    }

    /// Leaves the module: lifts its gate from `session`, and leaves the
    /// workbench as it is.
    pub fn exit(&mut self, session: &mut Session) {
        self.inner.exit(&mut session.inner);
    }
}

/// The module `json` holds, read and checked.
///
/// # Errors
///
/// Throws where the module breaks its format, and how.
#[wasm_bindgen(js_name = parseModule)]
pub fn parse_module(json: &str) -> Result<Ts<Module>, JsError> {
    Ok(Module::parse(json)?.into_ts()?)
}

/// The module picker for the module index `json` holds.
///
/// # Errors
///
/// Throws where the index breaks its format, and how.
#[wasm_bindgen(js_name = parseModuleIndex)]
pub fn parse_module_index(json: &str) -> Result<Ts<ModuleIndexView>, JsError> {
    Ok(model::parse_module_index(json)?.into_ts()?)
}

/// Whether `p` holds at `session`'s cursor.
///
/// # Errors
///
/// Throws when `p` is not a predicate.
#[wasm_bindgen(js_name = evalPredicate)]
pub fn eval_predicate(session: &Session, p: &Ts<Predicate>) -> Result<bool, JsError> {
    Ok(session.inner.eval_predicate(&arg(p)?))
}

/// `config` checked on its own: its summary and geometry, or the first rule
/// it breaks with its repair.
///
/// # Errors
///
/// Throws when `config` is not a configuration.
#[wasm_bindgen(js_name = validateConfig)]
pub fn validate_config(config: &Ts<Config>) -> Result<Ts<ConfigCheck>, JsError> {
    Ok(model::validate_config(&arg(config)?).into_ts()?)
}

/// The build: the crates' version and the share format.
///
/// # Errors
///
/// Never in practice: the build information always converts.
#[wasm_bindgen(js_name = buildInfo)]
pub fn build_info() -> Result<Ts<BuildInfo>, JsError> {
    Ok(model::build_info().into_ts()?)
}
