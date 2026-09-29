//! A module in progress: the step the student is on, a prediction's answer,
//! and the panel the page draws for them.
//!
//! Progress lives here, in the core, and never in a share link or a file.
//! An instruct step completes once an op made since it began leaves the
//! cursor at the end of an entry where its predicate holds; an observe step
//! once its predicate holds at the cursor; an explanation and an answered
//! prediction on Next. While a prediction is unanswered the run gates the
//! session: every op is refused and the cursor cannot pass the point where
//! the prediction began. "Show me" does an instruct step's ops, moves the
//! cursor to where an observe step's predicate first holds, and, only after
//! one wrong answer has been checked, reveals a prediction's answer.

use serde::Serialize;

use super::{Callout, EvalContext, Module, ModuleError, Step};
use crate::event::MicroEvent;
use crate::explain;
use crate::op::{Op, OpError};
use crate::session::{Boundary, Session};
use crate::text::{markup, Segments, TextSegment};
use crate::units::EventPos;

/// A module in progress on a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRun {
    /// The module.
    module: Module,
    /// The current step's index; the number of steps on the done screen.
    step: u32,
    /// The serial of the last op made before the current step began: an op
    /// with a greater one was made since.
    since: u64,
    /// The current step's prediction, when it is a predict step.
    predict: Option<Prediction>,
    /// The message of the last "Show me", Restart or prediction `then` the
    /// state refused on the current step; cleared once a new step begins.
    notice: Option<String>,
}

/// A prediction in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prediction {
    /// The option selected, if any.
    selected: Option<u32>,
    /// What the last check, or "Show me", found; none before a check, and
    /// again once another option is selected after a wrong answer.
    feedback: Option<Answer>,
    /// How many wrong answers have been checked.
    wrong_attempts: u32,
    /// The position where the prediction began, which the cursor may not
    /// pass until it is answered.
    limit: EventPos,
}

/// What a prediction's check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// The option selected is the answer.
    Correct,
    /// It is not.
    Wrong,
    /// "Show me" revealed the answer.
    Shown,
}

/// Everything the module panel draws.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ModuleView {
    /// The module's id.
    pub id: String,
    /// The module's title.
    pub title: String,
    /// What the module teaches.
    pub description: Vec<TextSegment>,
    /// The current step's index, from 0; the number of steps on the done
    /// screen.
    pub step_index: u32,
    /// The number of steps.
    pub step_count: u32,
    /// The current step's kind, or the done screen.
    pub kind: ModuleStepKind,
    /// The kind in the design's words: "Do this", "Predict", "Watch",
    /// "Explain", or "Done".
    pub kind_label: String,
    /// "Step 3 of 6", or "6 of 6" on the done screen.
    pub progress_text: String,
    /// The step's text, or the done screen's.
    pub text: Vec<TextSegment>,
    /// An instruct step's hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<Vec<TextSegment>>,
    /// A predict step's options, with their states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<OptionView>>,
    /// What checking the prediction, or revealing it, found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<Feedback>,
    /// The region the step points at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<Callout>,
    /// Why the last "Show me", Restart or prediction `then` was refused,
    /// such as at the session's limit; the step keeps waiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// The panel's buttons, in order; a button not listed is not shown.
    pub actions: Vec<ModuleAction>,
    /// The picker's line while the module runs: "Running 04 · Tracking LRU
    /// · step 3 of 6".
    pub drawer_summary: String,
}

/// A step's kind, as the panel shows it, or the done screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum ModuleStepKind {
    /// Asks the student to do something.
    Instruct,
    /// Asks for a prediction.
    Predict,
    /// Waits for something to happen.
    Observe,
    /// Explains.
    Explain,
    /// The done screen, after the last step.
    Done,
}

/// One option of a prediction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct OptionView {
    /// Its text.
    pub text: Vec<TextSegment>,
    /// How it is drawn.
    pub state: OptionState,
}

/// How an option is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum OptionState {
    /// Not selected.
    Idle,
    /// Selected, and not checked, or not the answer once it is revealed.
    Selected,
    /// Selected and checked, and not the answer.
    Wrong,
    /// Selected and checked, and the answer.
    Correct,
    /// The answer, revealed by "Show me".
    Shown,
}

/// What checking a prediction, or revealing it, found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    /// Its colour.
    pub tone: FeedbackTone,
    /// "Correct.", "Not quite." or "Shown.".
    pub head: String,
    /// The hint, or the explanation.
    pub text: Vec<TextSegment>,
}

/// The colour of a prediction's feedback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum FeedbackTone {
    /// The answer was right.
    Correct,
    /// The answer was wrong.
    Incorrect,
    /// The answer was revealed.
    Shown,
}

/// A button of the module panel, and whether it is live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ModuleAction {
    /// The button.
    pub action: ModuleActionKind,
    /// Whether it is live; a button shown disabled is not.
    pub enabled: bool,
}

/// A button of the module panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum ModuleActionKind {
    /// Check: checks the option selected.
    Check,
    /// Next: goes on to the next step.
    Next,
    /// Show me.
    ShowMe,
    /// Restart, on the done screen: starts the module again.
    Restart,
}

impl ModuleRun {
    /// A run of `module`, on its first step, not yet started on a session.
    pub fn new(module: Module) -> ModuleRun {
        ModuleRun {
            module,
            step: 0,
            since: 0,
            predict: None,
            notice: None,
        }
    }

    /// A run of the module read from `json`, as [`Module::parse`] reads and
    /// checks it, on its first step and not yet started.
    ///
    /// # Errors
    ///
    /// The [`ModuleError`] [`Module::parse`] gives.
    pub fn from_json(json: &str) -> Result<ModuleRun, ModuleError> {
        Module::parse(json).map(ModuleRun::new)
    }

    /// The module.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Starts the module on `session`, from its first step: appends
    /// `Config` with the preset, and `Program` with the module's program if
    /// it has one, so the work before stays reachable with Undo, and sets
    /// the mode to the module's. A gate another run left is lifted first.
    /// Returns the events of the ops appended, as the log holds them.
    ///
    /// # Errors
    ///
    /// The [`OpError`] refusing the ops, such as the session's limit; the
    /// session and this run are then as they were, a gate included.
    pub fn start<'s>(&mut self, session: &'s mut Session) -> Result<&'s [MicroEvent], OpError> {
        let mut ops = vec![Op::Config {
            config: self.module.preset().clone(),
        }];
        if let Some(program) = self.module.program() {
            ops.push(Op::Program {
                source: program.source.clone(),
                format: program.format,
            });
        }
        let held = session.limit();
        session.clear_gate();
        let appended = session
            .apply_all(ops)
            .map(<[MicroEvent]>::len)
            .inspect_err(|_| {
                if let Some(limit) = held {
                    session.set_gate(limit);
                }
            })?;
        // The preset of a module that is not advanced fits Basics, so the
        // switch applies at once.
        let _ = session.set_advanced(self.module.advanced());
        self.step = 0;
        self.enter(session);
        // Neither call above appends an event, so the ops' events are still
        // the last of the log. They are read from it here, as the slice
        // `apply_all` returned would have held the session through those
        // calls.
        let events = session.events();
        Ok(events
            .get(events.len().saturating_sub(appended)..)
            .unwrap_or_default())
    }

    /// Re-evaluates the current step after any change to `session`, and
    /// sets or lifts the session's gate. An instruct step completes, and
    /// the run goes on to the next step, when the cursor is at the end of
    /// an entry, an op has been made since the step began, and its `expect`
    /// holds; an observe step when its `until` holds at the cursor. One
    /// step at most completes per call.
    pub fn sync(&mut self, session: &mut Session) -> ModuleView {
        let done = match self.current() {
            Some(Step::Instruct { expect, .. }) => {
                session.position().boundary >= Boundary::EntryEnd
                    && session.last_serial() > self.since
                    && expect.eval(&EvalContext::new(session).since(self.since))
            }
            Some(Step::Observe { until, .. }) => {
                until.eval(&EvalContext::new(session).since(self.since))
            }
            Some(Step::Predict { .. }) => {
                if self.predict.is_none() {
                    self.predict = Some(Prediction::new(session.cursor_position()));
                }
                false
            }
            Some(Step::Explain { .. }) | None => false,
        };
        if done {
            self.advance(session);
        } else {
            self.gate(session);
        }
        self.view(session)
    }

    /// Selects option `option` of the current prediction, while it is
    /// unanswered. After a wrong answer, a new selection clears the
    /// feedback, and the prediction can be checked again.
    pub fn select(&mut self, option: u32) {
        let options = match self.current() {
            Some(Step::Predict { options, .. }) => options.len(),
            _ => return,
        };
        let in_range = usize::try_from(option).is_ok_and(|option| option < options);
        if let Some(predict) = self.predict.as_mut().filter(|_| in_range) {
            if !predict.answered() {
                predict.selected = Some(option);
                predict.feedback = None;
            }
        }
    }

    /// Checks the option selected: correct lifts the gate and applies the
    /// step's `then` ops where the prediction began; wrong counts a wrong
    /// attempt, which makes "Show me" available. Nothing happens with no
    /// option selected or once the prediction is answered.
    pub fn check(&mut self, session: &mut Session) -> ModuleView {
        let answer = match self.current() {
            Some(Step::Predict { answer, .. }) => *answer,
            _ => return self.view(session),
        };
        let Some(predict) = self.predict.as_mut() else {
            return self.view(session);
        };
        match predict.selected {
            Some(selected) if !predict.answered() => {
                if selected == answer {
                    predict.feedback = Some(Answer::Correct);
                    self.answered(session);
                } else {
                    predict.feedback = Some(Answer::Wrong);
                    predict.wrong_attempts = predict.wrong_attempts.saturating_add(1);
                }
            }
            _ => {}
        }
        self.view(session)
    }

    /// "Show me". On an instruct step, applies its `showMe` ops, each an op
    /// of its own, which leaves the cursor at the end of the last, so the
    /// step can complete. On an observe step, moves the cursor to the first
    /// position of the displayed run where `until` holds. On a predict step,
    /// once a wrong answer has been checked, reveals the answer, lifts the
    /// gate and applies the step's `then` ops where the prediction began.
    /// Then re-evaluates, as [`ModuleRun::sync`] does.
    ///
    /// Ops the state refuses (an instruct step's, or a predict step's
    /// revealed `then`) leave a notice on the view and the step waiting.
    pub fn show_me(&mut self, session: &mut Session) -> ModuleView {
        match self.current() {
            Some(Step::Instruct {
                show_me: Some(ops), ..
            }) => {
                // The ops were checked when the module was read; one the
                // state refuses, such as at the session's limit, leaves a
                // notice, and the step waits for the student.
                self.notice = session
                    .apply_all(ops.clone())
                    .err()
                    .map(|error| error.to_string());
            }
            Some(Step::Observe { until, .. }) => {
                if let Some(pos) = session.find_event_since(until, self.since) {
                    session.seek(pos);
                }
            }
            Some(Step::Predict { .. }) => {
                if let Some(predict) = self.predict.as_mut().filter(|predict| predict.can_show()) {
                    predict.feedback = Some(Answer::Shown);
                    self.answered(session);
                }
            }
            _ => {}
        }
        self.sync(session)
    }

    /// Next: on an explanation, or on a prediction answered or revealed,
    /// goes on to the next step; on the done screen, starts the module
    /// again, as [`ModuleRun::start`] does. Nothing happens anywhere else.
    ///
    /// A restart the state refuses leaves a notice on the view and the done
    /// screen as it is.
    pub fn next(&mut self, session: &mut Session) -> ModuleView {
        match self.current() {
            Some(Step::Explain { .. }) => self.advance(session),
            Some(Step::Predict { .. }) => {
                if self.predict.is_some_and(Prediction::answered) {
                    self.advance(session);
                }
            }
            Some(_) => {}
            None => {
                // A restart the session refuses, at its limit, leaves the
                // done screen as it is, with a notice why.
                self.notice = self.start(session).err().map(|error| error.to_string());
            }
        }
        self.view(session)
    }

    /// Leaves the module: lifts the gate, and leaves the workbench as it is.
    pub fn exit(&mut self, session: &mut Session) {
        session.clear_gate();
    }

    /// What the module panel draws, with `session` as it is.
    pub fn view(&self, session: &Session) -> ModuleView {
        let module = &self.module;
        let count = u32::try_from(module.steps().len()).unwrap_or(u32::MAX);
        let step = self.current();
        let kind = step.map_or(ModuleStepKind::Done, Step::view_kind);
        let text = step.map_or_else(
            || {
                Segments::new()
                    .plain(&explain::module_complete())
                    .into_vec()
            },
            |step| segments(step.text()),
        );
        let hint = match step {
            Some(Step::Instruct {
                hint: Some(hint), ..
            }) => Some(segments(hint)),
            _ => None,
        };
        let (options, feedback) = match (step, self.predict) {
            (
                Some(Step::Predict {
                    options,
                    answer,
                    explain,
                    hint,
                    ..
                }),
                predict,
            ) => {
                let predict = predict.unwrap_or(Prediction::new(session.cursor_position()));
                let options = (0_u32..)
                    .zip(options)
                    .map(|(index, text)| OptionView {
                        text: segments(text),
                        state: predict.state(index, *answer),
                    })
                    .collect();
                let feedback = predict
                    .feedback
                    .map(|answer| feedback(answer, explain, hint.as_deref()));
                (Some(options), feedback)
            }
            _ => (None, None),
        };
        ModuleView {
            id: module.id().to_owned(),
            title: module.title().to_owned(),
            description: segments(module.description()),
            step_index: self.step,
            step_count: count,
            kind,
            kind_label: explain::module_step_kind(kind),
            progress_text: explain::module_progress(self.step, count),
            text,
            hint,
            options,
            feedback,
            callout: step.and_then(Step::callout),
            notice: self.notice.clone(),
            actions: self.actions(),
            drawer_summary: explain::module_running(
                module.number(),
                module.title(),
                self.step,
                count,
            ),
        }
    }

    /// The current step; none on the done screen.
    fn current(&self) -> Option<&Step> {
        self.module.steps().get(usize::try_from(self.step).ok()?)
    }

    /// Goes on to the next step, or to the done screen after the last.
    fn advance(&mut self, session: &mut Session) {
        self.step = self.step.saturating_add(1);
        self.enter(session);
    }

    /// Begins the current step on `session`: an op made from now on is made
    /// since it began, and a prediction begins at the cursor, which the
    /// gate then holds it to. Clears any notice the previous step left.
    fn enter(&mut self, session: &mut Session) {
        self.since = session.last_serial();
        self.predict = match self.current() {
            Some(Step::Predict { .. }) => Some(Prediction::new(session.cursor_position())),
            _ => None,
        };
        self.notice = None;
        self.gate(session);
    }

    /// Sets the session's gate while the current step is an unanswered
    /// prediction, and lifts it otherwise.
    fn gate(&self, session: &mut Session) {
        match self.predict {
            Some(predict) if !predict.answered() => session.set_gate(predict.limit),
            _ => session.clear_gate(),
        }
    }

    /// Once the current prediction is answered or revealed: lifts the gate
    /// and applies the step's `then` ops where the prediction began, so they
    /// go on from the state it asked about, even if the student has moved
    /// the cursor back since. A refused `then` appends nothing, puts the
    /// cursor back where the student left it, and is kept as the notice.
    fn answered(&mut self, session: &mut Session) {
        session.clear_gate();
        let (
            Some(Step::Predict {
                then: Some(ops), ..
            }),
            Some(predict),
        ) = (self.current(), self.predict)
        else {
            return;
        };
        let back = session.cursor_position();
        session.seek(predict.limit);
        // As for "Show me": checked when the module was read, and a refused
        // `then` appends nothing and puts the cursor back where the student
        // left it.
        let error = session.apply_all(ops.clone()).err();
        if error.is_some() {
            session.seek(back);
        }
        self.notice = error.map(|error| error.to_string());
    }

    /// The panel's buttons: Next on an explanation; Show me on an instruct
    /// step that has ops for it and on an observe step; on an unanswered
    /// prediction, Check, live once an option is selected, and Show me once
    /// a wrong answer has been checked; Next once it is answered or
    /// revealed; Restart on the done screen.
    fn actions(&self) -> Vec<ModuleAction> {
        let live = |action| ModuleAction {
            action,
            enabled: true,
        };
        match self.current() {
            None => vec![live(ModuleActionKind::Restart)],
            Some(Step::Explain { .. }) => vec![live(ModuleActionKind::Next)],
            Some(Step::Instruct { show_me, .. }) => show_me
                .iter()
                .map(|_| live(ModuleActionKind::ShowMe))
                .collect(),
            Some(Step::Observe { .. }) => vec![live(ModuleActionKind::ShowMe)],
            Some(Step::Predict { .. }) => match self.predict {
                Some(predict) if predict.answered() => vec![live(ModuleActionKind::Next)],
                predict => {
                    let mut actions = vec![ModuleAction {
                        action: ModuleActionKind::Check,
                        enabled: predict.is_some_and(|predict| predict.selected.is_some()),
                    }];
                    if predict.is_some_and(Prediction::can_show) {
                        actions.push(live(ModuleActionKind::ShowMe));
                    }
                    actions
                }
            },
        }
    }
}

impl Prediction {
    /// A prediction beginning at position `limit`, with nothing selected.
    fn new(limit: EventPos) -> Self {
        Self {
            selected: None,
            feedback: None,
            wrong_attempts: 0,
            limit,
        }
    }

    /// Whether it has been answered correctly or revealed.
    fn answered(self) -> bool {
        matches!(self.feedback, Some(Answer::Correct | Answer::Shown))
    }

    /// Whether "Show me" may reveal it: it is unanswered, and a wrong answer
    /// has been checked.
    fn can_show(self) -> bool {
        !self.answered() && self.wrong_attempts > 0
    }

    /// How option `index` is drawn, `answer` being the right option.
    fn state(self, index: u32, answer: u32) -> OptionState {
        let selected = self.selected == Some(index);
        match self.feedback {
            Some(Answer::Correct) if selected => OptionState::Correct,
            Some(Answer::Wrong) if selected => OptionState::Wrong,
            Some(Answer::Shown) if index == answer => OptionState::Shown,
            _ if selected => OptionState::Selected,
            _ => OptionState::Idle,
        }
    }
}

impl Step {
    /// The step's kind, as the panel shows it.
    fn view_kind(&self) -> ModuleStepKind {
        match self {
            Step::Instruct { .. } => ModuleStepKind::Instruct,
            Step::Predict { .. } => ModuleStepKind::Predict,
            Step::Observe { .. } => ModuleStepKind::Observe,
            Step::Explain { .. } => ModuleStepKind::Explain,
        }
    }
}

/// The feedback of `answer`: a wrong answer is given the hint, or the
/// explanation when there is none; a right or revealed one the explanation.
fn feedback(answer: Answer, explain: &str, hint: Option<&str>) -> Feedback {
    let (tone, text) = match answer {
        Answer::Correct => (FeedbackTone::Correct, segments(explain)),
        Answer::Wrong => (FeedbackTone::Incorrect, segments(hint.unwrap_or(explain))),
        Answer::Shown => (
            FeedbackTone::Shown,
            Segments::new()
                .plain(&explain::answer_shown())
                .then(segments(explain))
                .into_vec(),
        ),
    };
    Feedback {
        tone,
        head: explain::feedback_head(tone),
        text,
    }
}

/// `text`, a module's text, as segments. Text is checked when the module is
/// read, so it always parses; were it not to, it would read as written.
fn segments(text: &str) -> Vec<TextSegment> {
    markup(text).unwrap_or_else(|_| Segments::new().plain(text).into_vec())
}
