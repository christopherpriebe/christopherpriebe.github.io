//! The helpers the integration tests share: builders of the ops and units
//! the tests apply, a session with ops applied, the comma-grouped numbers
//! messages print, and the settings of a golden snapshot.
//!
//! Each test crate includes this module and uses some of it, so the module
//! expects the helpers it leaves unused. It sits in a module compiled for
//! tests only, where a failed `unwrap` is a failed test, as in the tests
//! themselves.

#![expect(dead_code, reason = "each test crate uses a subset of the helpers")]

use cache_explorer_core::{
    AccessKind, Addr, CacheConfig, Config, EventPos, LineAddr, Op, ProgramFormat, Session, Word,
};
use insta::{Comparator, DefaultComparator, Settings, Snapshot};

/// `value` as an address, as one read from JSON is.
pub(crate) fn addr(value: u32) -> Addr {
    serde_json::from_value(value.into()).unwrap()
}

/// `value` as a line address, as one read from JSON is.
pub(crate) fn line(value: u32) -> LineAddr {
    serde_json::from_value(value.into()).unwrap()
}

/// `value` as a data value, as one read from JSON is.
pub(crate) fn word(value: u64) -> Word {
    serde_json::from_value(value.into()).unwrap()
}

/// Position `value`, as one read from JSON is.
pub(crate) fn pos(value: u32) -> EventPos {
    serde_json::from_value(value.into()).unwrap()
}

/// A load of `at` issued by hand.
pub(crate) fn load(at: u32) -> Op {
    Op::Issue {
        kind: AccessKind::Read,
        addr: addr(at),
        value: None,
    }
}

/// A store of `value` to `at` issued by hand.
pub(crate) fn store(at: u32, value: u64) -> Op {
    Op::Issue {
        kind: AccessKind::Write,
        addr: addr(at),
        value: Some(word(value)),
    }
}

/// A load of the assembly program `source`.
pub(crate) fn program(source: &str) -> Op {
    Op::Program {
        source: source.to_owned(),
        format: ProgramFormat::Asm,
    }
}

/// `Config::default()` with another cache geometry.
pub(crate) fn with_cache(size_bytes: u32, line_bytes: u32, associativity: u32) -> Config {
    let default = Config::default();
    Config {
        cache: CacheConfig {
            size_bytes,
            line_bytes,
            associativity,
            ..default.cache
        },
        ..default
    }
}

/// A session on `config` with `ops` applied, which must all be accepted.
pub(crate) fn session(config: Config, ops: impl IntoIterator<Item = Op>) -> Session {
    let mut session = Session::with_config(config).unwrap();
    for op in ops {
        session.apply(op).unwrap();
    }
    session
}

/// Applies each of `ops`, which must all be accepted.
#[track_caller]
pub(crate) fn apply_all(session: &mut Session, ops: impl IntoIterator<Item = Op>) {
    for op in ops {
        let accepted = session.apply(op.clone());
        assert!(accepted.is_ok(), "{op:?}: {accepted:?}");
    }
}

/// `n` with a comma before every group of three digits: how messages and
/// labels print limits and configuration values.
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

/// Compares a snapshot's header as well as its body, so an edited
/// description is reviewed again. insta compares only the body by default,
/// so an edited description would leave the committed header stale while
/// every test passed.
#[derive(Clone)]
struct WithHeader;

impl Comparator for WithHeader {
    fn matches(&self, reference: &Snapshot, test: &Snapshot) -> bool {
        DefaultComparator.matches_fully(reference, test)
    }

    fn dyn_clone(&self) -> Box<dyn Comparator> {
        Box::new(self.clone())
    }
}

/// The settings of a golden snapshot: `description` in its header, which is
/// compared with the body, and no expression. The test file that owns the
/// snapshot binds them around its own `insta::assert_snapshot!`, since insta
/// files a snapshot under the file that asserts it.
pub(crate) fn golden_settings(description: &str) -> Settings {
    let mut settings = Settings::clone_current();
    settings.set_description(description);
    settings.set_omit_expression(true);
    settings.set_comparator(Box::new(WithHeader));
    settings
}
