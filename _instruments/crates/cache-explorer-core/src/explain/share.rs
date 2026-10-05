//! The wording of sharing: a shared run's loading summary, the line Share
//! shows once it has copied a link, and the heading, lead and prefix action
//! of a link or file that cannot be opened.

use crate::format;
use crate::share::Source;

/// The line a shared run's loading screen shows under its heading: the
/// opening run's short summary, then its steps and the step it opens at,
/// "2-way LRU · 128 B · 14 steps, opening at step 9."; with several runs,
/// their count first: "2-way LRU · 128 B · 3 runs; this one has 14 steps,
/// opening at step 9." `summary` is the opening run's short summary, `runs`
/// the runs in the log, `steps` the opening run's steps and `opening` the
/// step it opens at, 0 before the first.
pub(crate) fn share_text(summary: &str, runs: u32, steps: u32, opening: u32) -> String {
    let steps = if steps == 0 {
        "no steps yet".to_owned()
    } else if opening == 0 {
        format!("{}, opening before the first", step_count(steps))
    } else {
        format!(
            "{}, opening at step {}",
            step_count(steps),
            format::data(opening)
        )
    };
    if runs > 1 {
        format!(
            "{summary} \u{b7} {} runs; this one has {steps}.",
            format::data(runs)
        )
    } else {
        format!("{summary} \u{b7} {steps}.")
    }
}

/// The status line once Share has copied the link: what the link encodes
/// and where it opens, "Link copied. It encodes the configuration and all
/// 14 steps, and opens at step 9."; with several runs, "Link copied. It
/// encodes all 3 runs, and opens this one at step 9 of 14." The counts are
/// as for [`share_text`].
pub(crate) fn share_copied(runs: u32, steps: u32, opening: u32) -> String {
    if runs > 1 {
        let place = if steps == 0 {
            ", which has no steps yet".to_owned()
        } else if opening == 0 {
            format!(" before step 1 of {}", format::data(steps))
        } else {
            format!(
                " at step {} of {}",
                format::data(opening),
                format::data(steps)
            )
        };
        return format!(
            "Link copied. It encodes all {} runs, and opens this one{place}.",
            format::data(runs)
        );
    }
    if steps == 0 {
        return "Link copied. It encodes the configuration; there are no steps yet.".to_owned();
    }
    let all = if steps == 1 {
        "its 1 step".to_owned()
    } else {
        format!("all {}", step_count(steps))
    };
    let opens = if opening == 0 {
        "opens before the first".to_owned()
    } else {
        format!("opens at step {}", format::data(opening))
    };
    format!("Link copied. It encodes the configuration and {all}, and {opens}.")
}

/// `steps` steps: "14 steps", "1 step".
fn step_count(steps: u32) -> String {
    format!(
        "{} {}",
        format::data(steps),
        format::plural(steps, "step", "steps")
    )
}

/// What a document from `source` is called in the wording: "link" or
/// "file".
fn noun(source: Source) -> &'static str {
    match source {
        Source::Link => "link",
        Source::File => "file",
    }
}

/// The heading of the page's state for a link or a file that cannot be
/// opened: "Could not open link", or for a file "Could not open file".
pub(crate) fn load_title(source: Source) -> String {
    format!("Could not open {}", noun(source))
}

/// The lead under that heading: "This link doesn't describe a cache the
/// Interactive Cache Explorer can build.", or the same of a file.
pub(crate) fn load_lead(source: Source) -> String {
    format!(
        "This {} doesn't describe a cache the Interactive Cache Explorer can build.",
        noun(source)
    )
}

/// The label of the option that opens the actions, the ops, a link or a
/// file holds before the one that could not be replayed: "Open the first 8
/// actions", or with one, "Open the first action".
pub(crate) fn prefix_label(actions: u32) -> String {
    if actions == 1 {
        "Open the first action".to_owned()
    } else {
        format!("Open the first {} actions", format::data(actions))
    }
}
