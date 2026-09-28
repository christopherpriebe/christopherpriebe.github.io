//! Predicates: the conditions a module's steps wait for, read from module
//! JSON, checked against the configuration in force when the module is
//! read, and evaluated against the machine at a position of the session's
//! log.
//!
//! A predicate is keyed by which field it has, as the spec shapes it:
//! `{"way": {…}, "valid": true}`, `{"counter": "misses", "op": ">=",
//! "value": 2}`. It is read through a flat form with every key optional,
//! which then requires exactly one of the keys that name a kind, and only
//! the other keys that kind takes, so an author reads which key is wrong
//! rather than serde's "did not match any variant".
//!
//! Most predicates read the machine: a way's fields, whether a line is
//! cached, a memory word, a counter, a register, the configuration. Two read
//! the log. `event` is the event the cursor rests on, and its entry within
//! the run, which no state can tell apart from the events around it.
//! `lastAccess` is the displayed run's most recent completed access at or
//! before the cursor, and holds only when an op made since the current step
//! began made it: ops are told apart by serial, since entry numbers restart
//! with each run and positions are reused after the log is truncated.

use std::cell::Cell;

use serde::{de, Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::{addr_or_hex, optional_addr_or_hex, ModuleError, ModuleProblem};
use crate::cache::Way;
use crate::config::{AllocatePolicy, Config, Replacement, WritePolicy};
use crate::de::deserialize_unit_enum;
use crate::event::{Counter, EventKind, MicroEvent};
use crate::format;
use crate::geometry::{AccessError, Geometry};
use crate::limits::MAX_PREDICATE_DEPTH;
use crate::machine::Machine;
use crate::op::{AccessKind, Field};
use crate::session::Session;
use crate::snapshot::WayLoc;
use crate::stats::Outcome;
use crate::units::{Addr, EventPos, Register, Tag, Word};

/// A condition on the machine or the log at a position of the session.
///
/// Serialized as the spec shapes it, keyed by which field it has: a way
/// predicate as `{"way":{"set":2,"way":0},"valid":true}`, a counter
/// predicate as `{"counter":"misses","op":">=","value":2}`. Read through a
/// flat form with every key optional, as an op is, and nested at most
/// [`MAX_PREDICATE_DEPTH`] deep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(untagged)]
pub enum Predicate {
    /// A way's fields.
    Way(WayPredicate),
    /// Whether a line is cached, and dirty.
    Line(LinePredicate),
    /// A word of memory.
    Memory(MemoryPredicate),
    /// A counter of the Stats region.
    Counter(CounterPredicate),
    /// A register.
    Register(RegisterPredicate),
    /// Every predicate listed.
    All(AllPredicate),
    /// At least one predicate listed.
    Any(AnyPredicate),
    /// Not the predicate given.
    Not(NotPredicate),
    /// The event the cursor rests on.
    Event(EventPredicate),
    /// The displayed run's most recent completed access, made since the
    /// step began.
    LastAccess(LastAccessPredicate),
    /// The displayed run's configuration.
    Config(ConfigPredicate),
}

/// True when every field given equals the way's: `tag` compares the tag's
/// value, not the line's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct WayPredicate {
    /// The way.
    pub way: WayLoc,
    /// Whether it holds a line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid: Option<bool>,
    /// The tag of the line it holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Tag>,
    /// Whether its line is dirty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    /// Its place in its set's replacement order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
}

/// True when a valid way holds the line, or with `present: false` when
/// none does; with `dirty`, when that way's dirty bit is as given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct LinePredicate {
    /// Any byte address inside the line.
    pub line: AddrLoc,
    /// Whether the line is cached; true when not given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub present: Option<bool>,
    /// Whether the cached line is dirty; `true` implies the line is cached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
}

/// True when memory's word at the address, not the cache's copy, equals
/// the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct MemoryPredicate {
    /// The word's address, a multiple of the word size.
    pub memory: AddrLoc,
    /// The value, written in module JSON as a JSON number and read exactly
    /// to 2^64 − 1.
    pub equals: Word,
}

/// True when the comparison holds for the counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct CounterPredicate {
    /// The counter.
    pub counter: Counter,
    /// How it compares.
    pub op: CmpOp,
    /// What it is compared with. A `u32`, as counts are: no counter of a
    /// session comes near its end, and a module read back across the
    /// boundary then holds no number JavaScript cannot represent.
    pub value: u32,
}

/// True when the register holds the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct RegisterPredicate {
    /// The register, `r0` to `r7`.
    pub register: Register,
    /// The value, as for a memory predicate.
    pub equals: Word,
}

/// True when every predicate listed holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct AllPredicate {
    /// The predicates; at least one.
    pub all: Vec<Predicate>,
}

/// True when at least one predicate listed holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct AnyPredicate {
    /// The predicates; at least one.
    pub any: Vec<Predicate>,
}

/// True when the predicate given does not hold.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct NotPredicate {
    /// The predicate.
    pub not: Box<Predicate>,
}

/// True when the event the cursor rests on is of the kind, and, when an
/// entry is given, belongs to that entry of the displayed run: flow A's
/// "drag the timeline back to step 1.2, the tag compare" is `{"event":
/// {"kind": "miss", "entry": 1}}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct EventPredicate {
    /// The event wanted.
    pub event: EventMatch,
}

/// The event an event predicate wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventMatch {
    /// Its kind.
    pub kind: EventKind,
    /// Its entry's number within the run, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<u32>,
}

/// True when the displayed run's most recent completed access at or before
/// the cursor was made by an op made since the step began, and matches
/// every field given: the design's instruct steps, which wait for an access
/// made after they began.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct LastAccessPredicate {
    /// The access wanted.
    pub last_access: AccessMatch,
}

/// The access a `lastAccess` predicate wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessMatch {
    /// Whether it loads or stores.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<AccessKind>,
    /// The word's address: a number, or in module JSON a hex string such as
    /// `"0x0A4"`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_addr_or_hex"
    )]
    pub addr: Option<Addr>,
    /// Whether it hit or missed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// True when the displayed run's configuration matches every field given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ConfigPredicate {
    /// The configuration wanted.
    pub config: ConfigMatch,
}

/// The fields of a configuration a `config` predicate wants.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigMatch {
    /// The cache's size, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u32>,
    /// The line size, in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_bytes: Option<u32>,
    /// The ways per set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub associativity: Option<u32>,
    /// The replacement policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<Replacement>,
    /// The write policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_policy: Option<WritePolicy>,
    /// The allocate policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocate_policy: Option<AllocatePolicy>,
    /// Whether timed mode is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timed: Option<bool>,
}

/// An address, as a line or memory predicate names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddrLoc {
    /// The address: a number, or in module JSON a hex string such as
    /// `"0x0A4"`.
    #[serde(deserialize_with = "addr_or_hex")]
    pub addr: Addr,
}

/// How a counter predicate compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
pub enum CmpOp {
    /// `==`: equal.
    #[serde(rename = "==")]
    Equal,
    /// `>=`: at least.
    #[serde(rename = ">=")]
    AtLeast,
    /// `<=`: at most.
    #[serde(rename = "<=")]
    AtMost,
}

deserialize_unit_enum!(CmpOp {
    "==" => Equal,
    ">=" => AtLeast,
    "<=" => AtMost,
});

/// What a predicate is evaluated against: the machine at a position of a
/// session's log, the event there, and the serial of the last op made
/// before the current step began, so that only an op with a greater serial
/// counts as made since.
#[derive(Clone, Copy, Debug)]
pub struct EvalContext<'a> {
    /// The session whose log the position is in.
    session: &'a Session,
    /// The machine at the position.
    machine: &'a Machine,
    /// The position.
    pos: EventPos,
    /// The serial of the last op made before the step began.
    since: u64,
}

impl<'a> EvalContext<'a> {
    /// The session's cursor, with every op of its log counting as made
    /// since the step began: what the spec's `evalPredicate` evaluates
    /// against.
    pub fn new(session: &'a Session) -> Self {
        Self {
            session,
            machine: session.machine(),
            pos: session.cursor_position(),
            since: 0,
        }
    }

    /// Position `pos` of `session`'s log, where the machine is `machine`,
    /// counting the ops whose serial is above `since` as made since the
    /// step began.
    pub(crate) fn at(
        session: &'a Session,
        machine: &'a Machine,
        pos: EventPos,
        since: u64,
    ) -> Self {
        Self {
            session,
            machine,
            pos,
            since,
        }
    }

    /// This context, counting only the ops whose serial is above `since` as
    /// made since the step began.
    #[must_use]
    pub(crate) fn since(self, since: u64) -> Self {
        Self { since, ..self }
    }

    /// The event the cursor rests on at the position; none at position 0.
    fn event(&self) -> Option<&'a MicroEvent> {
        self.session.event_at(self.pos)
    }
}

impl Predicate {
    /// Whether the predicate holds in `cx`. A predicate that names a set, a
    /// way or an address the configuration does not have is false; so is
    /// every predicate about the event or the last access where there is
    /// none.
    pub fn eval(&self, cx: &EvalContext<'_>) -> bool {
        let machine = cx.machine;
        let geometry = &machine.geometry;
        match self {
            Predicate::Way(p) => machine.cache.way(p.way.set, p.way.way).is_some_and(|way| {
                p.valid.is_none_or(|valid| way.is_valid() == valid)
                    && p.tag.is_none_or(|tag| way.tag() == Some(tag))
                    && p.dirty.is_none_or(|dirty| way.is_dirty() == dirty)
                    && p.rank.is_none_or(|rank| way.rank == rank)
            }),
            Predicate::Line(p) => {
                let Some(addr) = geometry.addr(p.line.addr.get()) else {
                    return false;
                };
                let location = geometry.locate(addr);
                let held = machine.cache.set(location.set).and_then(|set| {
                    set.ways()
                        .iter()
                        .find(|way| way.tag() == Some(location.tag))
                });
                held.is_some() == p.present.unwrap_or(true)
                    && p.dirty
                        .is_none_or(|dirty| held.is_some_and(Way::is_dirty) == dirty)
            }
            Predicate::Memory(p) => geometry
                .addr(p.memory.addr.get())
                .filter(|addr| addr.get() % geometry.word_bytes() == 0)
                .is_some_and(|addr| machine.memory.word(addr) == p.equals),
            Predicate::Counter(p) => {
                let counter = machine.counters.get(p.counter);
                let value = u64::from(p.value);
                match p.op {
                    CmpOp::Equal => counter == value,
                    CmpOp::AtLeast => counter >= value,
                    CmpOp::AtMost => counter <= value,
                }
            }
            Predicate::Register(p) => machine.registers.get(p.register) == p.equals.get(),
            Predicate::All(p) => p.all.iter().all(|p| p.eval(cx)),
            Predicate::Any(p) => p.any.iter().any(|p| p.eval(cx)),
            Predicate::Not(p) => !p.not.eval(cx),
            Predicate::Event(p) => cx.event().is_some_and(|event| {
                event.kind == p.event.kind
                    && p.event.entry.is_none_or(|entry| event.entry == Some(entry))
            }),
            Predicate::LastAccess(p) => {
                cx.session
                    .last_access(cx.pos, machine.run)
                    .is_some_and(|last| {
                        let wanted = &p.last_access;
                        last.serial > cx.since
                            && wanted.kind.is_none_or(|kind| last.kind == kind)
                            && wanted.addr.is_none_or(|addr| last.addr == addr)
                            && wanted.outcome.is_none_or(|outcome| last.outcome == outcome)
                    })
            }
            Predicate::Config(p) => p.config.matches(&machine.config),
        }
    }

    /// Checks the predicate at `path` against `config`, the configuration
    /// in force where it is evaluated, whose geometry is `geometry`: its
    /// sets, ways, tags and ranks exist, a rank only where the policy keeps
    /// an order, its addresses fit and a memory word's and an access's are
    /// aligned, its values fit the word, the `cycles` counter only in timed
    /// mode, an entry from 1, a line that is not present not dirty, and no
    /// empty list.
    pub(crate) fn check(
        &self,
        path: &str,
        config: &Config,
        geometry: &Geometry,
    ) -> Result<(), ModuleError> {
        let invalid = |at: &str, problem| {
            Err(ModuleError::Invalid {
                path: format!("{path}.{at}"),
                problem,
            })
        };
        match self {
            Predicate::Way(p) => {
                check_way(path, "way", p.way, geometry)?;
                if let Some(tag) = p.tag.filter(|tag| geometry.tag(tag.get()).is_none()) {
                    return invalid(
                        "tag",
                        ModuleProblem::TagTooWide {
                            tag,
                            tag_bits: geometry.tag_bits(),
                        },
                    );
                }
                if let Some(rank) = p.rank {
                    let ordered = matches!(
                        config.cache.replacement,
                        Replacement::Lru | Replacement::Fifo
                    );
                    if !ordered || geometry.ways() < 2 {
                        return invalid("rank", ModuleProblem::NoOrder);
                    }
                    if rank >= geometry.ways() {
                        return invalid(
                            "rank",
                            ModuleProblem::RankTooHigh {
                                rank,
                                ways: geometry.ways(),
                            },
                        );
                    }
                }
                Ok(())
            }
            Predicate::Line(p) => {
                check_addr(&format!("{path}.line.addr"), p.line.addr, geometry, false)?;
                if p.present == Some(false) && p.dirty == Some(true) {
                    return invalid("dirty", ModuleProblem::AbsentDirty);
                }
                Ok(())
            }
            Predicate::Memory(p) => {
                check_addr(
                    &format!("{path}.memory.addr"),
                    p.memory.addr,
                    geometry,
                    true,
                )?;
                if geometry.fit_word(p.equals.get()) != p.equals {
                    return invalid(
                        "equals",
                        ModuleProblem::ValueTooWide {
                            value: p.equals,
                            word_bytes: geometry.word_bytes(),
                        },
                    );
                }
                Ok(())
            }
            Predicate::Counter(p) => {
                if p.counter == Counter::Cycles && config.timing.is_none() {
                    return invalid("counter", ModuleProblem::Untimed);
                }
                Ok(())
            }
            Predicate::Register(_) | Predicate::Config(_) => Ok(()),
            Predicate::All(p) => {
                check_list(&format!("{path}.all"), "all", &p.all, config, geometry)
            }
            Predicate::Any(p) => {
                check_list(&format!("{path}.any"), "any", &p.any, config, geometry)
            }
            Predicate::Not(p) => p.not.check(&format!("{path}.not"), config, geometry),
            Predicate::Event(p) => {
                if p.event.entry == Some(0) {
                    return invalid("event.entry", ModuleProblem::EntryZero);
                }
                Ok(())
            }
            Predicate::LastAccess(p) => match p.last_access.addr {
                Some(addr) => check_addr(&format!("{path}.lastAccess.addr"), addr, geometry, true),
                None => Ok(()),
            },
        }
    }

    /// Whether the predicate tells `after`, the configuration a switch
    /// leads to, from `before`, the one in force before it: `after` meets
    /// every `config` predicate the predicate cannot hold without, and
    /// `before` does not, so it cannot hold until the switch is made, by
    /// "Show me" or by hand.
    pub(crate) fn tells_config(&self, before: &Config, after: &Config) -> bool {
        self.required_config(after) == Some(true) && self.required_config(before) == Some(false)
    }

    /// Whether `config` meets every `config` predicate the predicate cannot
    /// hold without: itself, or those an `all` holds, at any depth; none if
    /// there is no such predicate.
    fn required_config(&self, config: &Config) -> Option<bool> {
        match self {
            Predicate::Config(p) => Some(p.config.matches(config)),
            Predicate::All(p) => p
                .all
                .iter()
                .filter_map(|p| p.required_config(config))
                .reduce(|met, also| met && also),
            _ => None,
        }
    }
}

impl ConfigMatch {
    /// Whether `config` has every field given.
    fn matches(&self, config: &Config) -> bool {
        let cache = &config.cache;
        self.size_bytes.is_none_or(|size| cache.size_bytes == size)
            && self.line_bytes.is_none_or(|line| cache.line_bytes == line)
            && self
                .associativity
                .is_none_or(|ways| cache.associativity == ways)
            && self
                .replacement
                .is_none_or(|replacement| cache.replacement == replacement)
            && self
                .write_policy
                .is_none_or(|policy| cache.write_policy == policy)
            && self
                .allocate_policy
                .is_none_or(|policy| cache.allocate_policy == policy)
            && self
                .timed
                .is_none_or(|timed| config.timing.is_some() == timed)
    }
}

/// Checks the way `way` names at `path.key` against `geometry`: its set and
/// its way exist.
pub(crate) fn check_way(
    path: &str,
    key: &str,
    way: WayLoc,
    geometry: &Geometry,
) -> Result<(), ModuleError> {
    if geometry.set_index(way.set.get()).is_none() {
        return Err(ModuleError::Invalid {
            path: format!("{path}.{key}.set"),
            problem: ModuleProblem::NoSuchSet {
                set: way.set.get(),
                sets: geometry.sets(),
            },
        });
    }
    if geometry.way_index(way.way.get()).is_none() {
        return Err(ModuleError::Invalid {
            path: format!("{path}.{key}.way"),
            problem: ModuleProblem::NoSuchWay {
                way: way.way.get(),
                ways: geometry.ways(),
            },
        });
    }
    Ok(())
}

/// Checks the address `addr` at `path` against `geometry`: it fits the
/// address width and, if `word` is set, it is a multiple of the word size.
fn check_addr(path: &str, addr: Addr, geometry: &Geometry, word: bool) -> Result<(), ModuleError> {
    let error = if geometry.addr(addr.get()).is_none() {
        Some(AccessError::OutOfRange {
            addr: u64::from(addr.get()),
            address_bits: geometry.address_bits(),
        })
    } else if word && addr.get() % geometry.word_bytes() != 0 {
        Some(AccessError::Misaligned {
            addr,
            address_bits: geometry.address_bits(),
            word_bytes: geometry.word_bytes(),
        })
    } else {
        None
    };
    match error {
        Some(error) => Err(ModuleError::Invalid {
            path: path.to_owned(),
            problem: ModuleProblem::Address(error),
        }),
        None => Ok(()),
    }
}

/// Checks the list of an `all` or `any` predicate at `path`: it is not
/// empty, and each of its predicates checks.
fn check_list(
    path: &str,
    key: &'static str,
    list: &[Predicate],
    config: &Config,
    geometry: &Geometry,
) -> Result<(), ModuleError> {
    if list.is_empty() {
        return Err(ModuleError::Invalid {
            path: path.to_owned(),
            problem: ModuleProblem::EmptyList { key },
        });
    }
    for (index, predicate) in list.iter().enumerate() {
        predicate.check(&format!("{path}[{index}]"), config, geometry)?;
    }
    Ok(())
}

/// A predicate as read from JSON: every key any kind has, each optional.
/// [`Predicate`]'s `TryFrom` then requires exactly one key that names a
/// kind, the keys that kind requires, and no key it does not take.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPredicate {
    /// A way predicate's way.
    #[serde(default)]
    way: Field<WayLoc>,
    /// A way predicate's valid bit.
    #[serde(default)]
    valid: Field<bool>,
    /// A way predicate's tag.
    #[serde(default)]
    tag: Field<Tag>,
    /// A way or line predicate's dirty bit.
    #[serde(default)]
    dirty: Field<bool>,
    /// A way predicate's rank.
    #[serde(default)]
    rank: Field<u32>,
    /// A line predicate's line.
    #[serde(default)]
    line: Field<AddrLoc>,
    /// Whether a line predicate's line is cached.
    #[serde(default)]
    present: Field<bool>,
    /// A memory predicate's word.
    #[serde(default)]
    memory: Field<AddrLoc>,
    /// A memory or register predicate's value.
    #[serde(default)]
    equals: Field<Word>,
    /// A counter predicate's counter.
    #[serde(default)]
    counter: Field<Counter>,
    /// A counter predicate's comparison.
    #[serde(default)]
    op: Field<CmpOp>,
    /// A counter predicate's value.
    #[serde(default)]
    value: Field<u32>,
    /// A register predicate's register.
    #[serde(default)]
    register: Field<Register>,
    /// An `all` predicate's list.
    #[serde(default)]
    all: Field<Vec<Predicate>>,
    /// An `any` predicate's list.
    #[serde(default)]
    any: Field<Vec<Predicate>>,
    /// A `not` predicate's predicate.
    #[serde(default)]
    not: Field<Box<Predicate>>,
    /// An event predicate's event.
    #[serde(default)]
    event: Field<EventMatch>,
    /// A `lastAccess` predicate's access.
    #[serde(default)]
    last_access: Field<AccessMatch>,
    /// A `config` predicate's configuration.
    #[serde(default)]
    config: Field<ConfigMatch>,
}

thread_local! {
    /// How many predicates enclose the one being read, through `all`, `any`
    /// and `not`. serde's derive passes nothing down to a nested value, so
    /// the depth is kept here, and restored whenever a read returns, whether
    /// it read a predicate or not. An exception that JavaScript throws during
    /// a read unwinds past the restore and leaves the depth raised; the page
    /// passes only plain data, which throws none.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

impl<'de> Deserialize<'de> for Predicate {
    /// Reads the flat form, then requires the keys of one kind. A predicate
    /// nested deeper than [`MAX_PREDICATE_DEPTH`] is refused before any of
    /// it is read, so the reading stays within the stack whatever the
    /// deserializer's own limit on nesting.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let depth = DEPTH.get();
        if depth > MAX_PREDICATE_DEPTH {
            return Err(de::Error::custom(ShapeError::TooDeep));
        }
        DEPTH.set(depth + 1);
        let raw = RawPredicate::deserialize(deserializer);
        DEPTH.set(depth);
        Predicate::try_from(raw?).map_err(de::Error::custom)
    }
}

/// The kinds of predicate, each named by the key it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `way`.
    Way,
    /// `line`.
    Line,
    /// `memory`.
    Memory,
    /// `counter`.
    Counter,
    /// `register`.
    Register,
    /// `all`.
    All,
    /// `any`.
    Any,
    /// `not`.
    Not,
    /// `event`.
    Event,
    /// `lastAccess`.
    LastAccess,
    /// `config`.
    Config,
}

impl Kind {
    /// Every kind, in the order a message lists them.
    const ALL: [Kind; 11] = [
        Kind::Way,
        Kind::Line,
        Kind::Memory,
        Kind::Counter,
        Kind::Register,
        Kind::All,
        Kind::Any,
        Kind::Not,
        Kind::Event,
        Kind::LastAccess,
        Kind::Config,
    ];

    /// The key that names the kind.
    fn name(self) -> &'static str {
        match self {
            Kind::Way => "way",
            Kind::Line => "line",
            Kind::Memory => "memory",
            Kind::Counter => "counter",
            Kind::Register => "register",
            Kind::All => "all",
            Kind::Any => "any",
            Kind::Not => "not",
            Kind::Event => "event",
            Kind::LastAccess => "lastAccess",
            Kind::Config => "config",
        }
    }

    /// The keys the kind takes besides its own.
    fn extras(self) -> &'static [&'static str] {
        match self {
            Kind::Way => &["valid", "tag", "dirty", "rank"],
            Kind::Line => &["present", "dirty"],
            Kind::Memory | Kind::Register => &["equals"],
            Kind::Counter => &["op", "value"],
            Kind::All | Kind::Any | Kind::Not | Kind::Event | Kind::LastAccess | Kind::Config => {
                &[]
            }
        }
    }

    /// "a" or "an", whichever reads right before the kind's key.
    fn article(self) -> &'static str {
        if matches!(self, Kind::All | Kind::Any | Kind::Event) {
            "an"
        } else {
            "a"
        }
    }
}

/// Why a flat predicate is not a predicate: it does not have the keys of one
/// kind, or it nests too deep. Lower case and unterminated, like serde's own
/// messages, since it reaches a reader only through serde, which adds the
/// line and column.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
enum ShapeError {
    /// None, or more than one, of the keys that name a kind.
    #[error(
        "a predicate needs exactly one of {}; found {}",
        Kind::ALL.map(|kind| format!("`{}`", kind.name())).join(", "),
        listed(.found.iter().map(|kind| kind.name()))
    )]
    Kinds {
        /// The kinds whose keys were found.
        found: Vec<Kind>,
    },
    /// A key the kind does not take.
    #[error(
        "`{key}` is not allowed in {} `{}` predicate (allowed: {})",
        .kind.article(),
        .kind.name(),
        listed(.kind.extras().iter().copied())
    )]
    NotAllowed {
        /// The kind.
        kind: Kind,
        /// The key.
        key: &'static str,
    },
    /// A key the kind requires is absent or `null`.
    #[error("{} `{}` predicate needs `{key}`", .kind.article(), .kind.name())]
    Missing {
        /// The kind.
        kind: Kind,
        /// The key.
        key: &'static str,
    },
    /// Predicates nested deeper than [`MAX_PREDICATE_DEPTH`].
    #[error(
        "predicates nest at most {} deep in `all`, `any` and `not`",
        format::setting(MAX_PREDICATE_DEPTH)
    )]
    TooDeep,
}

/// `keys` joined for a message, `way, line`, or `none`.
fn listed<'k>(keys: impl Iterator<Item = &'k str>) -> String {
    let keys: Vec<&str> = keys.collect();
    if keys.is_empty() {
        "none".to_owned()
    } else {
        keys.join(", ")
    }
}

impl TryFrom<RawPredicate> for Predicate {
    type Error = ShapeError;

    // A key that names the kind, or that the kind requires, finds a `null`
    // missing; an optional key reads it as not given, as serde reads a
    // `null` option.
    fn try_from(raw: RawPredicate) -> Result<Self, ShapeError> {
        let present = [
            raw.way.is_present(),
            raw.line.is_present(),
            raw.memory.is_present(),
            raw.counter.is_present(),
            raw.register.is_present(),
            raw.all.is_present(),
            raw.any.is_present(),
            raw.not.is_present(),
            raw.event.is_present(),
            raw.last_access.is_present(),
            raw.config.is_present(),
        ];
        let found: Vec<Kind> = Kind::ALL
            .iter()
            .zip(present)
            .filter_map(|(&kind, present)| present.then_some(kind))
            .collect();
        let &[kind] = found.as_slice() else {
            return Err(ShapeError::Kinds { found });
        };
        let extras = [
            ("valid", raw.valid.is_present()),
            ("tag", raw.tag.is_present()),
            ("dirty", raw.dirty.is_present()),
            ("rank", raw.rank.is_present()),
            ("present", raw.present.is_present()),
            ("equals", raw.equals.is_present()),
            ("op", raw.op.is_present()),
            ("value", raw.value.is_present()),
        ];
        if let Some(&(key, _)) = extras
            .iter()
            .find(|&&(key, present)| present && !kind.extras().contains(&key))
        {
            return Err(ShapeError::NotAllowed { kind, key });
        }
        let missing = |key| ShapeError::Missing { kind, key };
        Ok(match kind {
            Kind::Way => Predicate::Way(WayPredicate {
                way: raw.way.value().ok_or_else(|| missing("way"))?,
                valid: raw.valid.value(),
                tag: raw.tag.value(),
                dirty: raw.dirty.value(),
                rank: raw.rank.value(),
            }),
            Kind::Line => Predicate::Line(LinePredicate {
                line: raw.line.value().ok_or_else(|| missing("line"))?,
                present: raw.present.value(),
                dirty: raw.dirty.value(),
            }),
            Kind::Memory => Predicate::Memory(MemoryPredicate {
                memory: raw.memory.value().ok_or_else(|| missing("memory"))?,
                equals: raw.equals.value().ok_or_else(|| missing("equals"))?,
            }),
            Kind::Counter => Predicate::Counter(CounterPredicate {
                counter: raw.counter.value().ok_or_else(|| missing("counter"))?,
                op: raw.op.value().ok_or_else(|| missing("op"))?,
                value: raw.value.value().ok_or_else(|| missing("value"))?,
            }),
            Kind::Register => Predicate::Register(RegisterPredicate {
                register: raw.register.value().ok_or_else(|| missing("register"))?,
                equals: raw.equals.value().ok_or_else(|| missing("equals"))?,
            }),
            Kind::All => Predicate::All(AllPredicate {
                all: raw.all.value().ok_or_else(|| missing("all"))?,
            }),
            Kind::Any => Predicate::Any(AnyPredicate {
                any: raw.any.value().ok_or_else(|| missing("any"))?,
            }),
            Kind::Not => Predicate::Not(NotPredicate {
                not: raw.not.value().ok_or_else(|| missing("not"))?,
            }),
            Kind::Event => Predicate::Event(EventPredicate {
                event: raw.event.value().ok_or_else(|| missing("event"))?,
            }),
            Kind::LastAccess => Predicate::LastAccess(LastAccessPredicate {
                last_access: raw
                    .last_access
                    .value()
                    .ok_or_else(|| missing("lastAccess"))?,
            }),
            Kind::Config => Predicate::Config(ConfigPredicate {
                config: raw.config.value().ok_or_else(|| missing("config"))?,
            }),
        })
    }
}
