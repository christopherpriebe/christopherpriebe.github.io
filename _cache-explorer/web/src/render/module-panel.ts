// The module panel, under the configuration bar: the module's name, a dot for each step, the step's
// kind, then its text, the options of a prediction, what the core said of the answer, and the
// buttons. Everything on it is the core's: which buttons are shown and live, how each option is
// marked, the words of the feedback. The region the step points at is outlined by spotlight.ts.

import type { ModuleAction, ModuleView, OptionView } from '../../pkg/cache_explorer';
import { copy } from '../copy';
import { cls, h } from '../dom';
import { segments } from './segments';

/** The kind of a button decides whether it is the filled one. */
const PRIMARY = new Set(['check', 'next', 'restart']);

const ACTION_NAMES = {
  check: 'module-check',
  next: 'module-next',
  showMe: 'module-show-me',
  restart: 'module-restart',
} as const;

/** One dot for each step: done steps filled, the step in hand larger, steps to come hollow. */
function dots(view: ModuleView): HTMLElement {
  return h(
    'span',
    { class: 'cache-explorer__module-dots', role: 'img', 'aria-label': view.progressText },
    ...Array.from({ length: view.stepCount }, (_, index) =>
      h('span', {
        class: cls(
          'cache-explorer__module-dot',
          index < view.stepIndex && 'cache-explorer__module-dot--done',
          index === view.stepIndex && 'cache-explorer__module-dot--here',
        ),
      }),
    ),
  );
}

/** Whether an option's answer has been given, which locks the choice. */
function answered(options: readonly OptionView[]): boolean {
  return options.some((option) => option.state === 'correct' || option.state === 'shown');
}

function option(view: OptionView, index: number, locked: boolean): HTMLElement {
  const note = copy.modules.optionStates[view.state];
  return h(
    'button',
    {
      type: 'button',
      class: cls('cache-explorer__module-option', `cache-explorer__module-option--${view.state}`),
      'data-action': 'module-select',
      'data-option': index,
      'aria-pressed': String(view.state !== 'idle'),
      disabled: locked,
    },
    h('span', { class: 'cache-explorer__module-mark', 'aria-hidden': 'true' }),
    ...segments(view.text),
    note !== '' && h('span', { class: 'cache-explorer__visually-hidden' }, ` (${note})`),
  );
}

function button(action: ModuleAction): HTMLElement {
  return h(
    'button',
    {
      type: 'button',
      class: cls(
        'cache-explorer__button',
        PRIMARY.has(action.action) && 'cache-explorer__button--primary',
      ),
      'data-action': ACTION_NAMES[action.action],
      disabled: !action.enabled,
    },
    copy.modules.actions[action.action],
  );
}

/** Draws the panel into `panel`, or empties it when no module is running. */
export function renderModulePanel(panel: HTMLElement, view: ModuleView | undefined): void {
  if (!view) {
    panel.replaceChildren();
    panel.removeAttribute('aria-label');
    return;
  }
  panel.setAttribute('aria-label', copy.modules.panel);
  const { options, feedback } = view;
  panel.replaceChildren(
    h(
      'div',
      { class: 'cache-explorer__module-main' },
      h(
        'div',
        { class: 'cache-explorer__module-meta' },
        h('span', { class: 'cache-explorer__label' }, `${copy.modules.module} · ${view.title}`),
        dots(view),
        h('span', { class: 'cache-explorer__label' }, `${view.progressText} · ${view.kindLabel}`),
      ),
      h('p', { class: 'cache-explorer__module-text' }, ...segments(view.text)),
      view.hint &&
        h(
          'p',
          { class: 'cache-explorer__module-hint' },
          h('span', { class: 'cache-explorer__label' }, copy.modules.hint),
          ' ',
          ...segments(view.hint),
        ),
      options &&
        h(
          'div',
          { class: 'cache-explorer__module-options', role: 'group', 'aria-label': view.kindLabel },
          ...options.map((each, index) => option(each, index, answered(options))),
        ),
      feedback &&
        h(
          'div',
          {
            class: cls(
              'cache-explorer__module-note',
              `cache-explorer__module-note--${feedback.tone}`,
            ),
            role: 'status',
          },
          h('b', null, feedback.head),
          ' ',
          ...segments(feedback.text),
        ),
      view.notice !== undefined &&
        h(
          'div',
          {
            class: 'cache-explorer__module-note cache-explorer__module-note--refused',
            role: 'status',
          },
          view.notice,
        ),
    ),
    h(
      'div',
      { class: 'cache-explorer__module-actions' },
      ...view.actions.map(button),
      h(
        'button',
        {
          type: 'button',
          class: 'cache-explorer__module-exit',
          'data-action': 'module-exit',
        },
        copy.modules.exit,
      ),
    ),
  );
}
