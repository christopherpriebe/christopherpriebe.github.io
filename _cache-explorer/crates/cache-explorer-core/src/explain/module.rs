//! The wording of the module panel: a step's kind, the progress, the
//! picker's line, a prediction's feedback and the done screen.

use crate::format;
use crate::module::{FeedbackTone, ModuleStepKind};

/// The module panel's label for a step of `kind`, in the design's words:
/// "Explain", "Do this", "Predict", "Watch", and "Done" on the done screen.
pub(crate) fn module_step_kind(kind: ModuleStepKind) -> String {
    match kind {
        ModuleStepKind::Explain => "Explain",
        ModuleStepKind::Instruct => "Do this",
        ModuleStepKind::Predict => "Predict",
        ModuleStepKind::Observe => "Watch",
        ModuleStepKind::Done => "Done",
    }
    .to_owned()
}

/// The module panel's progress: "Step 3 of 6" on step index 2 of 6, and
/// "6 of 6" on the done screen, where `step` is the number of steps.
pub(crate) fn module_progress(step: u32, steps: u32) -> String {
    if step >= steps {
        format!("{} of {}", format::data(steps), format::data(steps))
    } else {
        format!(
            "Step {} of {}",
            format::data(step.saturating_add(1)),
            format::data(steps)
        )
    }
}

/// The picker's line while a module runs: its number and title, then the
/// step, "Running 04 · Tracking LRU · step 3 of 6", or on the done screen
/// "Running 04 · Tracking LRU · complete". `step` is the step's index, and
/// the number of steps on the done screen.
pub(crate) fn module_running(number: &str, title: &str, step: u32, steps: u32) -> String {
    let at = if step >= steps {
        "complete".to_owned()
    } else {
        format!(
            "step {} of {}",
            format::data(step.saturating_add(1)),
            format::data(steps)
        )
    };
    format!("Running {number} \u{b7} {title} \u{b7} {at}")
}

/// The picker's line while no module runs: "9 modules · none running".
pub(crate) fn modules_idle(modules: u32) -> String {
    format!(
        "{} {} \u{b7} none running",
        format::data(modules),
        format::plural(modules, "module", "modules")
    )
}

/// The done screen's text.
pub(crate) fn module_complete() -> String {
    "Module complete. The workbench stays as you left it; keep experimenting or pick another \
     module."
        .to_owned()
}

/// The head of a prediction's feedback: "Correct.", "Not quite." or
/// "Shown.".
pub(crate) fn feedback_head(tone: FeedbackTone) -> String {
    match tone {
        FeedbackTone::Correct => "Correct.",
        FeedbackTone::Incorrect => "Not quite.",
        FeedbackTone::Shown => "Shown.",
    }
    .to_owned()
}

/// What a revealed prediction's feedback says before the explanation.
pub(crate) fn answer_shown() -> String {
    "The answer is highlighted. ".to_owned()
}

#[cfg(test)]
mod tests {
    #![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

    use super::*;

    #[test]
    fn the_module_panel_s_texts() {
        let labels = [
            (ModuleStepKind::Explain, "Explain"),
            (ModuleStepKind::Instruct, "Do this"),
            (ModuleStepKind::Predict, "Predict"),
            (ModuleStepKind::Observe, "Watch"),
            (ModuleStepKind::Done, "Done"),
        ];
        for (kind, label) in labels {
            assert_eq!(module_step_kind(kind), label);
        }
        assert_eq!(module_progress(2, 6), "Step 3 of 6");
        assert_eq!(module_progress(6, 6), "6 of 6");
        assert_eq!(
            module_running("04", "Tracking LRU", 2, 6),
            "Running 04 \u{b7} Tracking LRU \u{b7} step 3 of 6"
        );
        assert_eq!(
            module_running("04", "Tracking LRU", 6, 6),
            "Running 04 \u{b7} Tracking LRU \u{b7} complete"
        );
        assert_eq!(modules_idle(9), "9 modules \u{b7} none running");
        assert_eq!(modules_idle(1), "1 module \u{b7} none running");
        assert_eq!(feedback_head(FeedbackTone::Correct), "Correct.");
        assert_eq!(feedback_head(FeedbackTone::Incorrect), "Not quite.");
        assert_eq!(feedback_head(FeedbackTone::Shown), "Shown.");
        assert_eq!(answer_shown(), "The answer is highlighted. ");
    }
}
