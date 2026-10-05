// The configuration bar: the summary line with its Basics and Advanced switch, the groups of
// options under it, and the question "Reset to apply?" when a change needs a new run. Everything
// it draws comes from `snapshot.config`, which the core builds for the configuration on show (the
// run's, or the pending one); an option carries the whole configuration a click on it produces,
// so nothing here works out what an option does.

import type {
  ConfigGroup,
  ConfigGroupKind,
  ConfigNumberField,
  ConfigOption,
  TimingControl,
} from '../../pkg/cache_explorer';
import { commit, control, type Put } from '../controls';
import { copy } from '../copy';
import { h, type Attrs, type Child } from '../dom';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';

/** The parts of the bar a root keeps between renders. */
export interface ConfigBar {
  /** The bar, ruled above and below. */
  element: HTMLElement;
  /** The summary line and the groups: rebuilt on every render. */
  body: HTMLElement;
  /**
   * The question. Kept, and only rewritten when its text changes, so that a render which changes
   * nothing about it neither announces it again nor takes focus off its buttons.
   */
  prompt: HTMLElement;
}

export function createConfigBar(): ConfigBar {
  const body = h('div', { class: 'cache-explorer__config-body' });
  const prompt = h('div', { class: 'cache-explorer__pending', role: 'alert' });
  return { element: h('div', { class: 'cache-explorer__config' }, body), body, prompt };
}

/**
 * What each cell of the bar is called in `data-group`, and what a log line's `why:` link names.
 * The address width and the word size share one cell.
 */
type Cell = Exclude<ConfigGroupKind, 'addressBits' | 'wordSize'> | 'address' | 'timing';

/** How a render sees the bar: the configuration on show, and whether it may be changed. */
interface Bar {
  readonly root: HTMLElement;
  readonly snap: Snapshot;
  readonly ui: UiState;
  readonly put: Put;
  /** Whether the core lets the configuration change now (it does not while a prediction waits). */
  readonly live: boolean;
  /** Why it does not, for the title of what is switched off. */
  readonly guard: string | undefined;
  /** How far into its flash the group a log line pointed at is, or null when none is flashing. */
  readonly flashAge: number | null;
}

/** A button of a segmented control: pressed is the navy one. */
function segOption(label: string, pressed: boolean, attrs: Attrs): HTMLButtonElement {
  return h(
    'button',
    {
      type: 'button',
      class: 'cache-explorer__seg-option',
      'aria-pressed': String(pressed),
      ...attrs,
    },
    label,
  );
}

function detailLevel(advanced: boolean): HTMLElement {
  const option = (label: string, key: string, pressed: boolean): HTMLElement =>
    segOption(label, pressed, { 'data-action': 'detail', 'data-option': key });
  return h(
    'div',
    {
      class: 'cache-explorer__seg cache-explorer__seg--inline',
      role: 'group',
      'aria-label': copy.config.detailLevel,
    },
    option(copy.config.basics, 'basics', !advanced),
    option(copy.config.advanced, 'advanced', advanced),
  );
}

function summaryLine(bar: Bar): HTMLElement {
  const open = bar.ui.configOpen;
  return h(
    'div',
    { class: 'cache-explorer__config-head' },
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__config-toggle',
        'data-action': 'config-toggle',
        'aria-expanded': String(open),
      },
      h('span', { class: 'cache-explorer__config-glyph', 'aria-hidden': 'true' }, open ? '−' : '+'),
      h('span', { class: 'cache-explorer__config-title' }, copy.config.title),
      h('span', null, bar.snap.config.summary),
    ),
    detailLevel(bar.snap.state.advanced),
  );
}

/** One group's box: its name over its content, flashing when a log line has pointed at it. */
function cell(bar: Bar, key: Cell, readOnly: boolean, ...content: Child[]): HTMLElement {
  const flashing = bar.ui.flash?.cause === key;
  const elapsed = flashing ? Math.round(bar.flashAge ?? 0) : 0;
  return h(
    'div',
    {
      class: flashing
        ? 'cache-explorer__group cache-explorer__group--why'
        : 'cache-explorer__group',
      'data-group': key,
      // A render in the middle of the flash carries it on from where it had got.
      style: elapsed > 0 ? `animation-delay: -${String(elapsed)}ms` : null,
    },
    h('span', { class: 'cache-explorer__group-label' }, copy.config.groups[key]),
    ...content,
    readOnly && h('p', { class: 'cache-explorer__note' }, copy.config.readOnlyNote),
  );
}

/** Why an option or a field is switched off: the core's own reason for an option, else the gate's. */
function whyOff(bar: Bar, option?: ConfigOption): string | undefined {
  return option?.disabled ? option.title : bar.live ? undefined : bar.guard;
}

/**
 * The buttons of one group, as a segmented control that wraps onto rows when it is narrow. A group
 * Basics hides, drawn because a log line points at it, is read-only: its buttons do nothing, but
 * the current choice stays as it is, since that is what the line points at. That is not the look
 * of a button the core switches off, which every group can have.
 */
function options(bar: Bar, group: ConfigGroup, label: string, readOnly: boolean): HTMLElement {
  return h(
    'div',
    {
      class: readOnly
        ? 'cache-explorer__seg cache-explorer__seg--read-only'
        : 'cache-explorer__seg',
      role: 'group',
      'aria-label': label,
    },
    ...group.options.map((option, index) =>
      segOption(option.label, option.pressed, {
        'data-action': 'config-option',
        'data-group': group.group,
        'data-option': index,
        disabled: option.disabled || !bar.live,
        'aria-disabled': readOnly && !option.disabled && bar.live ? 'true' : null,
        title: whyOff(bar, option),
      }),
    ),
  );
}

function optionGroup(bar: Bar, kind: Cell, group: ConfigGroup, readOnly: boolean): HTMLElement {
  return cell(bar, kind, readOnly, options(bar, group, copy.config.groups[kind], readOnly));
}

/** The address-width list, which is one `<select>` for the life of the root. */
function widthSelect(bar: Bar, group: ConfigGroup): HTMLSelectElement {
  const select = control(bar.root, 'address-bits', () =>
    h('select', {
      class: 'cache-explorer__select',
      'data-action': 'address-bits',
      'aria-label': copy.config.address.addressLabel,
    }),
  );
  const same =
    select.options.length === group.options.length &&
    group.options.every((option, i) => {
      const shown = select.options[i];
      return (
        shown?.textContent === option.label &&
        shown.disabled === option.disabled &&
        shown.title === (option.title ?? '')
      );
    });
  if (!same) {
    select.replaceChildren(
      ...group.options.map((option, i) =>
        h('option', { value: i, disabled: option.disabled, title: option.title }, option.label),
      ),
    );
  }
  // The index, not the label, says which option is shown; the option list is the core's.
  const index = group.options.findIndex((option) => option.pressed);
  if (select.selectedIndex !== index) select.selectedIndex = index;
  select.disabled = !bar.live;
  select.title = bar.live ? '' : (bar.guard ?? '');
  return select;
}

/** The address width and the word size, as a summary line that opens into their two controls. */
function addressGroup(bar: Bar): HTMLElement {
  const { config } = bar.snap;
  const width = config.groups.find((group) => group.group === 'addressBits');
  const word = config.groups.find((group) => group.group === 'wordSize');
  const toggle = (label: string, open: boolean): HTMLElement =>
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__link',
        'data-action': 'address-toggle',
        'aria-expanded': String(open),
      },
      label,
    );
  if (!width || !word) return cell(bar, 'address', false);
  if (!bar.ui.addressOpen) {
    return cell(
      bar,
      'address',
      false,
      h(
        'div',
        { class: 'cache-explorer__summary-line' },
        config.addressSummary,
        ' ',
        toggle(copy.config.address.change, false),
      ),
    );
  }
  return cell(
    bar,
    'address',
    false,
    h(
      'div',
      { class: 'cache-explorer__address' },
      h('span', { class: 'cache-explorer__caption' }, copy.config.address.addressCaption),
      bar.put(widthSelect(bar, width)),
      h('span', { class: 'cache-explorer__caption' }, copy.config.address.wordCaption),
      options(bar, word, copy.config.address.wordLabel, false),
      toggle(copy.config.address.done, true),
    ),
  );
}

const TIMING_NOTE_ID = 'cache-explorer-timing-note';

/** One of the two timing fields, kept for the life of the root so the student's typing survives. */
function timingField(
  bar: Bar,
  spec: { action: string; field: ConfigNumberField; label: string; text: string },
  off: boolean,
): Node {
  const input = control(bar.root, spec.action, () =>
    h('input', {
      type: 'text',
      inputmode: 'numeric',
      autocomplete: 'off',
      spellcheck: 'false',
      class: 'cache-explorer__field',
      'data-action': spec.action,
      'aria-label': spec.label,
    }),
  );
  const wrong = bar.ui.timingError?.field === spec.field;
  input.disabled = off;
  input.title = bar.live ? '' : (bar.guard ?? '');
  if (wrong) {
    input.setAttribute('aria-invalid', 'true');
    input.setAttribute('aria-describedby', TIMING_NOTE_ID);
  } else {
    input.removeAttribute('aria-invalid');
    input.removeAttribute('aria-describedby');
  }
  // What the student typed stays in a field that does not read, so they can mend it.
  return wrong ? bar.put(input) : bar.put(input, spec.text);
}

function timingGroup(bar: Bar, timing: TimingControl, readOnly: boolean): HTMLElement {
  const { ui } = bar;
  const off = readOnly || !bar.live;
  const { timing: words } = copy.config;
  const hit = timingField(
    bar,
    { action: 'timing-hit', field: 'hitCycles', label: words.hitLabel, text: timing.hitText },
    off,
  );
  const miss = timingField(
    bar,
    { action: 'timing-miss', field: 'missCycles', label: words.missLabel, text: timing.missText },
    off,
  );
  return cell(
    bar,
    'timing',
    readOnly,
    h(
      'div',
      { class: 'cache-explorer__timing' },
      h('span', null, words.hit),
      hit,
      h('span', null, words.cycles),
      h('span', null, words.miss),
      miss,
      h('span', null, words.cycles),
    ),
    // The note sits with the field it is about, not under the checkbox.
    ui.timingError &&
      h('p', { class: 'cache-explorer__note', id: TIMING_NOTE_ID }, ui.timingError.message),
    h(
      'label',
      { class: 'cache-explorer__check' },
      h('input', {
        type: 'checkbox',
        'data-action': 'timed',
        checked: timing.on,
        disabled: off,
        title: whyOff(bar),
      }),
      words.timed,
    ),
  );
}

/**
 * The groups the bar shows now, in the order the core lists them: the address width and the word
 * size are one cell, drawn where the first of them is, and the timing controls, which are not a
 * list of options, come last.
 */
function groups(bar: Bar): HTMLElement {
  const { snap, ui } = bar;
  const { advanced } = snap.state;
  // A log line can point at a group Basics hides; it is drawn, read-only, for as long as it flashes.
  const flashed = (kind: Cell): boolean => ui.flash?.cause === kind;
  const cells: HTMLElement[] = [];
  let addressDrawn = false;
  for (const group of snap.config.groups) {
    const kind = group.group;
    if (kind === 'addressBits' || kind === 'wordSize') {
      if (!addressDrawn) cells.push(addressGroup(bar));
      addressDrawn = true;
      continue;
    }
    const readOnly = group.advancedOnly && !advanced;
    if (!readOnly || flashed(kind)) cells.push(optionGroup(bar, kind, group, readOnly));
  }
  if (advanced || flashed('timing')) cells.push(timingGroup(bar, snap.config.timing, !advanced));
  return h('div', { class: 'cache-explorer__config-groups' }, ...cells);
}

/** Shows the question for the pending change, or takes it away. */
function question(bar: ConfigBar, ui: UiState): void {
  const message = ui.pending === null ? null : ui.pendingMessage;
  if (message === null) {
    bar.prompt.remove();
    return;
  }
  if (bar.prompt.dataset.message !== message) {
    bar.prompt.dataset.message = message;
    bar.prompt.replaceChildren(
      h('span', { class: 'cache-explorer__pending-mark', 'aria-hidden': 'true' }),
      h('span', { class: 'cache-explorer__pending-title' }, copy.config.pending.title),
      h('span', { class: 'cache-explorer__pending-text' }, message),
      h(
        'button',
        {
          type: 'button',
          class: 'cache-explorer__button cache-explorer__button--primary',
          'data-action': 'apply-pending',
        },
        copy.config.pending.apply,
      ),
      h(
        'button',
        { type: 'button', class: 'cache-explorer__button', 'data-action': 'keep-run' },
        copy.config.pending.keep,
      ),
    );
  }
  if (!bar.prompt.isConnected) bar.element.append(bar.prompt);
}

/** Draws the bar into the parts a root keeps. */
export function renderConfigBar(
  root: HTMLElement,
  bar: ConfigBar,
  snap: Snapshot,
  ui: UiState,
): void {
  const { controls } = snap.state;
  commit(bar.body, (put) => {
    const drawing: Bar = {
      root,
      snap,
      ui,
      put,
      live: controls.enabled.includes('configure'),
      guard: controls.guard,
      flashAge: snap.flashAge,
    };
    return [
      summaryLine(drawing),
      snap.config.error && h('p', { class: 'cache-explorer__problem' }, snap.config.error.message),
      ui.configOpen && groups(drawing),
    ].filter((node): node is HTMLElement => node instanceof HTMLElement);
  });
  question(bar, ui);
}
