//! Guided modules: short lessons that run on the workbench, each a preset
//! configuration, perhaps a program, and steps that explain, ask the
//! student to do something, ask for a prediction, or wait for something to
//! happen.
//!
//! [`Module::parse`] reads a module's JSON and checks it before any of it
//! runs: every step against the configuration in force at that step, which
//! is the preset until a `config` op in an earlier step's "Show me" or
//! "then" ops switches it, so an author sees a set that does not exist or a
//! program that no longer fits when the module is read, not when a student
//! reaches the step. The text of steps and descriptions uses only inline
//! `code` and `*emphasis*`, returned as segments. [`parse_module_index`]
//! reads the list of modules the picker shows.
//!
//! [`ModuleRun`] is a module in progress: which step the student is on, and
//! a prediction's answer, held in the core and never in a share link or a
//! file. While a prediction is unanswered it gates the session, which then
//! refuses every op and keeps the cursor from passing the point where the
//! prediction began, so nothing can reveal the answer first.
//!
//! Text is also written in runs of plain, code and emphasised words, as
//! the page renders the core's structured messages: code in mono, the way
//! the malformed-link message shows the offending field.

mod predicate;
mod run;
mod text;

use serde::de::{self, Deserializer, Unexpected, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use predicate::{
    AccessMatch, AddrLoc, AllPredicate, AnyPredicate, CmpOp, ConfigMatch, ConfigPredicate,
    CounterPredicate, EvalContext, EventMatch, EventPredicate, LastAccessPredicate, LinePredicate,
    MemoryPredicate, NotPredicate, Predicate, RegisterPredicate, WayPredicate,
};
pub use run::{
    Feedback, FeedbackTone, ModuleAction, ModuleActionKind, ModuleRun, ModuleStepKind, ModuleView,
    OptionState, OptionView,
};
pub use text::{Markdown, MarkupError, TextSegment, TextStyle};

pub(crate) use text::{code_spans, Segments};

use crate::config::{Config, ConfigError};
use crate::de::deserialize_unit_enum;
use crate::engine;
use crate::explain;
use crate::format;
use crate::geometry::{AccessError, Geometry};
use crate::machine::Machine;
use crate::op::{Field, Op, OpError, ProgramSource};
use crate::units::{Addr, SetIndex, Tag, WayIndex, Word};

/// A guided module, read and checked by [`Module::parse`]. Serialized in
/// the module format, addresses as numbers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Module {
    /// `04-tracking-lru`: two digits, then a name.
    id: String,
    /// "Tracking LRU".
    title: String,
    /// What the module teaches, in the inline markdown steps use.
    description: String,
    /// Whether the module needs Advanced.
    advanced: bool,
    /// The configuration the module starts with.
    preset: Config,
    /// The program or trace the module loads when it starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    program: Option<ProgramSource>,
    /// The steps, at least one.
    steps: Vec<Step>,
}

/// One step of a module. Serialized with its kind under `kind`.
///
/// Every step has text, and may point at one region of the workbench with a
/// callout. Read through a flat form with every field optional, as an op
/// is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    try_from = "RawStep"
)]
pub enum Step {
    /// Asks the student to do something, and completes once an op made
    /// since the step began leaves the cursor at the end of an entry where
    /// `expect` holds.
    Instruct {
        /// What to do.
        text: String,
        /// What must hold once it is done.
        expect: Predicate,
        /// A hint.
        #[serde(skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
        /// The ops "Show me" applies to do it for the student.
        #[serde(skip_serializing_if = "Option::is_none")]
        show_me: Option<Vec<Op>>,
        /// The region the step points at.
        #[serde(skip_serializing_if = "Option::is_none")]
        callout: Option<Callout>,
    },
    /// Asks for a prediction, which must be answered before the student
    /// can go on.
    Predict {
        /// The question.
        text: String,
        /// The answers to choose from, at least two.
        options: Vec<String>,
        /// The right option's index, from 0.
        answer: u32,
        /// Why the right option is right.
        explain: String,
        /// The ops applied once the prediction is answered.
        #[serde(skip_serializing_if = "Option::is_none")]
        then: Option<Vec<Op>>,
        /// What a wrong answer is told.
        #[serde(skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
        /// The region the step points at.
        #[serde(skip_serializing_if = "Option::is_none")]
        callout: Option<Callout>,
    },
    /// Waits until `until` holds at the cursor.
    Observe {
        /// What to look at.
        text: String,
        /// What must hold.
        until: Predicate,
        /// The region the step points at.
        #[serde(skip_serializing_if = "Option::is_none")]
        callout: Option<Callout>,
    },
    /// Explains, and waits for Next.
    Explain {
        /// The explanation.
        text: String,
        /// The region the step points at.
        #[serde(skip_serializing_if = "Option::is_none")]
        callout: Option<Callout>,
    },
}

/// The region of the workbench a step points at: an area, or one set row
/// or way cell of the cache. Serialized as the area's name, `"cache"`, or
/// as the cell, `{"set":2,"way":0}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(untagged)]
pub enum Callout {
    /// An area.
    Area(CalloutArea),
    /// A set row, or a way cell.
    Cell(CellRef),
}

/// An area of the workbench a callout outlines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum CalloutArea {
    /// The address strip.
    AddressStrip,
    /// The cache table.
    Cache,
    /// The memory table.
    Memory,
    /// The event log.
    Log,
    /// The Stats region.
    Stats,
    /// The configuration bar.
    Config,
    /// The CPU box.
    Cpu,
    /// The transport: Issue, Step and the rest.
    Transport,
}

deserialize_unit_enum!(CalloutArea {
    "addressStrip" => AddressStrip,
    "cache" => Cache,
    "memory" => Memory,
    "log" => Log,
    "stats" => Stats,
    "config" => Config,
    "cpu" => Cpu,
    "transport" => Transport,
});

/// A set row of the cache, or with `way` one way cell of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CellRef {
    /// The set.
    pub set: SetIndex,
    /// The way, for a cell rather than the whole row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub way: Option<WayIndex>,
}

/// What the picker lists: one card per module, and its summary line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ModuleIndexView {
    /// The modules, in the index's order.
    pub modules: Vec<ModuleCard>,
    /// The picker's line while no module runs: "9 modules · none running".
    pub summary: String,
}

/// One module as the picker lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ModuleCard {
    /// The module's id, which names its file.
    pub id: String,
    /// Its number, the id's two digits: "04".
    pub number: String,
    /// Its title.
    pub title: String,
    /// What it teaches, as segments.
    pub description: Vec<TextSegment>,
    /// Whether it needs Advanced, which the card marks.
    pub advanced: bool,
}

/// Why a module or the module index cannot be read. `Display` is the
/// message an author reads: serde's, with its line and column, for JSON
/// that does not have the format's shape, and otherwise the place in the
/// module, as a JSON path, then what is wrong there:
/// `steps[2].until.event.entry: entry must be at least 1.`
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ModuleError {
    /// The text is not JSON of the format's shape.
    #[error("{message}")]
    Json {
        /// serde's message, with the line and column.
        message: String,
    },
    /// A value breaks a rule of the format.
    #[error("{path}: {problem}")]
    Invalid {
        /// Where, as a JSON path: `steps[2].until.event.entry`.
        path: String,
        /// What is wrong there.
        problem: ModuleProblem,
    },
}

/// What is wrong at a place in a module or the module index. `Display` is
/// the message after the place.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ModuleProblem {
    /// An id not of the form `04-tracking-lru`.
    #[error(
        "an id is two digits, a hyphen, then lower-case letters, digits and hyphens, as in \
         `04-tracking-lru`."
    )]
    Id,
    /// An id the index lists twice.
    #[error("`{id}` is listed more than once.")]
    DuplicateId {
        /// The id.
        id: String,
    },
    /// A title that is empty, or only spaces.
    #[error("the title is empty.")]
    EmptyTitle,
    /// Text that is not in the markdown module text may use.
    #[error(transparent)]
    Markup(MarkupError),
    /// A configuration that does not validate.
    #[error(transparent)]
    Config(ConfigError),
    /// A preset whose `advanced` is not the module's.
    #[error("the preset's `advanced` must be {advanced}, as the module's is.")]
    AdvancedMismatch {
        /// The module's `advanced`.
        advanced: bool,
    },
    /// A configuration a module that is not advanced cannot use.
    #[error(
        "a module that is not advanced uses Basics configurations only: write-back, \
         write-allocate and untimed."
    )]
    NotBasics,
    /// The module's program, which a new run keeps, does not parse under a
    /// configuration.
    #[error("the module's program does not fit this configuration. {error}")]
    ProgramUnfit {
        /// Why: the program's first error.
        error: OpError,
    },
    /// A module without steps.
    #[error("a module needs at least one step.")]
    NoSteps,
    /// A prediction with fewer than two options.
    #[error("a prediction needs at least two options; it has {}.", format::data(*.count))]
    FewOptions {
        /// The options it has.
        count: u32,
    },
    /// An answer that is not an option's index.
    #[error(
        "answer {} is not an option; the options are numbered from 0 to {}.",
        format::plain(*.answer),
        format::plain(.options.saturating_sub(1))
    )]
    AnswerOutOfRange {
        /// The answer given.
        answer: u32,
        /// The options there are.
        options: u32,
    },
    /// An empty list of ops.
    #[error("list at least one op, or leave `{key}` out.")]
    EmptyOps {
        /// The list's key: `showMe` or `then`.
        key: &'static str,
    },
    /// An op that no run of the configuration in force would accept.
    #[error(transparent)]
    Op(OpError),
    /// A "Show me" that changes the configuration, on a step whose
    /// `expect` does not tell the configuration it switches to from the
    /// one before, so the step could complete without the switch.
    #[error(
        "Show me changes the configuration, so `expect` needs a `config` predicate that the new \
         configuration meets and the one before it does not."
    )]
    ConfigUnchecked,
    /// A set the configuration in force does not have.
    #[error(
        "this configuration has no set {}; {}.",
        format::data(*.set),
        numbered("set", "sets", *.sets)
    )]
    NoSuchSet {
        /// The set given.
        set: u32,
        /// The sets there are.
        sets: u32,
    },
    /// A way the configuration in force does not have.
    #[error(
        "this configuration has no way {}; {}.",
        format::data(*.way),
        numbered("way", "ways", *.ways)
    )]
    NoSuchWay {
        /// The way given.
        way: u32,
        /// The ways each set has.
        ways: u32,
    },
    /// A tag wider than the configuration's tags.
    #[error(
        "tag {} does not fit this configuration's {}-bit tags ({}\u{2013}{}).",
        format::hex(.tag.get(), 2),
        format::setting(*.tag_bits),
        format::tag(0, *.tag_bits),
        format::tag(largest(*.tag_bits), *.tag_bits)
    )]
    TagTooWide {
        /// The tag given.
        tag: Tag,
        /// The configuration's tag width.
        tag_bits: u8,
    },
    /// A rank where the configuration keeps no replacement order.
    #[error(
        "this configuration keeps no replacement order to check: ranks need LRU or FIFO and more \
         than one way."
    )]
    NoOrder,
    /// A rank no way can have.
    #[error(
        "rank {} is out of range; with {} ways the ranks are 0\u{2013}{}.",
        format::data(*.rank),
        format::data(*.ways),
        format::data(.ways.saturating_sub(1))
    )]
    RankTooHigh {
        /// The rank given.
        rank: u32,
        /// The ways each set has.
        ways: u32,
    },
    /// An address out of range, or a word's address not aligned.
    #[error(transparent)]
    Address(AccessError),
    /// A value the word cannot hold.
    #[error("{}", crate::op::value_too_wide(*.value, *.word_bytes))]
    ValueTooWide {
        /// The value given.
        value: Word,
        /// The word size, in bytes.
        word_bytes: u32,
    },
    /// The `cycles` counter where timed mode is off.
    #[error("the `cycles` counter counts only in timed mode, and this configuration is untimed.")]
    Untimed,
    /// Entry 0: entries are numbered from 1.
    #[error("entry must be at least 1.")]
    EntryZero,
    /// An `all` or `any` without predicates.
    #[error("an `{key}` predicate needs at least one predicate.")]
    EmptyList {
        /// `all` or `any`.
        key: &'static str,
    },
    /// A line predicate asking for a dirty line that is not cached.
    #[error("a line that is not present cannot be dirty.")]
    AbsentDirty,
}

impl Module {
    /// Reads a module from its JSON, and checks it.
    ///
    /// The JSON must have the module format's shape, with no unknown field.
    /// Then: the id has the form `04-tracking-lru`; the title has text; the
    /// description and every step's text, options, explanation and hint use
    /// only inline `code` and `*emphasis*`; the preset validates, its
    /// `advanced` is the module's, and a module that is not advanced uses
    /// Basics configurations only, the preset and every configuration it
    /// switches to; the program parses under the preset and under every
    /// configuration the module switches to, since a new run keeps it;
    /// there is at least one step. Each step is checked against the
    /// configuration in force: a prediction has at least two options and
    /// its answer is one of them; its "Show me" and "then" ops are ops a run
    /// of that configuration would accept, whatever the state; its sets,
    /// ways, tags, ranks, addresses and values exist in or fit that
    /// configuration, the `cycles` counter only when it is timed; entries
    /// are numbered from 1 and no `all` or `any` is empty. An instruct step
    /// whose "Show me" switches the configuration has its `expect` checked
    /// against the new one, and needs `config` predicates there that the
    /// new configuration meets and the one before it does not, so the step
    /// waits for a switch made by hand too.
    ///
    /// # Errors
    ///
    /// [`ModuleError::Json`] for JSON without the format's shape, and
    /// otherwise the [`ModuleError::Invalid`] of the first rule broken.
    pub fn parse(json: &str) -> Result<Module, ModuleError> {
        let raw: RawModule = serde_json::from_str(json).map_err(|error| ModuleError::Json {
            message: error.to_string(),
        })?;
        let module = Module {
            id: raw.id,
            title: raw.title,
            description: raw.description,
            advanced: raw.advanced,
            preset: raw.preset,
            program: raw.program,
            steps: raw.steps,
        };
        module.check()?;
        Ok(module)
    }

    /// The module's id: `04-tracking-lru`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The module's number, its id's two digits: `04`.
    pub fn number(&self) -> &str {
        number(&self.id)
    }

    /// The module's title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// What the module teaches, in inline markdown.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Whether the module needs Advanced.
    pub fn advanced(&self) -> bool {
        self.advanced
    }

    /// The configuration the module starts with.
    pub fn preset(&self) -> &Config {
        &self.preset
    }

    /// The program or trace the module loads when it starts.
    pub fn program(&self) -> Option<&ProgramSource> {
        self.program.as_ref()
    }

    /// The steps.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Checks everything [`Module::parse`] checks after the JSON's shape.
    fn check(&self) -> Result<(), ModuleError> {
        check_id("id", &self.id)?;
        if self.title.trim().is_empty() {
            return Err(invalid("title", ModuleProblem::EmptyTitle));
        }
        check_text("description", &self.description)?;
        let geometry = self
            .preset
            .validate()
            .map_err(|error| invalid("preset", ModuleProblem::Config(error)))?;
        if self.preset.advanced != self.advanced {
            return Err(invalid(
                "preset.advanced",
                ModuleProblem::AdvancedMismatch {
                    advanced: self.advanced,
                },
            ));
        }
        if !self.advanced && !self.preset.fits_basics() {
            return Err(invalid("preset", ModuleProblem::NotBasics));
        }
        let mut force = InForce {
            config: self.preset.clone(),
            geometry,
            program: self.program.clone(),
            advanced: self.advanced,
        };
        force.check_program("program")?;
        if self.steps.is_empty() {
            return Err(invalid("steps", ModuleProblem::NoSteps));
        }
        for (index, step) in self.steps.iter().enumerate() {
            step.check(&format!("steps[{index}]"), &mut force)?;
        }
        Ok(())
    }
}

impl Step {
    /// The step's text.
    pub fn text(&self) -> &str {
        match self {
            Step::Instruct { text, .. }
            | Step::Predict { text, .. }
            | Step::Observe { text, .. }
            | Step::Explain { text, .. } => text,
        }
    }

    /// The region the step points at.
    pub fn callout(&self) -> Option<Callout> {
        match self {
            Step::Instruct { callout, .. }
            | Step::Predict { callout, .. }
            | Step::Observe { callout, .. }
            | Step::Explain { callout, .. } => *callout,
        }
    }

    /// Checks the step at `path` against the configuration in force,
    /// `force`, and moves `force` on past its ops.
    fn check(&self, path: &str, force: &mut InForce) -> Result<(), ModuleError> {
        check_text(&format!("{path}.text"), self.text())?;
        if let Some(Callout::Cell(cell)) = self.callout() {
            check_cell(&format!("{path}.callout"), cell, &force.geometry)?;
        }
        match self {
            Step::Instruct {
                expect,
                hint,
                show_me,
                ..
            } => {
                check_hint(path, hint.as_deref())?;
                let before = force.config.clone();
                let mut switched = false;
                if let Some(ops) = show_me {
                    switched = force.apply(path, "showMe", ops)?;
                }
                let at = format!("{path}.expect");
                expect.check(&at, &force.config, &force.geometry)?;
                if switched && !expect.tells_config(&before, &force.config) {
                    return Err(invalid(&at, ModuleProblem::ConfigUnchecked));
                }
                Ok(())
            }
            Step::Predict {
                options,
                answer,
                explain,
                then,
                hint,
                ..
            } => {
                let count = u32::try_from(options.len()).unwrap_or(u32::MAX);
                if count < 2 {
                    return Err(invalid(
                        &format!("{path}.options"),
                        ModuleProblem::FewOptions { count },
                    ));
                }
                for (index, option) in options.iter().enumerate() {
                    check_text(&format!("{path}.options[{index}]"), option)?;
                }
                if *answer >= count {
                    return Err(invalid(
                        &format!("{path}.answer"),
                        ModuleProblem::AnswerOutOfRange {
                            answer: *answer,
                            options: count,
                        },
                    ));
                }
                check_text(&format!("{path}.explain"), explain)?;
                check_hint(path, hint.as_deref())?;
                if let Some(ops) = then {
                    force.apply(path, "then", ops)?;
                }
                Ok(())
            }
            Step::Observe { until, .. } => {
                until.check(&format!("{path}.until"), &force.config, &force.geometry)
            }
            Step::Explain { .. } => Ok(()),
        }
    }
}

/// What is in force at a step of a module being checked: the configuration
/// and its geometry, the program a new run keeps, and whether the module is
/// advanced.
struct InForce {
    /// The configuration.
    config: Config,
    /// Its geometry.
    geometry: Geometry,
    /// The program loaded, if any.
    program: Option<ProgramSource>,
    /// Whether the module is advanced.
    advanced: bool,
}

impl InForce {
    /// Checks that the program, if any, parses under the configuration, for
    /// the configuration at `path`.
    fn check_program(&self, path: &str) -> Result<(), ModuleError> {
        let Some(program) = &self.program else {
            return Ok(());
        };
        let load = Op::Program {
            source: program.source.clone(),
            format: program.format,
        };
        let machine = self.machine(path)?;
        engine::check_form(&machine, &load)
            .map_err(|error| invalid(path, ModuleProblem::ProgramUnfit { error }))
    }

    /// A machine at the start of a run of the configuration, which checks
    /// an op's form; the configuration is checked before it is in force.
    fn machine(&self, path: &str) -> Result<Machine, ModuleError> {
        Machine::start(self.config.clone())
            .map_err(|error| invalid(path, ModuleProblem::Config(error)))
    }

    /// Checks the ops listed under `key` of the step at `path`, each against
    /// the configuration in force before it, and moves on past each: a
    /// `config` op switches the configuration, which must validate, suit
    /// the module's mode and fit the program; a `Program` op replaces the
    /// program. Returns whether a `config` op switched the configuration.
    fn apply(&mut self, path: &str, key: &'static str, ops: &[Op]) -> Result<bool, ModuleError> {
        if ops.is_empty() {
            return Err(invalid(
                &format!("{path}.{key}"),
                ModuleProblem::EmptyOps { key },
            ));
        }
        let mut switched = false;
        for (index, op) in ops.iter().enumerate() {
            let at = format!("{path}.{key}[{index}]");
            match op {
                Op::Config { config } => {
                    let at = format!("{at}.config");
                    let geometry = config
                        .validate()
                        .map_err(|error| invalid(&at, ModuleProblem::Config(error)))?;
                    if !self.advanced && !config.fits_basics() {
                        return Err(invalid(&at, ModuleProblem::NotBasics));
                    }
                    self.config = config.clone();
                    self.geometry = geometry;
                    self.check_program(&at)?;
                    switched = true;
                }
                op => {
                    let machine = self.machine(&at)?;
                    engine::check_form(&machine, op)
                        .map_err(|error| invalid(&at, ModuleProblem::Op(error)))?;
                    if let Op::Program { source, format } = op {
                        self.program = Some(ProgramSource {
                            source: source.clone(),
                            format: *format,
                        });
                    }
                }
            }
        }
        Ok(switched)
    }
}

/// Reads the module index, `index.json`: one row per module, in the order
/// the picker lists them, each with the module's id, title, description and
/// `advanced`. The ids must be unique and of the form `04-tracking-lru`, and
/// the descriptions in the inline markdown module text uses.
///
/// # Errors
///
/// [`ModuleError::Json`] for JSON without the index's shape, and otherwise
/// the [`ModuleError::Invalid`] of the first row that breaks a rule.
pub fn parse_module_index(json: &str) -> Result<ModuleIndexView, ModuleError> {
    let rows: Vec<IndexRow> = serde_json::from_str(json).map_err(|error| ModuleError::Json {
        message: error.to_string(),
    })?;
    let mut modules: Vec<ModuleCard> = Vec::with_capacity(rows.len());
    for (index, row) in rows.into_iter().enumerate() {
        check_id(&format!("[{index}].id"), &row.id)?;
        if modules.iter().any(|card| card.id == row.id) {
            return Err(invalid(
                &format!("[{index}].id"),
                ModuleProblem::DuplicateId { id: row.id },
            ));
        }
        if row.title.trim().is_empty() {
            return Err(invalid(
                &format!("[{index}].title"),
                ModuleProblem::EmptyTitle,
            ));
        }
        let description = text::markup(&row.description).map_err(|error| {
            invalid(
                &format!("[{index}].description"),
                ModuleProblem::Markup(error),
            )
        })?;
        modules.push(ModuleCard {
            number: number(&row.id).to_owned(),
            id: row.id,
            title: row.title,
            description,
            advanced: row.advanced,
        });
    }
    let summary = explain::modules_idle(u32::try_from(modules.len()).unwrap_or(u32::MAX));
    Ok(ModuleIndexView { modules, summary })
}

/// The number of the module whose id is `id`: its first two characters.
fn number(id: &str) -> &str {
    id.get(..2).unwrap_or(id)
}

/// The error at `path`.
fn invalid(path: &str, problem: ModuleProblem) -> ModuleError {
    ModuleError::Invalid {
        path: path.to_owned(),
        problem,
    }
}

/// Checks that `id`, at `path`, has the form `04-tracking-lru`: two digits,
/// a hyphen, then at least one lower-case letter, digit or hyphen.
fn check_id(path: &str, id: &str) -> Result<(), ModuleError> {
    let bytes = id.as_bytes();
    let well_formed = bytes.len() > 3
        && bytes.iter().take(2).all(u8::is_ascii_digit)
        && bytes.get(2) == Some(&b'-')
        && bytes
            .iter()
            .skip(3)
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if well_formed {
        Ok(())
    } else {
        Err(invalid(path, ModuleProblem::Id))
    }
}

/// Checks that `text`, at `path`, uses only the inline markdown module text
/// may use.
fn check_text(path: &str, text: &str) -> Result<(), ModuleError> {
    text::markup(text)
        .map(|_| ())
        .map_err(|error| invalid(path, ModuleProblem::Markup(error)))
}

/// Checks the hint of the step at `path`, if it has one.
fn check_hint(path: &str, hint: Option<&str>) -> Result<(), ModuleError> {
    hint.map_or(Ok(()), |hint| check_text(&format!("{path}.hint"), hint))
}

/// Checks the callout cell `cell`, at `path`, against `geometry`: its set,
/// and its way if it names one, exist.
fn check_cell(path: &str, cell: CellRef, geometry: &Geometry) -> Result<(), ModuleError> {
    if geometry.set_index(cell.set.get()).is_none() {
        return Err(invalid(
            &format!("{path}.set"),
            ModuleProblem::NoSuchSet {
                set: cell.set.get(),
                sets: geometry.sets(),
            },
        ));
    }
    match cell
        .way
        .filter(|way| geometry.way_index(way.get()).is_none())
    {
        Some(way) => Err(invalid(
            &format!("{path}.way"),
            ModuleProblem::NoSuchWay {
                way: way.get(),
                ways: geometry.ways(),
            },
        )),
        None => Ok(()),
    }
}

/// The numbers of `count` sets or ways, for a message: "its only set is
/// 0", "its ways are 0–1".
fn numbered(one: &str, many: &str, count: u32) -> String {
    match count.checked_sub(1) {
        Some(last) if last > 0 => format!("its {many} are 0\u{2013}{}", format::data(last)),
        _ => format!("its only {one} is 0"),
    }
}

/// The largest tag `tag_bits` bits hold.
fn largest(tag_bits: u8) -> u32 {
    1_u64
        .checked_shl(u32::from(tag_bits))
        .map_or(u32::MAX, |limit| {
            u32::try_from(limit.saturating_sub(1)).unwrap_or(u32::MAX)
        })
}

/// A module as read from JSON, before it is checked.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawModule {
    /// The id.
    id: String,
    /// The title.
    title: String,
    /// The description.
    description: String,
    /// Whether the module needs Advanced.
    advanced: bool,
    /// The preset.
    preset: Config,
    /// The program, if any.
    #[serde(default)]
    program: Option<ProgramSource>,
    /// The steps.
    steps: Vec<Step>,
}

/// A row of the module index.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IndexRow {
    /// The module's id.
    id: String,
    /// Its title.
    title: String,
    /// Its description.
    description: String,
    /// Whether it needs Advanced.
    advanced: bool,
}

/// A step as read from JSON: its kind, its text, and every field any kind
/// has, each optional. [`Step`]'s `TryFrom` then requires the fields of the
/// kind and refuses the others.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawStep {
    /// The step's kind.
    kind: StepName,
    /// Its text.
    text: String,
    /// An instruct step's predicate.
    #[serde(default)]
    expect: Field<Predicate>,
    /// An instruct or predict step's hint.
    #[serde(default)]
    hint: Field<String>,
    /// An instruct step's "Show me" ops.
    #[serde(default)]
    show_me: Field<Vec<ModuleOp>>,
    /// A predict step's options.
    #[serde(default)]
    options: Field<Vec<String>>,
    /// A predict step's answer.
    #[serde(default)]
    answer: Field<u32>,
    /// A predict step's explanation.
    #[serde(default)]
    explain: Field<String>,
    /// A predict step's ops after the answer.
    #[serde(default)]
    then: Field<Vec<ModuleOp>>,
    /// An observe step's predicate.
    #[serde(default)]
    until: Field<Predicate>,
    /// Any step's callout.
    #[serde(default)]
    callout: Field<Callout>,
}

/// The kinds of step, as the `kind` field names them.
#[derive(Clone, Copy)]
enum StepName {
    /// `instruct`.
    Instruct,
    /// `predict`.
    Predict,
    /// `observe`.
    Observe,
    /// `explain`.
    Explain,
}

deserialize_unit_enum!(StepName {
    "instruct" => Instruct,
    "predict" => Predict,
    "observe" => Observe,
    "explain" => Explain,
});

impl StepName {
    /// The name the `kind` field gives it.
    fn name(self) -> &'static str {
        match self {
            StepName::Instruct => "instruct",
            StepName::Predict => "predict",
            StepName::Observe => "observe",
            StepName::Explain => "explain",
        }
    }
}

/// Why a flat step does not have the fields of its kind. Lower case and
/// unterminated, like serde's own messages, since it reaches a reader only
/// through serde, which adds the line and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
enum StepShapeError {
    /// A field the kind requires is absent or `null`.
    #[error("missing field `{field}` for step `{kind}`")]
    Missing {
        /// The kind.
        kind: &'static str,
        /// The field.
        field: &'static str,
    },
    /// A field the kind does not have is present.
    #[error("unexpected field `{field}` for step `{kind}`")]
    Unexpected {
        /// The kind.
        kind: &'static str,
        /// The field.
        field: &'static str,
    },
}

impl TryFrom<RawStep> for Step {
    type Error = StepShapeError;

    // An optional field that is `null` is not given, as serde reads a
    // `null` option; a required one that is `null` is missing.
    fn try_from(raw: RawStep) -> Result<Self, StepShapeError> {
        let kind = raw.kind.name();
        let present = [
            ("expect", raw.expect.is_present()),
            ("hint", raw.hint.is_present()),
            ("showMe", raw.show_me.is_present()),
            ("options", raw.options.is_present()),
            ("answer", raw.answer.is_present()),
            ("explain", raw.explain.is_present()),
            ("then", raw.then.is_present()),
            ("until", raw.until.is_present()),
        ];
        let allowed: &[&str] = match raw.kind {
            StepName::Instruct => &["expect", "hint", "showMe"],
            StepName::Predict => &["options", "answer", "explain", "then", "hint"],
            StepName::Observe => &["until"],
            StepName::Explain => &[],
        };
        if let Some(&(field, _)) = present
            .iter()
            .find(|&&(field, present)| present && !allowed.contains(&field))
        {
            return Err(StepShapeError::Unexpected { kind, field });
        }
        let missing = |field| StepShapeError::Missing { kind, field };
        let ops = |ops: Option<Vec<ModuleOp>>| {
            ops.map(|ops| ops.into_iter().map(|ModuleOp(op)| op).collect())
        };
        let text = raw.text;
        let callout = raw.callout.value();
        Ok(match raw.kind {
            StepName::Instruct => Step::Instruct {
                text,
                expect: raw.expect.value().ok_or_else(|| missing("expect"))?,
                hint: raw.hint.value(),
                show_me: ops(raw.show_me.value()),
                callout,
            },
            StepName::Predict => Step::Predict {
                text,
                options: raw.options.value().ok_or_else(|| missing("options"))?,
                answer: raw.answer.value().ok_or_else(|| missing("answer"))?,
                explain: raw.explain.value().ok_or_else(|| missing("explain"))?,
                then: ops(raw.then.value()),
                hint: raw.hint.value(),
                callout,
            },
            StepName::Observe => Step::Observe {
                text,
                until: raw.until.value().ok_or_else(|| missing("until"))?,
                callout,
            },
            StepName::Explain => Step::Explain { text, callout },
        })
    }
}

impl<'de> Deserialize<'de> for Callout {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(CalloutVisitor)
    }
}

/// Reads a callout: an area's name, or a cell.
struct CalloutVisitor;

impl<'de> Visitor<'de> for CalloutVisitor {
    type Value = Callout;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an area such as \"cache\", or a cell such as {\"set\": 2, \"way\": 0}")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Callout, E> {
        CalloutArea::deserialize(de::value::StrDeserializer::<E>::new(v)).map(Callout::Area)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<Callout, A::Error> {
        CellRef::deserialize(de::value::MapAccessDeserializer::new(map)).map(Callout::Cell)
    }
}

/// An op as module JSON writes it: an op of the share format, except that
/// an issue's `addr`, a memory edit's `target.addr` and a way edit's
/// `patch.line` may be hex strings such as `"0x0A4"`.
struct ModuleOp(Op);

impl<'de> Deserialize<'de> for ModuleOp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(deserializer)?;
        if let Some(op) = value.as_object_mut() {
            hex_to_number::<D::Error>(op.get_mut("addr"))?;
            if let Some(target) = op.get_mut("target").and_then(Value::as_object_mut) {
                hex_to_number::<D::Error>(target.get_mut("addr"))?;
            }
            if let Some(patch) = op.get_mut("patch").and_then(Value::as_object_mut) {
                hex_to_number::<D::Error>(patch.get_mut("line"))?;
            }
        }
        Op::deserialize(value)
            .map(ModuleOp)
            .map_err(de::Error::custom)
    }
}

/// What an address in module JSON is expected to be, for serde's messages.
const ADDRESS: &str = "an address: a number, or hex after 0x such as \"0x0A4\"";

/// Replaces `value`, if it is a string, with the address it writes in hex.
fn hex_to_number<E: de::Error>(value: Option<&mut Value>) -> Result<(), E> {
    let Some(value) = value else {
        return Ok(());
    };
    if let Some(text) = value.as_str() {
        let addr =
            hex_address(text).ok_or_else(|| E::invalid_value(Unexpected::Str(text), &ADDRESS))?;
        *value = Value::from(addr);
    }
    Ok(())
}

/// The address `text` writes: `0x`, or `0X`, then one to eight hex digits.
fn hex_address(text: &str) -> Option<u32> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

/// Reads an address as module JSON may write it: a number, or a hex string
/// such as `"0x0A4"`.
fn addr_or_hex<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Addr, D::Error> {
    deserializer.deserialize_any(AddrVisitor)
}

/// Reads an optional address as [`addr_or_hex`] reads one.
fn optional_addr_or_hex<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Addr>, D::Error> {
    Option::<HexAddr>::deserialize(deserializer).map(|addr| addr.map(|HexAddr(addr)| addr))
}

/// An address read by [`addr_or_hex`].
struct HexAddr(Addr);

impl<'de> Deserialize<'de> for HexAddr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        addr_or_hex(deserializer).map(HexAddr)
    }
}

/// Reads an address: a number, or a hex string.
struct AddrVisitor;

impl Visitor<'_> for AddrVisitor {
    type Value = Addr;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(ADDRESS)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Addr, E> {
        u32::try_from(v)
            .map(Addr::new_unchecked)
            .map_err(|_| E::invalid_value(Unexpected::Unsigned(v), &self))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Addr, E> {
        u64::try_from(v)
            .map_err(|_| E::invalid_value(Unexpected::Signed(v), &self))
            .and_then(|v| self.visit_u64(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Addr, E> {
        hex_address(v)
            .map(Addr::new_unchecked)
            .ok_or_else(|| E::invalid_value(Unexpected::Str(v), &self))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn a_hex_address_is_0x_and_one_to_eight_digits() {
        assert_eq!(hex_address("0x0A4"), Some(0x0A4));
        assert_eq!(hex_address("0X0a4"), Some(0x0A4));
        assert_eq!(hex_address("0xFFFFFFFF"), Some(u32::MAX));
        for text in [
            "0A4",
            "0x",
            "0x+A4",
            "0x0G4",
            "0x100000000",
            " 0x0A4",
            "164",
        ] {
            assert_eq!(hex_address(text), None, "{text}");
        }
    }

    #[test]
    fn an_address_reads_from_a_number_or_hex() {
        let read = |json: &str| AddrLoc::deserialize(&mut serde_json::Deserializer::from_str(json));
        assert_eq!(read(r#"{"addr": 164}"#).unwrap().addr.get(), 164);
        assert_eq!(read(r#"{"addr": "0x0A4"}"#).unwrap().addr.get(), 164);
        assert!(read(r#"{"addr": -1}"#)
            .unwrap_err()
            .to_string()
            .starts_with("invalid value: integer `-1`, expected an address"));
        assert!(read(r#"{"addr": 4294967296}"#)
            .unwrap_err()
            .to_string()
            .starts_with("invalid value: integer `4294967296`, expected an address"));
    }

    #[test]
    fn a_module_id_is_two_digits_and_a_name() {
        for id in ["04-tracking-lru", "01-a", "10-x2-y"] {
            assert!(check_id("id", id).is_ok(), "{id}");
        }
        for id in [
            "4-tracking",
            "04tracking",
            "04-",
            "04-Tracking",
            "04-tracking_lru",
            "ab-c",
        ] {
            assert!(check_id("id", id).is_err(), "{id}");
        }
        assert_eq!(number("04-tracking-lru"), "04");
    }

    #[test]
    fn callouts_serialize_as_an_area_or_a_cell() {
        let area: Callout = serde_json::from_str(r#""transport""#).unwrap();
        assert_eq!(area, Callout::Area(CalloutArea::Transport));
        assert_eq!(serde_json::to_string(&area).unwrap(), r#""transport""#);
        let cell: Callout = serde_json::from_str(r#"{"set": 2}"#).unwrap();
        assert_eq!(serde_json::to_string(&cell).unwrap(), r#"{"set":2}"#);
        for name in [
            "addressStrip",
            "cache",
            "memory",
            "log",
            "stats",
            "config",
            "cpu",
            "transport",
        ] {
            let json = format!("\"{name}\"");
            let area: Callout = serde_json::from_str(&json).unwrap();
            assert_eq!(serde_json::to_string(&area).unwrap(), json);
        }
    }
}
