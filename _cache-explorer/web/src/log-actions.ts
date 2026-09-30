// What the log, its ticks and its scrubber do: each moves the cursor to a position the core
// gave, and scrolling the log moves the window of entries the log builds.

import type { Action } from './actions';
import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { LOG_ROW_PX } from './metrics';
import { firstInView } from './render/log';

/** The entries built above the first one in view, so a short scroll finds entries already there. */
export const LOG_MARGIN = 4;

function move(context: Context, to: () => void): void {
  try {
    to();
  } catch (error) {
    context.ui.status = refusal(error);
  }
  context.refresh();
}

/** An entry's header or a row of its events carries the position it goes to. */
function seek(context: Context, action: Action): void {
  const { pos } = action.params;
  if (pos !== undefined)
    move(context, () => {
      context.session.seek(Number(pos));
    });
}

/** A tick carries the step it goes to. */
function seekTick(context: Context, action: Action): void {
  const { step } = action.params;
  if (step !== undefined)
    move(context, () => {
      context.session.seekStep(Number(step));
    });
}

/**
 * The scrubber counts steps of the run. The cursor moves at most once an animation frame, to the
 * latest step asked for.
 */
function scrub(context: Context, action: Action): void {
  const { ui } = context;
  if (!(action.element instanceof HTMLInputElement)) return;
  ui.scrub.step = Number(action.element.value);
  if (ui.scrub.frame !== null) return;
  ui.scrub.frame = requestAnimationFrame(() => {
    ui.scrub.frame = null;
    move(context, () => {
      context.session.seekStep(ui.scrub.step);
    });
  });
}

/** The log was scrolled: build the entries around the ones now in view. */
export function scrollLog(context: Context, scroller: HTMLElement): void {
  const { ui } = context;
  // Once entries are built, the first one in view is the one to build around: their heights are
  // known and the blank rows' are only estimated.
  const row = firstInView(scroller);
  const anchor = row?.dataset.entry === undefined ? null : Number(row.dataset.entry);
  const first = Math.max(0, (anchor ?? Math.floor(scroller.scrollTop / LOG_ROW_PX)) - LOG_MARGIN);
  if (first === ui.logFirst) return;
  ui.logFirst = first;
  context.refresh();
}

/** Stops the scrub that is waiting for its frame; the tool is done with the log. */
export function stopScrub(ui: Context['ui']): void {
  if (ui.scrub.frame !== null) cancelAnimationFrame(ui.scrub.frame);
  ui.scrub.frame = null;
}

/** The log's handlers, by the `data-action` and the event they answer. */
export const logActions = {
  clicks: new Map<string, ActionHandler>([
    ['seek-tick', seekTick],
    ['seek-entry', seek],
    ['seek-event', seek],
  ]),
  inputs: new Map<string, ActionHandler>([['scrub', scrub]]),
};
