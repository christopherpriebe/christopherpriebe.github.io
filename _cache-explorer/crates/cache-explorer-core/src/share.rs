//! Share strings and saved files: a session's log as a document, the text a
//! link or a file holds, and opening one again.
//!
//! The document holds the first run's configuration, with the mode the
//! student chose; every op of the log, the undone ones after the cursor
//! included; and where the cursor was, relative to an op, so that a change
//! of wording, or of how many events another op yields, cannot move it. A
//! saved file is the document as compact JSON, and a share string, the text
//! after `#s=`, is the same JSON deflated and in base64url, so a file holds
//! whatever a link can. A session keeps its saved file within the most a
//! file or a link may hold, refusing any op that would take it further, so
//! every session can be saved, and opened again. It writes a link only when
//! a link can open it: a longer run is a [`LinkTooLong`], whose message
//! offers the saved file instead.
//!
//! Opening a link or a file bounds the text first, then decodes it, checks
//! the document's format version and sizes, and validates the first run's
//! configuration, all before anything replays. [`ShareLoad`] then replays the
//! ops a slice at a time, as the session would have applied them, its event
//! limit included, so the page can draw the loading state between slices.
//! What cannot be opened is a [`LoadError`], and for the page a
//! [`LoadFailure`]: the message with the offending field in mono, the
//! document's contents, and the ways forward there are, a repaired
//! configuration or the actions before the op that failed. A link or a file
//! calls its ops actions, since a step is one micro-event.

use base64::engine::general_purpose::{URL_SAFE_NO_PAD, URL_SAFE_NO_PAD_INDIFFERENT};
use base64::Engine as _;
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::{decompress_to_vec_with_limit, TINFLStatus};
use serde::de::{Error as _, Unexpected};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::config::{Config, ConfigError};
use crate::event::{EventKind, MicroEvent};
use crate::explain;
use crate::format;
use crate::limits::{
    LOAD_SLICE_EVENTS, MAX_PROGRAM_BYTES, MAX_SHARE_JSON_BYTES, MAX_SHARE_OPS,
    MAX_SHARE_TEXT_BYTES, SESSION_EVENT_LIMIT,
};
use crate::module::{code_spans, Segments, TextSegment};
use crate::op::{Op, OpError};
use crate::session::{position_in_op, Loader, ReplayError, Session};
use crate::units::EventPos;

/// The format version of the share documents this crate writes, and the
/// newest it reads. It increases with any change to the document's shape, or
/// to what the same ops replay to.
pub const SCHEMA_VERSION: u32 = 1;

/// The deflate level of a share string: the shortest links, and always the
/// same string for the same document.
const DEFLATE_LEVEL: u8 = 9;

/// A session's log as a link or a file holds it.
///
/// Its own keys are spelled as the spec spells them, `schema_version`,
/// `config`, `ops` and `at`; what they hold is camelCase, as everywhere
/// else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareDoc {
    /// The format version it was written in: [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The first run's configuration; its `advanced` holds the mode chosen
    /// when the document was written.
    pub config: Config,
    /// The whole log, the undone ops after the cursor included.
    pub ops: Vec<Op>,
    /// Where the cursor was; without it, the log opens at its end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<ShareAt>,
}

/// Where the cursor was, relative to an op, so that a change to another op's
/// events cannot move it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareAt {
    /// The op, from 0.
    pub op: u32,
    /// After how many of the op's events, from 1; 0 is just before the op,
    /// so `{ op: 0, event: 0 }` is position 0 of a log with ops. A count
    /// beyond the op's events is its end, and an op beyond the log the end
    /// of the log.
    pub event: u32,
}

/// Why the text of a link or a file is not a document that can be read.
/// `Display` is the message the student reads.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum EncodingError {
    /// The link is not base64url text.
    #[error("The link's text is not valid base64url; it may have been cut short or changed.")]
    Base64,
    /// The link's bytes are not a deflate stream.
    #[error("The link's data does not unpack; it may have been cut short or changed.")]
    Deflate,
    /// The link unpacks to bytes that are not UTF-8 text.
    #[error("The link's data is not text; it may have been cut short or changed.")]
    Utf8,
    /// The text, the JSON, the log or a program is larger than can be
    /// opened.
    #[error(transparent)]
    TooLarge(#[from] TooLarge),
}

/// What is larger than a link or a file may hold. These bound the text, not
/// the work: the replay's work is bounded by the session's event limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum TooLarge {
    /// The share string is longer than [`MAX_SHARE_TEXT_BYTES`].
    #[error(
        "The link is {} bytes long; at most {} bytes can be opened.",
        format::setting(*.bytes),
        format::setting(MAX_SHARE_TEXT_BYTES)
    )]
    Text {
        /// Its length, in bytes.
        bytes: u32,
    },
    /// The JSON is larger than [`MAX_SHARE_JSON_BYTES`].
    #[error("{}", json_too_large(*.bytes))]
    Json {
        /// Its size in bytes: a file's; none for a link, whose inflating
        /// stops at the limit.
        bytes: Option<u32>,
    },
    /// The log has more than [`MAX_SHARE_OPS`] ops.
    #[error(
        "The saved run has {} actions; at most {} can be opened.",
        format::setting(*.ops),
        format::setting(MAX_SHARE_OPS)
    )]
    Ops {
        /// The ops it has.
        ops: u32,
    },
    /// A `Program` op's source is longer than [`MAX_PROGRAM_BYTES`].
    #[error(
        "A program in the saved run is {} bytes; programs of at most {} bytes can be opened.",
        format::setting(*.bytes),
        format::setting(MAX_PROGRAM_BYTES)
    )]
    Program {
        /// The op, from 0.
        op: u32,
        /// The source's length, in bytes.
        bytes: u32,
    },
}

/// Why a session's run cannot be written as a link: its share string would
/// be longer than [`MAX_SHARE_TEXT_BYTES`], so no link could open it. Its
/// JSON is never the reason: the session keeps its saved file, the same
/// JSON a link unpacks to, within [`MAX_SHARE_JSON_BYTES`], so a saved file
/// always holds the run. `Display` is what Share says instead of copying a
/// link, and offers that file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error(
    "This run is too long for a link: its link would be {} bytes long, and at most {} bytes can \
     be opened. Save it as a file instead.",
    format::setting(*.bytes),
    format::setting(MAX_SHARE_TEXT_BYTES)
)]
pub struct LinkTooLong {
    /// The share string's length, in bytes.
    pub bytes: u32,
}

/// Why a link or a file cannot be opened. `Display` is the message the
/// student reads, and the text the throwing constructors throw.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum LoadError {
    /// The text cannot be decoded, or is larger than can be opened.
    #[error(transparent)]
    Encoding(#[from] EncodingError),
    /// The JSON is not a document of this format: `Line 1, column 57:
    /// unknown field `ways`, …`.
    #[error("{}{message}", json_place(*.line, *.column))]
    Json {
        /// The line of the JSON text, from 1.
        line: u32,
        /// The column, from 1.
        column: u32,
        /// serde's message, as serde words it.
        message: String,
    },
    /// The document was written by a newer version of the tool.
    #[error(
        "This run was saved by a newer version of the Interactive Cache Explorer (format {}); \
         this page reads format {}. Reload the page to update.",
        format::plain(*.found),
        format::plain(*.supported)
    )]
    NewerSchema {
        /// The document's format version.
        found: u32,
        /// The newest this crate reads: [`SCHEMA_VERSION`].
        supported: u32,
    },
    /// The first run's configuration is invalid, so nothing replays.
    #[error("{error}{}", not_replayed_note(*.total_ops))]
    InitialConfig {
        /// The first rule it breaks.
        error: ConfigError,
        /// The ops of the log, none of which replayed.
        total_ops: u32,
    },
    /// An op is refused by the machine it applies to, as the session would
    /// have refused it, the session's event limit included. The ops before
    /// it replayed.
    #[error("{error}{}", not_replayed_note(*.not_replayed))]
    Op {
        /// The op, from 0.
        index: u32,
        /// Why it is refused.
        error: OpError,
        /// The ops after it, which were not replayed.
        not_replayed: u32,
    },
    /// The session was asked for before the load finished.
    #[error("The run has not finished loading.")]
    Unfinished,
    /// The session was asked for again, after it was taken.
    #[error("The run has already been opened.")]
    Taken,
}

/// What the page shows for a link or a file that cannot be opened, and the
/// ways forward that exist for it besides starting fresh.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct LoadFailure {
    /// "Could not open link", or "Could not open file".
    pub title: String,
    /// "This link doesn't describe a cache the Interactive Cache Explorer
    /// can build."
    pub lead: String,
    /// Why, with the offending field in mono, and how many actions were not
    /// replayed: `associativity: 3` then "is not a power of two. … The 6
    /// actions after it were not replayed."
    pub message: Vec<TextSegment>,
    /// The document as pretty JSON, for "Show link contents"; the text as
    /// it came when it is not JSON; absent when it could not be decoded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    /// The same document with the configuration that failed repaired:
    /// "Open with 2-way instead"; absent when a link cannot hold it, as for
    /// a file larger than a link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<LoadOption>,
    /// The ops before the one that failed, opening at their end: "Open the
    /// first 8 actions"; absent when a link cannot hold them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<LoadOption>,
}

/// An action that opens another document instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct LoadOption {
    /// The action's label.
    pub label: String,
    /// The document it opens, as a share string.
    pub share: String,
}

/// How a load is going, after a slice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LoadProgress {
    /// More is to replay; the summary counts what has replayed so far.
    Replaying {
        /// The run so far, for the loading state.
        summary: ShareSummary,
    },
    /// Every op has replayed, and the session is ready to take.
    Done {
        /// The run as it opens.
        summary: ShareSummary,
    },
    /// The link or file cannot be opened.
    Failed {
        /// What the page shows.
        failure: LoadFailure,
    },
}

/// A shared run in a line: the opening run's configuration, its steps and
/// where it opens, for the loading state and for Share's status line. The
/// opening run is the one holding the cursor, or the last.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ShareSummary {
    /// The opening run's configuration: "2-way LRU · 128 B".
    pub config_summary: String,
    /// The runs in the log.
    pub runs: u32,
    /// The ops in the log.
    pub op_count: u32,
    /// The opening run's steps; while replaying, those replayed so far.
    pub step_count: u32,
    /// The step the run opens at, 0 before the first; while replaying, the
    /// steps so far until the replay has passed it.
    pub opening_step: u32,
    /// "2-way LRU · 128 B · 14 steps, opening at step 9."
    pub text: String,
    /// "Link copied. It encodes the configuration and all 14 steps, and
    /// opens at step 9."
    pub copied_text: String,
    /// One byte per step of the opening run replayed so far, as
    /// [`Session::ticks`] gives them.
    pub ticks: Vec<u8>,
}

/// A share string or a file being opened: decoded and checked at once, then
/// replayed in slices, so the page can draw the loading state between them.
#[derive(Debug)]
pub struct ShareLoad {
    /// Where the load is.
    state: LoadState,
}

/// Where a load is.
#[derive(Debug)]
enum LoadState {
    /// Replaying.
    Replaying(Box<Replaying>),
    /// Every op has replayed; the session, until it is taken.
    Done {
        /// The run as it opens.
        summary: ShareSummary,
        /// The session; none once taken.
        session: Option<Box<Session>>,
    },
    /// It cannot be opened.
    Failed {
        /// Why.
        error: LoadError,
        /// What the page shows.
        failure: Box<LoadFailure>,
    },
}

/// A document being replayed.
#[derive(Debug)]
struct Replaying {
    /// Whether it came from a link or a file.
    source: Source,
    /// Its first run's configuration.
    config: Config,
    /// Where it opens.
    at: Option<ShareAt>,
    /// The replay.
    loader: Loader,
    /// The opening run so far.
    opening: Opening,
}

/// Where a document came from, which the failure's wording names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// A share link.
    Link,
    /// A saved file.
    File,
}

/// The opening run of a document being replayed, as far as it has replayed:
/// its steps, their ticks, and the step it opens at once the replay has
/// passed it.
#[derive(Debug)]
struct Opening {
    /// The run, from 1.
    run: u32,
    /// Its configuration.
    config: Config,
    /// The runs in the document.
    runs: u32,
    /// The ops in the document.
    op_count: u32,
    /// One byte per step of the run replayed so far.
    ticks: Vec<u8>,
    /// Where the ticks of the entry being replayed start.
    entry_start: usize,
    /// Whether the entry being replayed has missed.
    missed: bool,
    /// The events already looked at.
    seen: usize,
    /// The step the run opens at, once the replay has passed it.
    step: Option<u32>,
}

/// A document read only as far as its format version.
#[derive(Deserialize)]
struct Version {
    /// The document's format version, from 1.
    #[serde(deserialize_with = "format_version")]
    schema_version: u32,
}

/// Why a document could not be read, with the document when it was read
/// before the failure.
struct Unread {
    /// Why.
    error: LoadError,
    /// The document, if it was read.
    doc: Option<ShareDoc>,
}

impl ShareDoc {
    /// The document as a share string: its compact JSON, deflated at level 9
    /// and written in base64url without padding. The same document always
    /// gives the same string. Whether a link can open it is not checked
    /// here: a document read from a file may be too large for one, and
    /// [`Session::to_share_string`] checks a session's.
    pub fn to_share_string(&self) -> String {
        encode(&to_json(self))
    }

    /// The document as a saved file holds it: compact JSON, the text a
    /// share string deflates, so a file holds whatever a link can. A file
    /// read may be pretty JSON; it is written compact.
    pub fn to_json(&self) -> String {
        to_json(self)
    }

    /// The document a share string holds, decoded and checked, without
    /// replaying anything: its size, its encoding, its format version, and
    /// the sizes of its log and programs. `=` padding is accepted.
    ///
    /// # Errors
    ///
    /// The [`LoadError`] of the first check that fails.
    pub fn from_share_string(s: &str) -> Result<ShareDoc, LoadError> {
        let text = link_text(s)?;
        read(&text).map_err(|unread| unread.error)
    }

    /// The document a saved file holds, checked as
    /// [`ShareDoc::from_share_string`] checks one, its size included.
    ///
    /// # Errors
    ///
    /// The [`LoadError`] of the first check that fails.
    pub fn from_json(json: &str) -> Result<ShareDoc, LoadError> {
        file_size(json)?;
        read(json).map_err(|unread| unread.error)
    }
}

impl LoadError {
    /// The message as segments, with what it quotes in mono: the offending
    /// field of a configuration and its value, as in `associativity: 3`, and
    /// the names serde and the op messages quote between backticks. Then, for
    /// a configuration or an op that failed, how many actions were not
    /// replayed.
    pub(crate) fn segments(&self) -> Vec<TextSegment> {
        match self {
            LoadError::InitialConfig { error, total_ops } => Segments::new()
                .then(error.segments())
                .plain(&not_replayed_note(*total_ops))
                .into_vec(),
            LoadError::Op {
                error,
                not_replayed: after,
                ..
            } => {
                let segments = match error {
                    OpError::Config(error) => error.segments(),
                    error => code_spans(&error.to_string()),
                };
                Segments::new()
                    .then(segments)
                    .plain(&not_replayed_note(*after))
                    .into_vec()
            }
            LoadError::Json {
                line,
                column,
                message,
            } => Segments::new()
                .plain(&json_place(*line, *column))
                .then(code_spans(message))
                .into_vec(),
            error => code_spans(&error.to_string()),
        }
    }
}

impl From<TooLarge> for LoadError {
    fn from(error: TooLarge) -> Self {
        LoadError::Encoding(error.into())
    }
}

impl ShareLoad {
    /// Opens a share string: decodes it and checks it, its format version
    /// and its first run's configuration, and replays nothing yet. A link
    /// that cannot be opened is reported by [`ShareLoad::advance`].
    pub fn from_share_string(s: &str) -> ShareLoad {
        match link_text(s) {
            Ok(text) => Self::open(Source::Link, &text),
            Err(error) => Self::failed(Source::Link, &error.into(), None, None),
        }
    }

    /// Opens a saved file's JSON as [`ShareLoad::from_share_string`] opens a
    /// link.
    pub fn from_json(json: &str) -> ShareLoad {
        match file_size(json) {
            Ok(()) => Self::open(Source::File, json),
            Err(error) => Self::failed(Source::File, &error.into(), None, None),
        }
    }

    /// Replays the next slice: at least one entry, then more while another
    /// entry is sure to fit within `max_events` events, stopping at an entry
    /// boundary. Once the load is done or has failed, every call answers the
    /// same.
    pub fn advance(&mut self, max_events: u32) -> LoadProgress {
        let state = std::mem::replace(&mut self.state, LoadState::taken());
        self.state = state.advanced(max_events);
        self.progress()
    }

    /// Replays the next slice as [`ShareLoad::advance`] does, within
    /// [`LOAD_SLICE_EVENTS`] events.
    pub fn advance_slice(&mut self) -> LoadProgress {
        self.advance(LOAD_SLICE_EVENTS)
    }

    /// The loaded session, its cursor where the document placed it, moved
    /// out of the load.
    ///
    /// # Errors
    ///
    /// The [`LoadError`] of a load that failed; [`LoadError::Unfinished`]
    /// before [`ShareLoad::advance`] has returned `Done`, and
    /// [`LoadError::Taken`] once the session has been taken.
    pub fn take_session(&mut self) -> Result<Session, LoadError> {
        match &mut self.state {
            LoadState::Done { session, .. } => session
                .take()
                .map(|session| *session)
                .ok_or(LoadError::Taken),
            LoadState::Replaying(_) => Err(LoadError::Unfinished),
            LoadState::Failed { error, .. } => Err(error.clone()),
        }
    }

    /// Replays whatever is left in one slice and takes the session.
    fn into_session(mut self) -> Result<Session, LoadError> {
        while let LoadProgress::Replaying { .. } = self.advance(u32::MAX) {}
        self.take_session()
    }

    /// A load of `text`, the JSON of a document from `source`.
    fn open(source: Source, text: &str) -> ShareLoad {
        let doc = match read(text) {
            Ok(doc) => doc,
            Err(unread) => {
                return Self::failed(source, &unread.error, unread.doc.as_ref(), Some(text));
            }
        };
        let session = match Session::with_config(doc.config.clone()) {
            Ok(session) => session,
            Err(error) => {
                let error = LoadError::InitialConfig {
                    error,
                    total_ops: count(doc.ops.len()),
                };
                return Self::failed(source, &error, Some(&doc), Some(text));
            }
        };
        let opening = Opening::new(&doc);
        let ShareDoc {
            config, ops, at, ..
        } = doc;
        ShareLoad {
            state: LoadState::Replaying(Box::new(Replaying {
                source,
                config,
                at,
                loader: Loader::new(session, ops),
                opening,
            })),
        }
    }

    /// A load that has failed with `error`, on `doc` if it was read, from
    /// `text` if it was decoded.
    fn failed(
        source: Source,
        error: &LoadError,
        doc: Option<&ShareDoc>,
        text: Option<&str>,
    ) -> ShareLoad {
        ShareLoad {
            state: LoadState::failed(source, error, doc, text),
        }
    }

    /// How the load is going.
    fn progress(&self) -> LoadProgress {
        match &self.state {
            LoadState::Replaying(replaying) => LoadProgress::Replaying {
                summary: replaying.opening.summary(),
            },
            LoadState::Done { summary, .. } => LoadProgress::Done {
                summary: summary.clone(),
            },
            LoadState::Failed { failure, .. } => LoadProgress::Failed {
                failure: (**failure).clone(),
            },
        }
    }
}

impl LoadState {
    /// A load whose session has been taken: what a load is while it moves
    /// from one state to the next.
    fn taken() -> Self {
        LoadState::Done {
            summary: ShareSummary::default(),
            session: None,
        }
    }

    /// A load that has failed with `error`, on `doc` if it was read, from
    /// `text` if it was decoded.
    fn failed(
        source: Source,
        error: &LoadError,
        doc: Option<&ShareDoc>,
        text: Option<&str>,
    ) -> Self {
        LoadState::Failed {
            failure: Box::new(failure(source, error, doc, text)),
            error: error.clone(),
        }
    }

    /// This state once the next slice has replayed. A load that is done or
    /// has failed stays as it is.
    fn advanced(self, max_events: u32) -> Self {
        match self {
            LoadState::Replaying(mut replaying) => match replaying.loader.advance(max_events) {
                Ok(false) => {
                    replaying.opening.observe(&replaying.loader, replaying.at);
                    LoadState::Replaying(replaying)
                }
                Ok(true) => Self::finish(*replaying),
                Err(error) => Self::fail(*replaying, error),
            },
            settled @ (LoadState::Done { .. } | LoadState::Failed { .. }) => settled,
        }
    }

    /// The state of a replay that has replayed every op: the session, with
    /// its cursor where the document places it.
    fn finish(replaying: Replaying) -> Self {
        let Replaying { at, loader, .. } = replaying;
        let cursor = match at {
            Some(at) => position_in_op(loader.op_ends(), at.op, at.event),
            None => end_of(loader.op_ends()),
        };
        let session = loader.finish(cursor);
        LoadState::Done {
            summary: session.share_summary(),
            session: Some(Box::new(session)),
        }
    }

    /// The state of a replay whose op was refused with `error`.
    fn fail(replaying: Replaying, error: ReplayError) -> Self {
        let Replaying {
            source,
            config,
            at,
            loader,
            ..
        } = replaying;
        let ops = loader.into_ops();
        let total = count(ops.len());
        let error = match error {
            ReplayError::Op { index, error } => LoadError::Op {
                index: index.get(),
                error,
                not_replayed: total.saturating_sub(index.get()).saturating_sub(1),
            },
            ReplayError::InitialConfig(error) => LoadError::InitialConfig {
                error,
                total_ops: total,
            },
        };
        let doc = ShareDoc {
            schema_version: SCHEMA_VERSION,
            config,
            ops,
            at,
        };
        Self::failed(source, &error, Some(&doc), None)
    }
}

impl Opening {
    /// The opening run of `doc`, before anything has replayed: the run
    /// holding the document's cursor, or the last.
    fn new(doc: &ShareDoc) -> Self {
        let len = doc.ops.len();
        // The ops up to the cursor: an op the cursor is inside or at the end
        // of has started, and a new run with it.
        let reached = match doc.at {
            None => len,
            Some(at) => {
                let op = usize::try_from(at.op).unwrap_or(usize::MAX);
                if op >= len {
                    len
                } else if at.event == 0 {
                    op
                } else {
                    op.saturating_add(1)
                }
            }
        };
        // Each `Config` op starts a run.
        let runs = |ops: &[Op]| {
            let configs = ops
                .iter()
                .filter(|op| matches!(op, Op::Config { .. }))
                .count();
            count(configs).saturating_add(1)
        };
        let before = doc.ops.get(..reached).unwrap_or_default();
        let config = before
            .iter()
            .rev()
            .find_map(|op| match op {
                Op::Config { config } => Some(config),
                _ => None,
            })
            .unwrap_or(&doc.config)
            .clone();
        Self {
            run: runs(before),
            config,
            runs: runs(&doc.ops),
            op_count: count(len),
            ticks: Vec::new(),
            entry_start: 0,
            missed: false,
            seen: 0,
            step: None,
        }
    }

    /// Takes in the events `loader` has replayed since the last look, and
    /// the step the run opens at once the replay has passed `at`.
    fn observe(&mut self, loader: &Loader, at: Option<ShareAt>) {
        let events = loader.events();
        for event in events.get(self.seen..).unwrap_or_default() {
            self.tick(event);
        }
        self.seen = events.len();
        if self.step.is_some() {
            return;
        }
        // The place is known once its op has replayed, or every op has, or
        // once the op being replayed has replayed as far as the place.
        let op_ends = loader.op_ends();
        let replayed = count(op_ends.len());
        let done = replayed == self.op_count;
        let target = match at {
            Some(at) if done || at.op < replayed => Some(position_in_op(op_ends, at.op, at.event)),
            Some(at) if at.op == replayed => reached(op_ends, at.event, events.len()),
            None if done => Some(end_of(op_ends)),
            _ => None,
        };
        if let Some(target) = target {
            let upto = usize::try_from(target.get()).unwrap_or(usize::MAX);
            let steps = events
                .get(..upto.min(events.len()))
                .unwrap_or_default()
                .iter()
                .filter(|event| event.run == self.run && event.kind != EventKind::Setup)
                .count();
            self.step = Some(count(steps));
        }
    }

    /// Takes in one event: a step of the run is a tick, first steps of
    /// entries marked, and every step of an entry that missed.
    fn tick(&mut self, event: &MicroEvent) {
        if event.run != self.run || event.kind == EventKind::Setup {
            return;
        }
        if event.seq == 1 {
            self.entry_start = self.ticks.len();
            self.missed = false;
        }
        if event.kind == EventKind::Miss {
            self.missed = true;
            for tick in self.ticks.get_mut(self.entry_start..).unwrap_or_default() {
                *tick |= MISS_TICK;
            }
        }
        let first = if event.seq == 1 { FIRST_TICK } else { 0 };
        let miss = if self.missed { MISS_TICK } else { 0 };
        self.ticks.push(first | miss);
    }

    /// The run so far.
    fn summary(&self) -> ShareSummary {
        let steps = count(self.ticks.len());
        ShareSummary::new(
            &self.config,
            self.runs,
            self.op_count,
            steps,
            self.step.unwrap_or(steps),
            self.ticks.clone(),
        )
    }
}

/// The bit of a tick set on an entry's first step.
const FIRST_TICK: u8 = 1;

/// The bit of a tick set on every step of an entry that missed.
const MISS_TICK: u8 = 2;

impl ShareSummary {
    /// The summary of a log of `runs` runs and `op_count` ops whose opening
    /// run has the configuration `config`, `steps` steps and `ticks`, and
    /// opens at step `opening`.
    fn new(
        config: &Config,
        runs: u32,
        op_count: u32,
        steps: u32,
        opening: u32,
        ticks: Vec<u8>,
    ) -> Self {
        let config_summary = config.short_summary();
        Self {
            text: explain::share_text(&config_summary, runs, steps, opening),
            copied_text: explain::share_copied(runs, steps, opening),
            config_summary,
            runs,
            op_count,
            step_count: steps,
            opening_step: opening,
            ticks,
        }
    }
}

impl Session {
    /// The session as a share document: the first run's configuration with
    /// the mode chosen, every op, the undone ones included, and where the
    /// cursor is. An empty log has no op to place the cursor by, so its
    /// document has no `at`.
    pub fn share_doc(&self) -> ShareDoc {
        ShareDoc {
            schema_version: SCHEMA_VERSION,
            config: self.share_config(),
            ops: self.ops().to_vec(),
            at: self.share_at(),
        }
    }

    /// The session as a share string, as [`ShareDoc::to_share_string`]
    /// writes its document, when a link can open it: the string within
    /// [`MAX_SHARE_TEXT_BYTES`]. Its JSON, the saved file's, is within
    /// [`MAX_SHARE_JSON_BYTES`] already, since the session refuses any op
    /// that would take the file past it, wherever the cursor goes. The ops'
    /// JSON is kept as they are appended, so after a cursor move only the
    /// configuration and the cursor are serialized again.
    ///
    /// # Errors
    ///
    /// [`LinkTooLong`] when the string is too long for a link; the saved
    /// file holds the run.
    pub fn to_share_string(&self) -> Result<String, LinkTooLong> {
        let share = encode(&self.share_json());
        if exceeds(share.len(), MAX_SHARE_TEXT_BYTES) {
            return Err(LinkTooLong {
                bytes: count(share.len()),
            });
        }
        Ok(share)
    }

    /// The session as a saved file holds it: its document as compact JSON,
    /// the JSON its share string deflates, written from the ops' JSON the
    /// session keeps. It is never larger than [`MAX_SHARE_JSON_BYTES`].
    pub fn to_json(&self) -> String {
        self.share_json()
    }

    /// The size of the file [`Session::to_json`] writes, in bytes, without
    /// writing it: the ops' JSON, whose size is kept as they are appended,
    /// and the first run's configuration and the cursor's place.
    pub fn to_json_len(&self) -> u32 {
        let doc = ShareDoc {
            schema_version: SCHEMA_VERSION,
            config: self.share_config(),
            ops: Vec::new(),
            at: self.share_at(),
        };
        count(
            to_json(&doc)
                .len()
                .saturating_add(self.saved_ops().text().len()),
        )
    }

    /// The share summary of the session as it stands: the displayed run's
    /// configuration, steps and step, for Share's status line.
    pub fn share_summary(&self) -> ShareSummary {
        let timeline = self.timeline();
        ShareSummary::new(
            self.config(),
            timeline.runs,
            self.op_count(),
            timeline.step_count,
            timeline.step,
            self.ticks(),
        )
    }

    /// The session a share string holds: a [`ShareLoad`] replayed to the
    /// end.
    ///
    /// # Errors
    ///
    /// The [`LoadError`] of a link that cannot be opened.
    pub fn from_share_string(s: &str) -> Result<Session, LoadError> {
        ShareLoad::from_share_string(s).into_session()
    }

    /// The session a saved file holds, as [`Session::from_share_string`]
    /// opens a link.
    ///
    /// # Errors
    ///
    /// The [`LoadError`] of a file that cannot be opened.
    pub fn from_json(json: &str) -> Result<Session, LoadError> {
        ShareLoad::from_json(json).into_session()
    }

    /// The first run's configuration with the mode chosen.
    fn share_config(&self) -> Config {
        Config {
            advanced: self.mode_chosen(),
            ..self.initial().clone()
        }
    }

    /// Where the cursor is, relative to the op holding it.
    fn share_at(&self) -> Option<ShareAt> {
        self.cursor_in_op().map(|(op, event)| ShareAt {
            op: op.get(),
            event,
        })
    }

    /// The compact JSON of [`Session::share_doc`], from the ops' JSON the
    /// session keeps: the same text `serde_json` writes for the document.
    fn share_json(&self) -> String {
        let mut json = format!(
            "{{\"schema_version\":{SCHEMA_VERSION},\"config\":{},\"ops\":[{}]",
            to_json(&self.share_config()),
            self.saved_ops().text()
        );
        if let Some(at) = self.share_at() {
            json.push_str(",\"at\":");
            json.push_str(&to_json(&at));
        }
        json.push('}');
        json
    }
}

/// `op` as a share document writes it.
pub(crate) fn op_json(op: &Op) -> String {
    to_json(op)
}

/// A session's ops as its saved file writes them, kept as ops are appended
/// and dropped, so that a file or a link written after a cursor move
/// serializes only the configuration and the cursor, and the file's size is
/// known op by op without writing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SavedOps {
    /// The most bytes the file takes besides its ops' JSON, whatever the
    /// mode and wherever the cursor is: see [`frame`].
    frame: usize,
    /// Each op's JSON, in order, with a comma between two: what the
    /// document's `ops` array holds between its brackets.
    text: String,
    /// Where each op's JSON ends in `text`.
    ends: Vec<usize>,
}

/// The room a saved file has for more ops, taken as ops are checked in
/// turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileRoom {
    /// The bytes left under [`MAX_SHARE_JSON_BYTES`].
    left: usize,
    /// Whether the file holds an op already, from which a comma separates
    /// the next.
    after_op: bool,
}

impl SavedOps {
    /// No op yet, for a session whose first run's configuration is
    /// `initial`.
    pub(crate) fn new(initial: &Config) -> Self {
        Self {
            frame: frame(initial),
            text: String::new(),
            ends: Vec::new(),
        }
    }

    /// The ops' JSON, a comma between two.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Keeps the JSON of the first `ops` ops only.
    pub(crate) fn truncate(&mut self, ops: usize) {
        // Each end is where an op's JSON ended as it was appended, so it
        // falls between two characters.
        self.text.truncate(self.len_of(ops));
        self.ends.truncate(ops);
    }

    /// Appends the next op's JSON, `json`.
    pub(crate) fn push(&mut self, json: &str) {
        if !self.ends.is_empty() {
            self.text.push(',');
        }
        self.text.push_str(json);
        self.ends.push(self.text.len());
    }

    /// The room the file has for ops after its first `ops`, those after
    /// them not counted, since an op appended there drops them.
    pub(crate) fn room_after(&self, ops: usize) -> FileRoom {
        let used = self.frame.saturating_add(self.len_of(ops));
        let limit = usize::try_from(MAX_SHARE_JSON_BYTES).unwrap_or(usize::MAX);
        FileRoom {
            left: limit.saturating_sub(used),
            after_op: ops.min(self.ends.len()) > 0,
        }
    }

    /// The bytes of the first `ops` ops' JSON, the commas between them
    /// included.
    fn len_of(&self, ops: usize) -> usize {
        ops.min(self.ends.len())
            .checked_sub(1)
            .and_then(|last| self.ends.get(last))
            .copied()
            .unwrap_or(0)
    }
}

impl FileRoom {
    /// The room of the saved file of a log with no op yet over a first run
    /// of `initial`: where the fold and a load start.
    pub(crate) fn new(initial: &Config) -> Self {
        SavedOps::new(initial).room_after(0)
    }

    /// Whether an op whose JSON is `json` fits.
    pub(crate) fn fits(self, json: &str) -> bool {
        self.needed(json) <= self.left
    }

    /// Takes the room of an op whose JSON is `json`.
    ///
    /// # Errors
    ///
    /// [`OpError::FileFull`] when it does not fit; nothing is taken.
    pub(crate) fn take(&mut self, json: &str) -> Result<(), OpError> {
        if !self.fits(json) {
            return Err(OpError::FileFull);
        }
        self.left = self.left.saturating_sub(self.needed(json));
        self.after_op = true;
        Ok(())
    }

    /// The bytes an op whose JSON is `json` adds to the file, with the comma
    /// before it.
    fn needed(self, json: &str) -> usize {
        json.len().saturating_add(usize::from(self.after_op))
    }
}

/// The most bytes a saved file of a session on `initial` takes besides its
/// ops' JSON: the document with no op, its configuration in whichever mode
/// writes it longer, and the longest place a cursor can have, since no log
/// a session keeps holds more than [`MAX_SHARE_OPS`] ops or more than
/// [`SESSION_EVENT_LIMIT`] events. So a file within that and its ops is
/// within it whatever the mode chosen and wherever the cursor goes.
fn frame(initial: &Config) -> usize {
    let at = ShareAt {
        op: MAX_SHARE_OPS.saturating_sub(1),
        event: SESSION_EVENT_LIMIT,
    };
    [false, true]
        .into_iter()
        .map(|advanced| {
            let doc = ShareDoc {
                schema_version: SCHEMA_VERSION,
                config: Config {
                    advanced,
                    ..initial.clone()
                },
                ops: Vec::new(),
                at: Some(at),
            };
            to_json(&doc).len()
        })
        .max()
        .unwrap_or(usize::MAX)
}

/// `value` as compact JSON. A configuration, an op, a place and a document
/// hold no map and nothing that fails to serialize, so this never fails.
fn to_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// `json` as a share string: deflated, then base64url without padding.
fn encode(json: &str) -> String {
    URL_SAFE_NO_PAD.encode(compress_to_vec(json.as_bytes(), DEFLATE_LEVEL))
}

/// `json` as a share string, if a link can open it: the JSON within
/// [`MAX_SHARE_JSON_BYTES`], the most a link unpacks to, and the string
/// within [`MAX_SHARE_TEXT_BYTES`].
///
/// # Errors
///
/// [`TooLarge::Json`] with the JSON's size, or [`TooLarge::Text`] with the
/// string's length: the first of the two a link could not open.
fn link(json: &str) -> Result<String, TooLarge> {
    if exceeds(json.len(), MAX_SHARE_JSON_BYTES) {
        return Err(TooLarge::Json {
            bytes: Some(count(json.len())),
        });
    }
    let share = encode(json);
    if exceeds(share.len(), MAX_SHARE_TEXT_BYTES) {
        return Err(TooLarge::Text {
            bytes: count(share.len()),
        });
    }
    Ok(share)
}

/// The JSON text a share string holds: bounded first, then decoded from
/// base64url with or without padding and inflated, the inflating stopped at
/// [`MAX_SHARE_JSON_BYTES`] so that a short link cannot unpack into
/// unbounded memory.
fn link_text(s: &str) -> Result<String, EncodingError> {
    if exceeds(s.len(), MAX_SHARE_TEXT_BYTES) {
        return Err(TooLarge::Text {
            bytes: count(s.len()),
        }
        .into());
    }
    let deflated = URL_SAFE_NO_PAD_INDIFFERENT
        .decode(s)
        .map_err(|_| EncodingError::Base64)?;
    let limit = usize::try_from(MAX_SHARE_JSON_BYTES).unwrap_or(usize::MAX);
    let json = decompress_to_vec_with_limit(&deflated, limit).map_err(|error| {
        if error.status == TINFLStatus::HasMoreOutput {
            EncodingError::TooLarge(TooLarge::Json { bytes: None })
        } else {
            EncodingError::Deflate
        }
    })?;
    String::from_utf8(json).map_err(|_| EncodingError::Utf8)
}

/// Checks a saved file's size against [`MAX_SHARE_JSON_BYTES`].
fn file_size(json: &str) -> Result<(), EncodingError> {
    if exceeds(json.len(), MAX_SHARE_JSON_BYTES) {
        return Err(TooLarge::Json {
            bytes: Some(count(json.len())),
        }
        .into());
    }
    Ok(())
}

/// The document `text` holds: its format version first, so a newer
/// document is refused for that whatever its shape, then the document, then
/// the sizes of its log and programs.
fn read(text: &str) -> Result<ShareDoc, Box<Unread>> {
    let unread = |error, doc| Box::new(Unread { error, doc });
    let version: Version =
        serde_json::from_str(text).map_err(|error| unread(json_error(&error), None))?;
    if version.schema_version > SCHEMA_VERSION {
        let newer = LoadError::NewerSchema {
            found: version.schema_version,
            supported: SCHEMA_VERSION,
        };
        return Err(unread(newer, None));
    }
    // Format 1 is the first, so no older document needs migrating yet; when
    // the format changes, the reader of each older version goes here.
    let doc: ShareDoc =
        serde_json::from_str(text).map_err(|error| unread(json_error(&error), None))?;
    match too_large(&doc) {
        Some(error) => Err(unread(error.into(), Some(doc))),
        None => Ok(doc),
    }
}

/// The first size limit `doc` breaks: its ops, then its programs in order.
fn too_large(doc: &ShareDoc) -> Option<TooLarge> {
    if exceeds(doc.ops.len(), MAX_SHARE_OPS) {
        return Some(TooLarge::Ops {
            ops: count(doc.ops.len()),
        });
    }
    (0_u32..).zip(&doc.ops).find_map(|(op, entry)| match entry {
        Op::Program { source, .. } if exceeds(source.len(), MAX_PROGRAM_BYTES) => {
            Some(TooLarge::Program {
                op,
                bytes: count(source.len()),
            })
        }
        _ => None,
    })
}

/// Reads a document's format version, refusing 0: formats count from 1.
fn format_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version == 0 {
        return Err(D::Error::invalid_value(
            Unexpected::Unsigned(0),
            &"a format version from 1",
        ));
    }
    Ok(version)
}

/// The [`LoadError::Json`] of `error`: its place, and serde's message
/// without the place `serde_json` appends to it. `serde_json` gives column 0
/// where the text ends at the start of a line, as an empty file does; the
/// message counts columns from 1, so that is column 1.
fn json_error(error: &serde_json::Error) -> LoadError {
    let text = error.to_string();
    let place = format!(" at line {} column {}", error.line(), error.column());
    let message = text.strip_suffix(&place).unwrap_or(&text).to_owned();
    LoadError::Json {
        line: count(error.line()).max(1),
        column: count(error.column()).max(1),
        message,
    }
}

/// What the page shows for a load from `source` that failed with `error`,
/// on `doc` if it was read, from `text` if it was decoded.
fn failure(
    source: Source,
    error: &LoadError,
    doc: Option<&ShareDoc>,
    text: Option<&str>,
) -> LoadFailure {
    let file = source == Source::File;
    LoadFailure {
        title: explain::load_title(file),
        lead: explain::load_lead(file),
        message: error.segments(),
        contents: contents(doc, text),
        repair: doc.and_then(|doc| repair(doc, error)),
        prefix: doc.and_then(|doc| prefix(doc, error)),
    }
}

/// What "Show link contents" shows: the document as pretty JSON if it was
/// read; the JSON as pretty JSON if it is JSON; the text as it came
/// otherwise; nothing if it was never decoded.
fn contents(doc: Option<&ShareDoc>, text: Option<&str>) -> Option<String> {
    if let Some(doc) = doc {
        // A document holds no map, and every value serializes.
        return serde_json::to_string_pretty(doc).ok();
    }
    let text = text?;
    let pretty = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok());
    Some(pretty.unwrap_or_else(|| text.to_owned()))
}

/// "Open with 2-way instead": `doc` with the configuration that `error`
/// found invalid repaired, the first run's or a `Config` op's, when a link
/// can hold it.
fn repair(doc: &ShareDoc, error: &LoadError) -> Option<LoadOption> {
    let (repair, ops) = match error {
        LoadError::InitialConfig { .. } => (doc.config.repair()?, None),
        LoadError::Op {
            index,
            error: OpError::Config(_),
            ..
        } => {
            let slot = usize::try_from(*index).ok()?;
            let Some(Op::Config { config }) = doc.ops.get(slot) else {
                return None;
            };
            (config.repair()?, Some(slot))
        }
        _ => return None,
    };
    let mut repaired = ShareDoc {
        schema_version: SCHEMA_VERSION,
        ..doc.clone()
    };
    match ops {
        None => repaired.config = repair.config,
        Some(slot) => {
            let op = repaired.ops.get_mut(slot)?;
            *op = Op::Config {
                config: repair.config,
            };
        }
    }
    Some(LoadOption {
        label: repair.label,
        share: openable(&repaired)?,
    })
}

/// "Open the first 8 actions": `doc` up to the op that `error` refused,
/// opening at its end, when an op before it replayed and a link can hold
/// them.
fn prefix(doc: &ShareDoc, error: &LoadError) -> Option<LoadOption> {
    let LoadError::Op { index, .. } = error else {
        return None;
    };
    if *index == 0 {
        return None;
    }
    let ops = doc.ops.get(..usize::try_from(*index).ok()?)?.to_vec();
    let prefix = ShareDoc {
        schema_version: SCHEMA_VERSION,
        config: doc.config.clone(),
        ops,
        at: None,
    };
    Some(LoadOption {
        label: explain::prefix_label(*index),
        share: openable(&prefix)?,
    })
}

/// `doc` as a share string, if one can be opened: a file holds more than a
/// link, so a document read from a file may be too large for a link, its
/// JSON or its share string. Its ops and programs are those of a document
/// that was read, or fewer, so they are within their limits.
fn openable(doc: &ShareDoc) -> Option<String> {
    link(&to_json(doc)).ok()
}

/// The sentence after a configuration or an op that failed, saying how
/// many actions, the ops of the log, after it were not replayed, with the
/// space before it: " The 6 actions after it were not replayed."; nothing
/// when none were.
fn not_replayed_note(actions: u32) -> String {
    match actions {
        0 => String::new(),
        1 => " The action after it was not replayed.".to_owned(),
        actions => format!(
            " The {} actions after it were not replayed.",
            format::data(actions)
        ),
    }
}

/// The place a JSON error opens with: "Line 1, column 57: ".
fn json_place(line: u32, column: u32) -> String {
    format!(
        "Line {}, column {}: ",
        format::position(line),
        format::position(column)
    )
}

/// The message of JSON larger than can be opened.
fn json_too_large(bytes: Option<u32>) -> String {
    match bytes {
        Some(bytes) => format!(
            "The saved run is {} bytes of JSON; at most {} bytes can be opened.",
            format::setting(bytes),
            format::setting(MAX_SHARE_JSON_BYTES)
        ),
        None => format!(
            "The link unpacks to more than the {} bytes of JSON that can be opened.",
            format::setting(MAX_SHARE_JSON_BYTES)
        ),
    }
}

/// The position `event` events into the op being replayed, the one after
/// the ops that end at `op_ends`, once the `events` replayed so far reach
/// it; none before.
fn reached(op_ends: &[EventPos], event: u32, events: usize) -> Option<EventPos> {
    let place = end_of(op_ends).get().checked_add(event)?;
    let replayed = usize::try_from(place).ok()? <= events;
    replayed.then_some(EventPos::new_unchecked(place))
}

/// The end of a log whose ops end at `op_ends`.
fn end_of(op_ends: &[EventPos]) -> EventPos {
    op_ends
        .last()
        .copied()
        .unwrap_or(EventPos::new_unchecked(0))
}

/// Whether `len` is more than `limit`.
fn exceeds(len: usize, limit: u32) -> bool {
    u64::try_from(len).unwrap_or(u64::MAX) > u64::from(limit)
}

/// `n` as a count the core keeps, saturating.
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::Preset;
    use crate::op::{AccessKind, ProgramFormat};
    use crate::session::ConfigChange;
    use crate::units::Addr;

    fn load(addr: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: Addr::new_unchecked(addr),
            value: None,
        }
    }

    /// Checks that the JSON the session writes from the ops' JSON it keeps
    /// is the JSON `serde_json` writes for its document, the file's and the
    /// link's alike, and that the size it keeps is that JSON's.
    fn kept_json_is_the_documents(session: &Session) {
        let json = session.share_json();
        assert_eq!(json, to_json(&session.share_doc()));
        assert_eq!(session.to_json(), json);
        assert_eq!(session.to_json_len() as usize, json.len());
        assert_eq!(
            link_text(&session.to_share_string().unwrap()).unwrap(),
            json
        );
    }

    #[test]
    fn the_ops_json_kept_and_its_size_are_the_documents_as_the_log_changes() {
        let mut session = Session::with_config(Preset::TwoWayLru.config()).unwrap();
        kept_json_is_the_documents(&session);
        for op in [
            load(0x0A4),
            Op::Program {
                source: "ld r1, [0x0A8]\nadd r1, r1, #1\nst [0x0B0], r1".to_owned(),
                format: ProgramFormat::Asm,
            },
            Op::Run { n: 3 },
            Op::Config {
                config: Preset::FourWayWriteBack.config(),
            },
            load(0x1A4),
        ] {
            session.apply(op).unwrap();
            kept_json_is_the_documents(&session);
        }
        // Undone ops stay; a new op drops them.
        assert!(session.undo());
        assert!(session.undo());
        kept_json_is_the_documents(&session);
        session.apply(load(0x2A4)).unwrap();
        assert_eq!(session.op_count(), 4);
        kept_json_is_the_documents(&session);
        assert_eq!(session.set_advanced(true), ConfigChange::Applied);
        kept_json_is_the_documents(&session);
        session.seek(EventPos::new_unchecked(0));
        kept_json_is_the_documents(&session);
        assert_eq!(session.set_advanced(false), ConfigChange::Applied);
        kept_json_is_the_documents(&session);
    }

    #[test]
    fn a_files_room_is_what_its_frame_and_its_ops_leave() {
        let initial = Preset::TwoWayLru.config();
        let limit = usize::try_from(MAX_SHARE_JSON_BYTES).unwrap();
        let mut saved = SavedOps::new(&initial);
        // The frame is the document with no op at its longest: in the mode
        // that writes it longer, with the longest place a cursor can have.
        let longest = ShareDoc {
            schema_version: SCHEMA_VERSION,
            config: Config {
                advanced: false,
                ..initial.clone()
            },
            ops: Vec::new(),
            at: Some(ShareAt {
                op: MAX_SHARE_OPS - 1,
                event: SESSION_EVENT_LIMIT,
            }),
        };
        let advanced = ShareDoc {
            config: Config {
                advanced: true,
                ..initial
            },
            ..longest.clone()
        };
        assert!(to_json(&advanced).len() < to_json(&longest).len());
        assert_eq!(saved.frame, to_json(&longest).len());
        let empty = FileRoom {
            left: limit - saved.frame,
            after_op: false,
        };
        assert_eq!(saved.room_after(0), empty);
        // An op takes its JSON, and a comma before it after the first.
        let ops = [load(0x0A4), Op::Step];
        let mut room = empty;
        for (count, op) in (1..).zip(&ops) {
            let json = op_json(op);
            room.take(&json).unwrap();
            saved.push(&json);
            assert_eq!(room, saved.room_after(count));
        }
        let (first, second) = (op_json(&load(0x0A4)), op_json(&Op::Step));
        assert_eq!(saved.text(), format!("{first},{second}"));
        assert_eq!(
            room.left,
            limit - saved.frame - first.len() - 1 - second.len()
        );
        // The document with those ops at its longest is the frame and the
        // ops' JSON, to the byte.
        let doc = ShareDoc {
            ops: ops.to_vec(),
            ..longest
        };
        assert_eq!(to_json(&doc).len(), saved.frame + saved.text().len());
        // An op that fills the room to the byte fits; one a byte longer does
        // not, and takes nothing.
        let exact = "x".repeat(room.left - 1);
        let over = format!("{exact}x");
        assert!(room.fits(&exact));
        assert!(!room.fits(&over));
        let mut full = room;
        assert_eq!(full.take(&over), Err(OpError::FileFull));
        assert_eq!(full, room);
        full.take(&exact).unwrap();
        assert_eq!(full.left, 0);
        assert!(!full.fits(""));
        // Dropping ops gives their room back; counting past the ops kept
        // counts those.
        saved.truncate(1);
        assert_eq!(saved.text(), first);
        assert_eq!(saved.room_after(5), saved.room_after(1));
        saved.truncate(0);
        assert_eq!(saved.text(), "");
        assert_eq!(saved.room_after(0), empty);
    }

    #[test]
    fn a_json_error_keeps_serdes_message_without_its_place() {
        let Err(error) =
            serde_json::from_str::<ShareDoc>("{\n  \"schema_version\": 1,\n  \"x\": 2\n}")
        else {
            panic!("an unknown field");
        };
        let LoadError::Json {
            line,
            column,
            message,
        } = json_error(&error)
        else {
            panic!("a JSON error");
        };
        assert_eq!((line, column), (3, 5));
        assert!(
            message.starts_with("unknown field `x`, expected one of `schema_version`"),
            "{message}"
        );
        assert!(!message.contains(" at line "), "{message}");
    }

    #[test]
    fn a_program_too_large_is_the_first_one_in_order() {
        let long = "R 0x0A4\n".repeat(usize::try_from(MAX_PROGRAM_BYTES).unwrap() / 8 + 1);
        let program = |source: &str| Op::Program {
            source: source.to_owned(),
            format: ProgramFormat::Trace,
        };
        let doc = ShareDoc {
            schema_version: SCHEMA_VERSION,
            config: Preset::DirectMapped8.config(),
            ops: vec![program("R 0x0A4"), program(&long), program(&long)],
            at: None,
        };
        assert_eq!(
            too_large(&doc),
            Some(TooLarge::Program {
                op: 1,
                bytes: u32::try_from(long.len()).unwrap()
            })
        );
    }
}
