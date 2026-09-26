//! One load or store, as the ordered micro-events of its case.
//!
//! Every access starts with `Decode`, then the tag compare, `Hit` or `Miss`,
//! which moves the access counters. A miss that allocates chooses a
//! `Victim`, writes it back if it is dirty (`WriteBack`), evicts it if it
//! holds a line (`Evict`) and brings the new line in (`Fill`). Then comes the
//! result: a load's `Complete`; a store's `Write`, followed under
//! write-through by `WriteThrough`; or, for a store that misses under
//! write-no-allocate, a `MemWrite` that leaves the cache alone. Last comes
//! `Rank`, only if the hit or the fill moved a rank.
//!
//! | Case | Events |
//! |---|---|
//! | Load hit | `Decode`, `Hit`, `Complete`, `Rank`? |
//! | Load miss, empty way | `Decode`, `Miss`, `Victim`, `Fill`, `Complete`, `Rank`? |
//! | Load miss, clean victim | `Decode`, `Miss`, `Victim`, `Evict`, `Fill`, `Complete`, `Rank`? |
//! | Load miss, dirty victim | `Decode`, `Miss`, `Victim`, `WriteBack`, `Evict`, `Fill`, `Complete`, `Rank`? |
//! | Store hit, write-back | `Decode`, `Hit`, `Write`, `Rank`? |
//! | Store hit, write-through | `Decode`, `Hit`, `Write`, `WriteThrough`, `Rank`? |
//! | Store miss, allocate, write-back | `Decode`, `Miss`, `Victim`, `WriteBack`?, `Evict`?, `Fill`, `Write`, `Rank`? |
//! | Store miss, allocate, write-through | `Decode`, `Miss`, `Victim`, `Evict`?, `Fill`, `Write`, `WriteThrough`, `Rank`? |
//! | Store miss, no-allocate, either write policy | `Decode`, `Miss`, `MemWrite` |
//!
//! A `?` marks an event that happens only when its condition holds: `Rank`
//! when a rank moved, `WriteBack` when the victim is dirty, `Evict` when it
//! holds a line.
//!
//! A write-back only writes memory: the victim keeps its line and its dirty
//! bit until `Evict` clears valid, tag, dirty and data together.
//!
//! In timed mode each access is charged where its time goes: the hit time at
//! the tag compare, and the miss penalty at each transfer to or from memory,
//! so at `WriteBack`, at `Fill` and at `MemWrite`. A hit therefore costs the
//! hit time, a miss the hit time plus the miss penalty, and a dirty victim's
//! write-back the miss penalty again. Every miss also has a `Stall` event,
//! once it is known what the CPU waits for: after `Victim` when the miss
//! allocates, since by then it is known whether the victim is dirty, and
//! after `Miss` for a write-around. Every event is stamped with the run's
//! cycle count, so the stamps count the stall down.
//!
//! Each event hands the emitter its log line as a closure, which the
//! `explain` module words and which is built only when the sink records.

use crate::cache::{Dirtiness, Slot, Way};
use crate::config::{AllocatePolicy, WritePolicy};
use crate::engine::{Draft, Emitter, Flow, Halt};
use crate::event::{Cause, ChangeRef, Counter, EventKind};
use crate::explain::{self, StallFor, WriteNote};
use crate::geometry::Location;
use crate::machine::{AccessContext, InvariantViolation, Lookup, Machine, StallWindow};
use crate::memory::{set_word_at, word_at};
use crate::op::AccessKind;
use crate::replacement::{RankMove, VictimReason};
use crate::units::{Addr, Cycle, LineAddr, Register, SetIndex, Tag, WayIndex, Word, WordIndex};

/// A load or store the engine executes. Its address has been checked: it
/// fits the address width and is a multiple of the word size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// A load of the word at `addr` into register `into`.
    Load {
        /// The word's address.
        addr: Addr,
        /// The register the word goes into.
        into: Register,
    },
    /// A store of `value` to the word at `addr`. Only the value's low
    /// `8 × word_bytes` bits are stored.
    Store {
        /// The word's address.
        addr: Addr,
        /// The value, as wide as a register.
        value: u64,
    },
}

impl Request {
    /// The address the request reads or writes.
    pub(crate) fn addr(self) -> Addr {
        match self {
            Request::Load { addr, .. } | Request::Store { addr, .. } => addr,
        }
    }

    /// Whether the request is a load or a store.
    pub(crate) fn kind(self) -> AccessKind {
        match self {
            Request::Load { .. } => AccessKind::Read,
            Request::Store { .. } => AccessKind::Write,
        }
    }
}

/// Executes `request` as the run's next entry, handing its events to `out`,
/// then checks the machine's invariants at the entry's end.
///
/// Returns [`Flow::Stop`] when the sink stopped the access, or when an
/// invariant no longer holds; the op then ends with an `Error` event.
pub(crate) fn access(machine: &mut Machine, request: Request, out: &mut Emitter<'_>) -> Flow {
    let result =
        run(machine, request, out).and_then(|()| machine.check_invariants().map_err(Halt::Broken));
    match result {
        Ok(()) => Flow::Continue,
        Err(Halt::Stopped) => Flow::Stop,
        Err(Halt::Broken(violation)) => {
            out.internal_error(machine, violation);
            Flow::Stop
        }
    }
}

/// The access's events, from `Decode` to its last.
fn run(machine: &mut Machine, request: Request, out: &mut Emitter<'_>) -> Result<(), Halt> {
    let addr = request.addr();
    let location = machine.geometry.locate(addr);
    out.begin_entry(machine);
    machine.access = Some(AccessContext {
        request,
        location,
        lookup: None,
        victim: None,
        way: None,
        stall: None,
        done: false,
    });
    let decode = Draft {
        addr: Some(addr),
        set: Some(location.set),
        ..Draft::new(EventKind::Decode, Some(Cause::LineSize))
    };
    out.emit(machine, decode, || {
        explain::decode(&machine.geometry, request.kind(), addr, location)
    })?;

    let hit = lookup(machine, request, location, out)?;
    let way = match (hit, request) {
        (Some(way), _) => way,
        (None, Request::Store { value, .. })
            if machine.config.cache.allocate_policy == AllocatePolicy::WriteNoAllocate =>
        {
            stall(machine, location.set, StallFor::WriteAround, out)?;
            return write_around(machine, addr, value, location, out);
        }
        (None, _) => allocate(machine, location, out)?,
    };
    let moves = plan_ranks(machine, location.set, way, hit.is_some())?;
    let ranked = !moves.is_empty();
    match request {
        Request::Load { into, .. } => complete(machine, addr, into, location, way, ranked, out)?,
        Request::Store { value, .. } => write(machine, addr, value, location, way, ranked, out)?,
    }
    if ranked {
        rank(machine, location.set, way, &moves, out)?;
    }
    Ok(())
}

/// The tag compare: `Hit` or `Miss`, which moves `accesses`, `loads` or
/// `stores`, and `hits` or `misses`, and in timed mode charges the hit time.
/// Returns the way that hit.
fn lookup(
    machine: &mut Machine,
    request: Request,
    location: Location,
    out: &mut Emitter<'_>,
) -> Result<Option<WayIndex>, Halt> {
    let Some(set) = machine.cache.set(location.set) else {
        return Err(lost(machine, location.set));
    };
    let tags: Vec<Option<Tag>> = set.ways().iter().map(Way::tag).collect();
    let hit = (0_u32..)
        .zip(&tags)
        .find_map(|(index, tag)| (*tag == Some(location.tag)).then_some(index))
        .map(WayIndex::new_unchecked);
    let kind = match request.kind() {
        AccessKind::Read => Counter::Loads,
        AccessKind::Write => Counter::Stores,
    };
    let (event, outcome) = match hit {
        Some(_) => (EventKind::Hit, Counter::Hits),
        None => (EventKind::Miss, Counter::Misses),
    };
    // A one-way set's miss names the tag its way held.
    let held = tags.first().copied().flatten();
    let counters = [Counter::Accesses, kind, outcome];
    for counter in counters {
        machine.counters.add(counter, 1);
    }
    let mut changed: Vec<ChangeRef> = counters
        .into_iter()
        .map(|counter| ChangeRef::Counter { counter })
        .collect();
    changed.extend(charge(machine, Cost::Hit));
    if let Some(access) = &mut machine.access {
        access.lookup = Some(Lookup { hit, tags });
        access.way = hit;
    }
    let draft = Draft {
        addr: Some(request.addr()),
        set: Some(location.set),
        way: hit,
        changed,
        ..Draft::new(event, Some(Cause::Associativity))
    };
    out.emit(machine, draft, || match hit {
        Some(way) => explain::hit(&machine.geometry, location.set, way, location.tag),
        None => explain::miss(&machine.geometry, location.set, location.tag, held),
    })?;
    Ok(hit)
}

/// The allocation path of a miss: `Victim`, then in timed mode `Stall`,
/// then `WriteBack` if the victim is dirty and `Evict` if it holds a line,
/// then `Fill`. Returns the way filled.
fn allocate(
    machine: &mut Machine,
    location: Location,
    out: &mut Emitter<'_>,
) -> Result<WayIndex, Halt> {
    let set = location.set;
    let policy = machine.config.cache.replacement.policy();
    let Some(ways) = machine.cache.set(set) else {
        return Err(lost(machine, set));
    };
    let choice = policy.victim(ways.ways(), &mut machine.prng);
    let way = choice.way;
    let cause = victim_cause(choice.reason);
    if let Some(access) = &mut machine.access {
        access.victim = Some(choice);
    }
    let victim = Draft {
        set: Some(set),
        way: Some(way),
        ..Draft::new(EventKind::Victim, Some(cause))
    };
    out.emit(machine, victim, || {
        explain::victim(&machine.geometry, set, choice)
    })?;

    let Some(held) = machine.cache.way(set, way) else {
        return Err(lost(machine, set));
    };
    let old = held.tag().map(|tag| (tag, held.is_dirty()));
    let dirty_line = old
        .filter(|(_, dirty)| *dirty)
        .map(|(tag, _)| machine.geometry.line_addr(tag, set));
    let line = location.line;
    let wait = match dirty_line {
        Some(victim) => StallFor::WriteBackThenFill { victim, line },
        None => StallFor::Fill { line },
    };
    stall(machine, set, wait, out)?;
    if let Some((tag, dirty)) = old {
        if let Some(victim) = dirty_line {
            write_back(machine, victim, set, way, out)?;
        }
        evict(machine, tag, set, way, dirty, cause, out)?;
    }
    fill(machine, location, way, out)?;
    Ok(way)
}

/// `Stall`, in timed mode only: the CPU waits while memory does what `wait`
/// names, one miss penalty per transfer. The stall's length, and the cycle
/// count at which it is over, go into the access context for the CPU's
/// countdown. The stall itself charges nothing: the transfers that follow
/// do. Untimed, there is no stall and nothing happens.
fn stall(
    machine: &mut Machine,
    set: SetIndex,
    wait: StallFor,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let Some(timing) = machine.config.timing else {
        return Ok(());
    };
    let total = timing.miss_cycles.saturating_mul(wait.transfers());
    let end = machine
        .counters
        .cycles
        .get()
        .saturating_add(u64::from(total));
    if let Some(access) = &mut machine.access {
        access.stall = Some(StallWindow {
            total,
            end: Cycle::new_unchecked(end),
        });
    }
    let draft = Draft {
        set: Some(set),
        ..Draft::new(EventKind::Stall, Some(Cause::Timing))
    };
    out.emit(machine, draft, || {
        explain::stall(&machine.geometry, timing.miss_cycles, wait)
    })
}

/// `WriteBack`: the dirty victim's line goes to memory. The way keeps its
/// line and its dirty bit; only memory changes, and its stale words with it.
/// In timed mode it costs the miss penalty.
fn write_back(
    machine: &mut Machine,
    line: LineAddr,
    set: SetIndex,
    way: WayIndex,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let word_bytes = machine.geometry.word_bytes();
    let mut changed = vec![ChangeRef::Counter {
        counter: Counter::Writebacks,
    }];
    match machine.cache.way(set, way).map(|victim| &victim.slot) {
        Some(Slot::Valid { data, .. }) => {
            for start in word_starts(machine) {
                let addr = Addr::new_unchecked(line.get() + start);
                if machine.memory.word(addr).get() != word_at(data, start, word_bytes) {
                    changed.push(ChangeRef::Memory { addr });
                }
            }
            machine.memory.write_line(line, data);
        }
        _ => return Err(lost(machine, set)),
    }
    machine.counters.add(Counter::Writebacks, 1);
    changed.extend(charge(machine, Cost::Miss));
    let draft = Draft {
        addr: Some(Addr::new_unchecked(line.get())),
        set: Some(set),
        way: Some(way),
        changed,
        ..Draft::new(EventKind::WriteBack, Some(Cause::WritePolicy))
    };
    out.emit(machine, draft, || {
        explain::write_back(&machine.geometry, way, line)
    })
}

/// `Evict`: the victim's line, with tag `tag`, leaves the way, which becomes
/// invalid; valid, tag, dirty and data go together.
fn evict(
    machine: &mut Machine,
    tag: Tag,
    set: SetIndex,
    way: WayIndex,
    dirty: bool,
    cause: Cause,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let line = machine.geometry.line_addr(tag, set);
    let Some(victim) = machine.cache.way_mut(set, way) else {
        return Err(lost(machine, set));
    };
    victim.slot = Slot::Invalid;
    machine.counters.add(Counter::Evictions, 1);
    let mut changed = line_cells(machine, set, way);
    if dirty {
        changed.push(ChangeRef::Dirty { set, way });
    }
    changed.push(ChangeRef::Counter {
        counter: Counter::Evictions,
    });
    let draft = Draft {
        addr: Some(Addr::new_unchecked(line.get())),
        set: Some(set),
        way: Some(way),
        changed,
        ..Draft::new(EventKind::Evict, Some(cause))
    };
    out.emit(machine, draft, || {
        explain::evict(&machine.geometry, way, line, tag)
    })
}

/// `Fill`: the line comes in from memory, clean, and memory records it as
/// touched. In timed mode it costs the miss penalty.
fn fill(
    machine: &mut Machine,
    location: Location,
    way: WayIndex,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let set = location.set;
    machine.memory.touch(location.line);
    let data = machine.memory.line(location.line);
    let Some(target) = machine.cache.way_mut(set, way) else {
        return Err(lost(machine, set));
    };
    target.slot = Slot::Valid {
        tag: location.tag,
        dirty: Dirtiness::Clean,
        data,
    };
    machine.counters.add(Counter::Fills, 1);
    if let Some(access) = &mut machine.access {
        access.victim = None;
        access.way = Some(way);
    }
    let mut changed = line_cells(machine, set, way);
    changed.push(ChangeRef::Counter {
        counter: Counter::Fills,
    });
    changed.extend(charge(machine, Cost::Miss));
    let draft = Draft {
        addr: Some(Addr::new_unchecked(location.line.get())),
        set: Some(set),
        way: Some(way),
        changed,
        ..Draft::new(EventKind::Fill, Some(Cause::LineSize))
    };
    out.emit(machine, draft, || {
        explain::fill(&machine.geometry, way, location.line)
    })
}

/// A load's `Complete`: the word goes into its register. Unless `ranked`,
/// a `Rank` event does not follow and this is the access's last event.
fn complete(
    machine: &mut Machine,
    addr: Addr,
    into: Register,
    location: Location,
    way: WayIndex,
    ranked: bool,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let word_bytes = machine.geometry.word_bytes();
    let start = word_start(machine, location.word);
    let value = match machine.cache.way(location.set, way).map(|held| &held.slot) {
        Some(Slot::Valid { data, .. }) => word_at(data, start, word_bytes),
        _ => return Err(lost(machine, location.set)),
    };
    let mut changed = Vec::new();
    if machine.registers.set(into, value) {
        changed.push(ChangeRef::Register { register: into });
    }
    if !ranked {
        finish(machine);
    }
    let draft = Draft {
        addr: Some(addr),
        set: Some(location.set),
        way: Some(way),
        changed,
        ..Draft::new(EventKind::Complete, None)
    };
    out.emit(machine, draft, || {
        explain::load(location.word, way, value, into)
    })
}

/// A store's result in the cache: `Write`, which under write-back also sets
/// the dirty bit, then under write-through `WriteThrough`, which writes the
/// word to memory too. Unless `ranked`, the last of these ends the access.
fn write(
    machine: &mut Machine,
    addr: Addr,
    value: u64,
    location: Location,
    way: WayIndex,
    ranked: bool,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let set = location.set;
    let word = machine.geometry.fit_word(value);
    let word_bytes = machine.geometry.word_bytes();
    let start = word_start(machine, location.word);
    let write_back = machine.config.cache.write_policy == WritePolicy::WriteBack;
    let target = Addr::new_unchecked(location.line.get() + start);
    let mut changed = Vec::new();
    let before = match machine.cache.way_mut(set, way).map(|held| &mut held.slot) {
        Some(Slot::Valid { data, dirty, .. }) => {
            let before = *dirty;
            if word_at(data, start, word_bytes) != word.get() {
                set_word_at(data, start, word_bytes, word.get());
                changed.push(ChangeRef::Word {
                    set,
                    way,
                    word: location.word,
                });
            }
            if write_back && *dirty == Dirtiness::Clean {
                *dirty = Dirtiness::Dirty;
                changed.push(ChangeRef::Dirty { set, way });
            }
            before
        }
        _ => return Err(lost(machine, set)),
    };
    if write_back && !ranked {
        finish(machine);
    }
    let draft = Draft {
        addr: Some(addr),
        set: Some(set),
        way: Some(way),
        changed,
        ..Draft::new(EventKind::Write, Some(Cause::WritePolicy))
    };
    out.emit(machine, draft, || {
        let note = if write_back {
            WriteNote::Back {
                before,
                stale: machine.memory.word(target) != word,
            }
        } else {
            WriteNote::Through
        };
        explain::write(&machine.geometry, addr, location.word, way, word, note)
    })?;
    if write_back {
        return Ok(());
    }

    let mut changed = to_memory(machine, target, word);
    machine.counters.add(Counter::Writethroughs, 1);
    changed.push(ChangeRef::Counter {
        counter: Counter::Writethroughs,
    });
    if !ranked {
        finish(machine);
    }
    let draft = Draft {
        addr: Some(addr),
        set: Some(set),
        way: Some(way),
        changed,
        ..Draft::new(EventKind::WriteThrough, Some(Cause::WritePolicy))
    };
    out.emit(machine, draft, || {
        explain::write_through(&machine.geometry, addr, word)
    })
}

/// A store that missed under write-no-allocate: `MemWrite` writes the word
/// to memory only, a write-around, and the access ends there. In timed mode
/// it costs the miss penalty.
fn write_around(
    machine: &mut Machine,
    addr: Addr,
    value: u64,
    location: Location,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let word = machine.geometry.fit_word(value);
    let start = word_start(machine, location.word);
    let target = Addr::new_unchecked(location.line.get() + start);
    let mut changed = to_memory(machine, target, word);
    machine.counters.add(Counter::Writearounds, 1);
    changed.push(ChangeRef::Counter {
        counter: Counter::Writearounds,
    });
    changed.extend(charge(machine, Cost::Miss));
    finish(machine);
    let draft = Draft {
        addr: Some(addr),
        set: Some(location.set),
        changed,
        ..Draft::new(EventKind::MemWrite, Some(Cause::AllocatePolicy))
    };
    out.emit(machine, draft, || {
        explain::write_around(&machine.geometry, addr, word)
    })
}

/// The rank moves the hit or the fill on `way` makes, planned before the
/// access's result so that the result knows whether it is the access's last
/// event. The policy moves the ranks and the moves are then held back: every
/// rank stays as it was until the `Rank` event makes them.
fn plan_ranks(
    machine: &mut Machine,
    set: SetIndex,
    way: WayIndex,
    hit: bool,
) -> Result<Vec<RankMove>, Halt> {
    let policy = machine.config.cache.replacement.policy();
    let Some(target) = machine.cache.set_mut(set) else {
        return Err(lost(machine, set));
    };
    let ways = target.ways_mut();
    let moves = if hit {
        policy.on_access(ways, way)
    } else {
        policy.on_fill(ways, way)
    };
    set_ranks(ways, &moves, |moved| moved.from);
    Ok(moves)
}

/// `Rank`: the planned rank moves are made, and the access ends.
fn rank(
    machine: &mut Machine,
    set: SetIndex,
    way: WayIndex,
    moves: &[RankMove],
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let Some(target) = machine.cache.set_mut(set) else {
        return Err(lost(machine, set));
    };
    set_ranks(target.ways_mut(), moves, |moved| moved.to);
    finish(machine);
    let draft = Draft {
        set: Some(set),
        way: Some(way),
        changed: moves
            .iter()
            .map(|moved| ChangeRef::Rank {
                set,
                way: moved.way,
            })
            .collect(),
        ..Draft::new(EventKind::Rank, Some(Cause::Replacement))
    };
    out.emit(machine, draft, || {
        let ranks: Vec<u32> = machine
            .cache
            .set(set)
            .map(|target| target.ways().iter().map(|held| held.rank).collect())
            .unwrap_or_default();
        explain::rank(machine.config.cache.replacement, set, way, &ranks)
    })
}

/// Gives each moved way the rank `rank` picks from its move.
fn set_ranks(ways: &mut [Way], moves: &[RankMove], rank: impl Fn(&RankMove) -> u32) {
    for moved in moves {
        let target = usize::try_from(moved.way.get())
            .ok()
            .and_then(|index| ways.get_mut(index));
        if let Some(target) = target {
            target.rank = rank(moved);
        }
    }
}

/// Which of timed mode's costs an event charges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cost {
    /// The hit time, which every access's tag compare costs.
    Hit,
    /// The miss penalty, which each transfer to or from memory costs.
    Miss,
}

/// Charges `cost` to the run's cycle counter in timed mode, and returns the
/// counter for the event's changed list; untimed, charges nothing and
/// returns `None`.
fn charge(machine: &mut Machine, cost: Cost) -> Option<ChangeRef> {
    let timing = machine.config.timing?;
    let cycles = match cost {
        Cost::Hit => timing.hit_cycles,
        Cost::Miss => timing.miss_cycles,
    };
    machine.counters.add(Counter::Cycles, cycles);
    Some(ChangeRef::Counter {
        counter: Counter::Cycles,
    })
}

/// Writes `word` to memory at `addr`, and returns the memory cell if its
/// value changed.
fn to_memory(machine: &mut Machine, addr: Addr, word: Word) -> Vec<ChangeRef> {
    let before = machine.memory.word(addr);
    machine.memory.write_word(addr, word);
    if before == word {
        Vec::new()
    } else {
        vec![ChangeRef::Memory { addr }]
    }
}

/// Marks the access as done: the event about to be emitted is its last, so
/// the machine it leaves is between entries.
fn finish(machine: &mut Machine) {
    if let Some(access) = &mut machine.access {
        access.done = true;
    }
}

/// The cause of choosing a victim, and of evicting it: the associativity
/// when there was no choice to make (an empty way, or a set's only way), the
/// replacement policy otherwise.
fn victim_cause(reason: VictimReason) -> Cause {
    match reason {
        VictimReason::Empty | VictimReason::OnlyWay => Cause::Associativity,
        VictimReason::LeastRecentlyUsed { .. }
        | VictimReason::FirstIn { .. }
        | VictimReason::Random => Cause::Replacement,
    }
}

/// The cells that change when a way takes or loses a line: its valid bit,
/// its tag and every word, whether or not a number happens to be equal,
/// since a different line now occupies the way.
fn line_cells(machine: &Machine, set: SetIndex, way: WayIndex) -> Vec<ChangeRef> {
    let words = (0..machine.geometry.words_per_line()).map(|word| ChangeRef::Word {
        set,
        way,
        word: WordIndex::new_unchecked(word),
    });
    [ChangeRef::Valid { set, way }, ChangeRef::Tag { set, way }]
        .into_iter()
        .chain(words)
        .collect()
}

/// The byte offset of each word within a line.
fn word_starts(machine: &Machine) -> impl Iterator<Item = u32> {
    let word_bytes = machine.geometry.word_bytes();
    (0..machine.geometry.words_per_line()).map(move |word| word * word_bytes)
}

/// The byte offset of `word` within its line.
fn word_start(machine: &Machine, word: WordIndex) -> u32 {
    word.get() * machine.geometry.word_bytes()
}

/// The error of finding no way where the geometry promises one. A set
/// without all its ways breaks I1; if the invariant check finds an earlier
/// problem, that one is named instead.
fn lost(machine: &Machine, set: SetIndex) -> Halt {
    Halt::Broken(machine.check_invariants().err().unwrap_or(
        InvariantViolation::RanksNotAPermutation {
            set,
            ways: machine.geometry.ways(),
        },
    ))
}
