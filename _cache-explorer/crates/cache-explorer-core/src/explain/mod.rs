//! The wording of the log: every line built from machine state is written
//! here, so the tool's voice has one source. That includes the CPU's status
//! and the Stats lines: AMAT and the measured cost in timed mode, and the
//! count beside the hit and miss sparkline; the program drawer's lines: its
//! summary and the messages of a program or trace that does not parse; the
//! configuration's summaries, the labels of its repairs and of the
//! configuration bar's options; the texts of the views: the cache's and
//! memory's headings and descriptions, the lanes, the log's headers, the
//! changed summary and the status line; and the editors' messages: each
//! rule's title and text, the preview of an edit, the labels of its actions
//! and the log line of an applied edit; and the sharing texts: a shared
//! run's summary, the line Share shows once it has copied a link, and the
//! heading, lead and prefix action of a link or file that cannot be opened;
//! and the module panel's texts: a step's kind, the progress, the picker's
//! line, a prediction's feedback and the done screen.
//! Error messages are the exception; each is the `Display` of its error
//! type, next to that type. The drawer shares three of those: an address out
//! of range or misaligned, and a value the word cannot hold.
//!
//! An access's lines keep the design's wording where it has one, as in "Way 1
//! chosen: least recently used (rank 1).", and the lines it does not draw
//! follow its voice: short declarative sentences that give the number that
//! matters. Addresses, lines and tags are padded to the configuration's
//! widths, and values follow the data rule. No line depends on the cursor or
//! on Basics and Advanced, so the same event always reads the same.

mod access;
mod config;
mod edit;
mod module;
mod program;
mod share;
#[cfg(test)]
mod testing;
mod view;

pub(crate) use access::{
    add, bnez, decode, evict, fault_misaligned, fault_out_of_range, fill, hit, internal_error,
    load, miss, out_of_budget, rank, stall, truncated, victim, write, write_around, write_back,
    write_through, StallFor, WriteNote,
};
pub(crate) use config::{
    address_option, allocate_option, bytes_option, config_summary, full_option, repair_label,
    replacement_option, same_as_full, short_summary, ways_option, word_option, write_option,
};
pub(crate) use edit::{
    already_cached, already_holds, already_in, belongs_in, cached_item, check_value, check_word,
    clean_copy, clean_copy_title, data_differs, data_length, discards, edit_action, edit_ready,
    edit_refused, edited_memory, edited_way, empty_way, empty_way_title, kind_mismatch,
    matches_dirty, matches_dirty_title, memory_item, memory_only, memory_only_title, missing_line,
    missing_line_title, needs_dirty, never_dirty, never_dirty_title, newer_value,
    newer_value_title, no_changes, no_memory_change, no_such_set, no_such_way, no_way_change,
    not_an_address, rank_fixed, rank_out_of_range, rank_out_of_range_title, rank_too_high,
    ranks_shift, recorded_as_step, unfinished_text, unfinished_title, way_item,
    write_through_agrees, wrong_set, wrong_set_full, EditAction, MemoryNote, Unfinished, WayItem,
};
pub(crate) use module::{
    answer_shown, feedback_head, module_complete, module_progress, module_running,
    module_step_kind, modules_idle,
};
pub(crate) use program::{
    end_of_program, needs_reset, new_run, parse_error, parse_problem, program_loaded,
    program_summary, program_unloaded, session_limit, ParseProblem,
};
pub(crate) use share::{load_lead, load_title, prefix_label, share_copied, share_text};
pub(crate) use view::{
    amat, bus_idle, cache_header, changed_summary, edit_memory, edit_way, entry_outcome,
    entry_status, fill_transfer, finished, issued, live, log_header, measured, memory_caption,
    memory_word, no_instruction, no_request, program_divider, ready, recent, request,
    request_source, stale, stalled, status_line, stopped_at, typed_request, way_description,
    write_around_transfer, write_back_transfer, write_through_transfer,
};

use crate::format;
use crate::geometry::Geometry;
use crate::op::ProgramFormat;
use crate::units::Tag;

/// What an entry is, as the CPU status names it. The design calls every
/// entry an access; here an instruction without a memory access, an edit and
/// an error that ends an op are named for what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryNoun {
    /// A load or store, issued by hand or executed by a program.
    Access,
    /// An instruction without a memory access, such as `add` or `bnez`.
    Instruction,
    /// An edit of a way or a memory word.
    Edit,
    /// An error that ended an op.
    Error,
}

/// "program" or "trace".
fn source_noun(format: ProgramFormat) -> &'static str {
    match format {
        ProgramFormat::Asm => "program",
        ProgramFormat::Trace => "trace",
    }
}

/// An address or a line address, padded to the address width. A `Display`
/// adapter, like [`format::Hex`] itself, so a per-event line can interpolate
/// it without allocating a `String` first.
fn address(geometry: &Geometry, value: u32) -> format::Hex {
    format::Hex {
        value: u64::from(value),
        digits: usize::from(geometry.address_bits()).div_ceil(4),
    }
}

/// A tag, padded to the tag width. A `Display` adapter, for the same reason
/// as [`address`].
fn tag(geometry: &Geometry, tag: Tag) -> format::Hex {
    format::Hex {
        value: u64::from(tag.get()),
        digits: usize::from(geometry.tag_bits()).div_ceil(4).max(2),
    }
}
