// What the configuration bar's controls do. A choice goes to the core as the configuration the
// option carries; the core applies it, or answers that it needs a new run and gives the question,
// which the bar shows until the student answers it.

import type {
  Cause,
  Config,
  ConfigChange,
  ConfigNumberField,
  ConfigOption,
} from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { copy } from './copy';
import type { UiState } from './ui-state';

/** The groups a log line's `why:` link can name. */
const CAUSES: readonly Cause[] = [
  'lineSize',
  'associativity',
  'replacement',
  'writePolicy',
  'allocatePolicy',
  'timing',
];

/** The two timing fields, by the action they carry. */
const TIMING_FIELDS = new Map<string, ConfigNumberField>([
  ['timing-hit', 'hitCycles'],
  ['timing-miss', 'missCycles'],
]);

/** Drops the pending configuration, and everything that was about it. */
function clearPending(ui: UiState): void {
  ui.pending = null;
  ui.pendingMessage = null;
  ui.pendingBasics = false;
  ui.timingError = null;
}

/** Takes what the core said about a change: it happened, or it needs a new run and asks first. */
function settle(ui: UiState, change: ConfigChange, basics: boolean): void {
  if (change.status === 'applied') {
    clearPending(ui);
    return;
  }
  ui.pending = change.config;
  ui.pendingMessage = change.message;
  ui.pendingBasics = basics;
}

/** The configuration the bar shows: the pending one, or the run's. */
function shown(context: Context): Config {
  return context.ui.pending ?? context.session.state().config;
}

/** The options of a group of the bar as it is now. */
function optionsOf(context: Context, group: string | undefined): readonly ConfigOption[] {
  const controls = context.session.configControls(shown(context));
  return controls.groups.find((each) => each.group === group)?.options ?? [];
}

/** Sends a configuration to the core, which applies it or asks; a refusal goes to the status line. */
function propose(context: Context, config: Config): void {
  const { ui } = context;
  ui.timingError = null;
  try {
    settle(ui, context.session.proposeConfig(config), ui.pendingBasics);
  } catch (error) {
    ui.status = refusal(error);
  }
  context.refresh();
}

/** What the student has typed in a timing field, read by the core: a change to send, or why not. */
function readTiming(context: Context, action: Action): Config | null {
  const field = TIMING_FIELDS.get(action.name);
  if (!field || !(action.element instanceof HTMLInputElement)) return null;
  const read = context.session.readConfigNumber(field, action.element.value, shown(context));
  context.ui.timingError = read.status === 'invalid' ? { field, message: read.message } : null;
  return read.status === 'valid' ? read.config : null;
}

function toggleBar(context: Context): void {
  context.ui.configOpen = !context.ui.configOpen;
  context.refresh();
}

function toggleAddress(context: Context): void {
  context.ui.addressOpen = !context.ui.addressOpen;
  context.refresh();
}

/** Basics or Advanced: a display choice that starts no run, unless Basics cannot show the run. */
function chooseDetail(context: Context, action: Action): void {
  const advanced = action.params.option === 'advanced';
  const change = context.session.setAdvanced(advanced);
  if (change.status === 'needsReset') {
    settle(context.ui, change, true);
  } else if (advanced && context.ui.pendingBasics) {
    // Choosing Advanced again withdraws the question about leaving it.
    clearPending(context.ui);
  }
  context.refresh();
}

function chooseOption(context: Context, action: Action): void {
  const { group, option } = action.params;
  const chosen = optionsOf(context, group)[Number(option)];
  if (chosen && !chosen.disabled) propose(context, chosen.config);
}

function chooseWidth(context: Context, action: Action): void {
  const { element } = action;
  const chosen =
    element instanceof HTMLSelectElement
      ? optionsOf(context, 'addressBits')[element.selectedIndex]
      : undefined;
  if (chosen && !chosen.disabled) propose(context, chosen.config);
  else context.refresh();
}

/** "Reset and apply": the pending configuration starts a new run, which Undo can leave again. */
function applyPending(context: Context): void {
  const { ui, session } = context;
  if (!ui.pending) return;
  try {
    session.applyOp({ op: 'config', config: ui.pending });
    // The new run fits Basics, so leaving Advanced cannot ask again.
    if (ui.pendingBasics) session.setAdvanced(false);
  } catch (error) {
    ui.status = refusal(error);
    context.refresh();
    return;
  }
  clearPending(ui);
  ui.status = copy.config.applied;
  context.refresh();
}

function keepRun(context: Context): void {
  clearPending(context.ui);
  context.refresh();
}

/** A log line points at the group that made an event happen as it did. */
function why(context: Context, action: Action): void {
  const cause = CAUSES.find((each) => each === action.params.group);
  if (cause) context.flash(cause);
}

/** A timing field is read as the student types in it, so it can say at once when it will not do. */
function typeTiming(context: Context, action: Action): void {
  readTiming(context, action);
  context.refresh();
}

/** A timing field is sent when the student leaves it. */
function sendTiming(context: Context, action: Action): void {
  const config = readTiming(context, action);
  if (config) propose(context, config);
  else context.refresh();
}

function toggleTiming(context: Context): void {
  propose(context, context.session.configControls(shown(context)).timing.toggle);
}

const timingNames = [...TIMING_FIELDS.keys()];

/** The bar's handlers, by the `data-action` and the event they answer. */
export const configActions = {
  clicks: new Map<string, ActionHandler>([
    ['config-toggle', toggleBar],
    ['address-toggle', toggleAddress],
    ['detail', chooseDetail],
    ['config-option', chooseOption],
    ['apply-pending', applyPending],
    ['keep-run', keepRun],
    ['why', why],
  ]),
  inputs: new Map<string, ActionHandler>(timingNames.map((name) => [name, typeTiming])),
  changes: new Map<string, ActionHandler>([
    ...timingNames.map((name): [string, ActionHandler] => [name, sendTiming]),
    ['timed', toggleTiming],
    ['address-bits', chooseWidth],
  ]),
};
