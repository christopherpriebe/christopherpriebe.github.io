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

use std::fmt;

use serde::de::{self, DeserializeSeed};
use serde::{Deserialize, Deserializer, Serialize};
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
///
/// Read by [`RawPredicateSeed`], not derived: `all`, `any` and `not` hold
/// further predicates, and a derived `Deserialize` has no way to carry this
/// read's nesting depth down into them.
struct RawPredicate {
    /// A way predicate's way.
    way: Field<WayLoc>,
    /// A way predicate's valid bit.
    valid: Field<bool>,
    /// A way predicate's tag.
    tag: Field<Tag>,
    /// A way or line predicate's dirty bit.
    dirty: Field<bool>,
    /// A way predicate's rank.
    rank: Field<u32>,
    /// A line predicate's line.
    line: Field<AddrLoc>,
    /// Whether a line predicate's line is cached.
    present: Field<bool>,
    /// A memory predicate's word.
    memory: Field<AddrLoc>,
    /// A memory or register predicate's value.
    equals: Field<Word>,
    /// A counter predicate's counter.
    counter: Field<Counter>,
    /// A counter predicate's comparison.
    op: Field<CmpOp>,
    /// A counter predicate's value.
    value: Field<u32>,
    /// A register predicate's register.
    register: Field<Register>,
    /// An `all` predicate's list.
    all: Field<Vec<Predicate>>,
    /// An `any` predicate's list.
    any: Field<Vec<Predicate>>,
    /// A `not` predicate's predicate.
    not: Field<Box<Predicate>>,
    /// An event predicate's event.
    event: Field<EventMatch>,
    /// A `lastAccess` predicate's access.
    last_access: Field<AccessMatch>,
    /// A `config` predicate's configuration.
    config: Field<ConfigMatch>,
}

/// [`RawPredicate`]'s fields, in the order given to `deserialize_struct` and
/// listed in an unknown field's message.
const FIELDS: &[&str] = &[
    "way",
    "valid",
    "tag",
    "dirty",
    "rank",
    "line",
    "present",
    "memory",
    "equals",
    "counter",
    "op",
    "value",
    "register",
    "all",
    "any",
    "not",
    "event",
    "lastAccess",
    "config",
];

impl<'de> Deserialize<'de> for Predicate {
    /// Reads the flat form, then requires the keys of one kind. A predicate
    /// nested deeper than [`MAX_PREDICATE_DEPTH`] is refused before any of
    /// it is read, so the reading stays within the stack whatever the
    /// deserializer's own limit on nesting.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        PredicateSeed { depth: 0 }.deserialize(deserializer)
    }
}

/// Reads a [`Predicate`] enclosed by `depth` `all`, `any` and `not`
/// predicates, refusing before any of it is read once `depth` exceeds
/// [`MAX_PREDICATE_DEPTH`].
///
/// The depth travels with the read, on the call stack, rather than in a
/// shared counter: a read that fails, by an ordinary refusal, an unwind or a
/// thrown exception, leaves nothing behind for the next read to find.
struct PredicateSeed {
    /// How many `all`, `any` or `not` predicates enclose this one.
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for PredicateSeed {
    type Value = Predicate;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Predicate, D::Error> {
        if self.depth > MAX_PREDICATE_DEPTH {
            return Err(de::Error::custom(ShapeError::TooDeep));
        }
        let raw = RawPredicateSeed {
            depth: self.depth + 1,
        }
        .deserialize(deserializer)?;
        Predicate::try_from(raw).map_err(de::Error::custom)
    }
}

/// Reads a [`RawPredicate`], seeding the predicates its `all`, `any` and
/// `not` fields hold at `depth`.
struct RawPredicateSeed {
    /// The depth [`PredicateSeed`] passes to nested predicates.
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for RawPredicateSeed {
    type Value = RawPredicate;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawPredicate, D::Error> {
        deserializer.deserialize_struct(
            "RawPredicate",
            FIELDS,
            RawPredicateVisitor { depth: self.depth },
        )
    }
}

/// Sets `slot` to the map's next value for the field `name`, or refuses a
/// second value for it, as serde's derive refuses a repeated field.
fn set<'de, A: de::MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    slot: &mut Option<T>,
    name: &'static str,
) -> Result<(), A::Error> {
    if slot.is_some() {
        return Err(de::Error::duplicate_field(name));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

/// [`set`], reading the value through `seed` rather than through
/// `Deserialize`.
fn set_seed<'de, A: de::MapAccess<'de>, S: DeserializeSeed<'de>>(
    map: &mut A,
    slot: &mut Option<S::Value>,
    name: &'static str,
    seed: S,
) -> Result<(), A::Error> {
    if slot.is_some() {
        return Err(de::Error::duplicate_field(name));
    }
    *slot = Some(map.next_value_seed(seed)?);
    Ok(())
}

struct RawPredicateVisitor {
    /// The depth [`PredicateSeed`] passes to a nested `all`, `any` or `not`.
    depth: u32,
}

impl<'de> de::Visitor<'de> for RawPredicateVisitor {
    type Value = RawPredicate;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("struct RawPredicate")
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<RawPredicate, A::Error> {
        let mut way = None;
        let mut valid = None;
        let mut tag = None;
        let mut dirty = None;
        let mut rank = None;
        let mut line = None;
        let mut present = None;
        let mut memory = None;
        let mut equals = None;
        let mut counter = None;
        let mut op = None;
        let mut value = None;
        let mut register = None;
        let mut all = None;
        let mut any = None;
        let mut not = None;
        let mut event = None;
        let mut last_access = None;
        let mut config = None;
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::Way => set(&mut map, &mut way, "way")?,
                Key::Valid => set(&mut map, &mut valid, "valid")?,
                Key::Tag => set(&mut map, &mut tag, "tag")?,
                Key::Dirty => set(&mut map, &mut dirty, "dirty")?,
                Key::Rank => set(&mut map, &mut rank, "rank")?,
                Key::Line => set(&mut map, &mut line, "line")?,
                Key::Present => set(&mut map, &mut present, "present")?,
                Key::Memory => set(&mut map, &mut memory, "memory")?,
                Key::Equals => set(&mut map, &mut equals, "equals")?,
                Key::Counter => set(&mut map, &mut counter, "counter")?,
                Key::Op => set(&mut map, &mut op, "op")?,
                Key::Value => set(&mut map, &mut value, "value")?,
                Key::Register => set(&mut map, &mut register, "register")?,
                Key::All => set_seed(
                    &mut map,
                    &mut all,
                    "all",
                    FieldSeed(PredicateVecSeed { depth: self.depth }),
                )?,
                Key::Any => set_seed(
                    &mut map,
                    &mut any,
                    "any",
                    FieldSeed(PredicateVecSeed { depth: self.depth }),
                )?,
                Key::Not => set_seed(
                    &mut map,
                    &mut not,
                    "not",
                    FieldSeed(BoxPredicateSeed { depth: self.depth }),
                )?,
                Key::Event => set(&mut map, &mut event, "event")?,
                Key::LastAccess => set(&mut map, &mut last_access, "lastAccess")?,
                Key::Config => set(&mut map, &mut config, "config")?,
            }
        }
        Ok(RawPredicate {
            way: way.unwrap_or_default(),
            valid: valid.unwrap_or_default(),
            tag: tag.unwrap_or_default(),
            dirty: dirty.unwrap_or_default(),
            rank: rank.unwrap_or_default(),
            line: line.unwrap_or_default(),
            present: present.unwrap_or_default(),
            memory: memory.unwrap_or_default(),
            equals: equals.unwrap_or_default(),
            counter: counter.unwrap_or_default(),
            op: op.unwrap_or_default(),
            value: value.unwrap_or_default(),
            register: register.unwrap_or_default(),
            all: all.unwrap_or_default(),
            any: any.unwrap_or_default(),
            not: not.unwrap_or_default(),
            event: event.unwrap_or_default(),
            last_access: last_access.unwrap_or_default(),
            config: config.unwrap_or_default(),
        })
    }
}

/// One of [`FIELDS`], read as serde's derive reads a struct's field names.
#[derive(Clone, Copy)]
enum Key {
    /// `way`.
    Way,
    /// `valid`.
    Valid,
    /// `tag`.
    Tag,
    /// `dirty`.
    Dirty,
    /// `rank`.
    Rank,
    /// `line`.
    Line,
    /// `present`.
    Present,
    /// `memory`.
    Memory,
    /// `equals`.
    Equals,
    /// `counter`.
    Counter,
    /// `op`.
    Op,
    /// `value`.
    Value,
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

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Key, D::Error> {
        deserializer.deserialize_identifier(KeyVisitor)
    }
}

/// `key`, if it is one of [`FIELDS`].
fn key_of(key: &str) -> Option<Key> {
    Some(match key {
        "way" => Key::Way,
        "valid" => Key::Valid,
        "tag" => Key::Tag,
        "dirty" => Key::Dirty,
        "rank" => Key::Rank,
        "line" => Key::Line,
        "present" => Key::Present,
        "memory" => Key::Memory,
        "equals" => Key::Equals,
        "counter" => Key::Counter,
        "op" => Key::Op,
        "value" => Key::Value,
        "register" => Key::Register,
        "all" => Key::All,
        "any" => Key::Any,
        "not" => Key::Not,
        "event" => Key::Event,
        "lastAccess" => Key::LastAccess,
        "config" => Key::Config,
        _ => return None,
    })
}

struct KeyVisitor;

impl de::Visitor<'_> for KeyVisitor {
    type Value = Key;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("field identifier")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Key, E> {
        key_of(value).ok_or_else(|| de::Error::unknown_field(value, FIELDS))
    }

    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<Key, E> {
        std::str::from_utf8(value)
            .ok()
            .and_then(key_of)
            .ok_or_else(|| de::Error::unknown_field(&String::from_utf8_lossy(value), FIELDS))
    }
}

/// Reads an optional field's value through `seed`, as [`Field<T>`]'s own
/// `Deserialize` reads one unseeded: present as `null` reads as
/// [`Field::Null`]; present with a value, as [`Field::Value`] through
/// `seed`.
struct FieldSeed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for FieldSeed<S> {
    type Value = Field<S::Value>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_option(FieldVisitor(self.0))
    }
}

struct FieldVisitor<S>(S);

impl<'de, S: DeserializeSeed<'de>> de::Visitor<'de> for FieldVisitor<S> {
    type Value = Field<S::Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("option")
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Field::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.deserialize(deserializer).map(Field::Value)
    }
}

/// Reads an `all` or `any` predicate's list, seeding each predicate at
/// `depth`.
struct PredicateVecSeed {
    /// The depth each element of the list is seeded at.
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for PredicateVecSeed {
    type Value = Vec<Predicate>;

    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Vec<Predicate>, D::Error> {
        deserializer.deserialize_seq(PredicateVecVisitor { depth: self.depth })
    }
}

struct PredicateVecVisitor {
    /// The depth each element is seeded at.
    depth: u32,
}

impl<'de> de::Visitor<'de> for PredicateVecVisitor {
    type Value = Vec<Predicate>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a sequence")
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Predicate>, A::Error> {
        let mut predicates = Vec::new();
        while let Some(predicate) = seq.next_element_seed(PredicateSeed { depth: self.depth })? {
            predicates.push(predicate);
        }
        Ok(predicates)
    }
}

/// Reads a `not` predicate's predicate, seeded at `depth`, boxed.
struct BoxPredicateSeed {
    /// The depth the boxed predicate is seeded at.
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for BoxPredicateSeed {
    type Value = Box<Predicate>;

    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Box<Predicate>, D::Error> {
        PredicateSeed { depth: self.depth }
            .deserialize(deserializer)
            .map(Box::new)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use std::panic;

    use serde::de::{MapAccess, Visitor};
    use serde::Deserializer;

    use super::*;

    /// A deserializer for one predicate: `remaining` more `not` wrappers
    /// around a leaf `counter` predicate, panicking instead of reading
    /// further once asked to read a struct with `panic_after` at 0.
    struct NotChain {
        /// How many more `not` levels remain before the leaf.
        remaining: u32,
        /// Structs left to read before this one panics instead.
        panic_after: u32,
    }

    impl<'de> Deserializer<'de> for NotChain {
        type Error = serde_json::Error;

        fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            self.deserialize_struct("", &[], visitor)
        }

        fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            // Every `not` this test builds is present, never `null`.
            visitor.visit_some(self)
        }

        fn deserialize_struct<V: Visitor<'de>>(
            self,
            name: &'static str,
            fields: &'static [&'static str],
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            assert!(
                self.panic_after != 0,
                "a deliberate panic partway through a predicate read"
            );
            if self.remaining == 0 {
                let leaf = serde_json::json!({"counter": "accesses", "op": "==", "value": 0});
                return leaf.deserialize_struct(name, fields, visitor);
            }
            visitor.visit_map(NotMapAccess {
                child: Some(NotChain {
                    remaining: self.remaining - 1,
                    panic_after: self.panic_after - 1,
                }),
            })
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf unit unit_struct newtype_struct seq tuple
            tuple_struct map enum identifier ignored_any
        }
    }

    /// A map of exactly one key, `not`, whose value reads through `child`
    /// once and then is gone, as a real `{"not": ...}` object has.
    struct NotMapAccess {
        /// The value at `not`, taken by the one `next_value_seed` call.
        child: Option<NotChain>,
    }

    impl<'de> MapAccess<'de> for NotMapAccess {
        type Error = serde_json::Error;

        fn next_key_seed<K: DeserializeSeed<'de>>(
            &mut self,
            seed: K,
        ) -> Result<Option<K::Value>, Self::Error> {
            if self.child.is_some() {
                seed.deserialize(KeyDeserializer).map(Some)
            } else {
                Ok(None)
            }
        }

        fn next_value_seed<S: DeserializeSeed<'de>>(
            &mut self,
            seed: S,
        ) -> Result<S::Value, Self::Error> {
            let child = self
                .child
                .take()
                .expect("next_value_seed follows next_key_seed, which found the one key");
            seed.deserialize(child)
        }
    }

    /// A deserializer that reads as the field name `not`.
    struct KeyDeserializer;

    impl<'de> Deserializer<'de> for KeyDeserializer {
        type Error = serde_json::Error;

        fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            visitor.visit_str("not")
        }

        fn deserialize_identifier<V: Visitor<'de>>(
            self,
            visitor: V,
        ) -> Result<V::Value, Self::Error> {
            visitor.visit_str("not")
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum ignored_any
        }
    }

    /// A panic partway through reading a predicate, caught with
    /// `catch_unwind`, leaves nothing behind: unlike a thread-local counter
    /// left raised by a skipped restore, a depth carried on the stack goes
    /// with the unwind, so a later, unrelated read is unaffected.
    #[test]
    fn a_panic_mid_read_leaves_no_depth_behind() {
        let panicked = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            Predicate::deserialize(NotChain {
                remaining: 5,
                panic_after: 3,
            })
        }));
        assert!(panicked.is_err());
        let counter = serde_json::json!({"counter": "accesses", "op": "==", "value": 0});
        let deepest =
            (0..MAX_PREDICATE_DEPTH).fold(counter, |inner, _| serde_json::json!({ "not": inner }));
        assert!(serde_json::from_value::<Predicate>(deepest).is_ok());
    }
}
