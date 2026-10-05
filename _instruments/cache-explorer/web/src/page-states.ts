// The states of the page that are not the workbench: loading, a browser without WebAssembly, a
// container too narrow for the workbench, and a start that failed. Each is one plane, built here
// and put into the root by whoever knows which state the page is in.

import type { LoadFailure, ShareSummary } from '../pkg/cache_explorer';
import { copy } from './copy';
import { h, type Child } from './dom';
import { replayedTicks } from './render/log';
import { segments } from './render/segments';
import type { SourceKind } from './ui-state';

const SKELETON_ROWS = 4;

/** The plane's head: the tool's name at the left, and a status at the right when there is one. */
function head(status?: string): HTMLElement {
  return h(
    'div',
    { class: 'cache-explorer__head' },
    h('span', { class: 'cache-explorer__label' }, copy.title),
    status !== undefined && h('span', { class: 'cache-explorer__label' }, status),
  );
}

function plane(modifier: string, busy: boolean, ...content: Child[]): HTMLElement {
  return h(
    'section',
    {
      class: `cache-explorer__plane cache-explorer__plane--${modifier}`,
      'aria-label': copy.title,
      'aria-busy': busy ? 'true' : null,
    },
    ...content,
  );
}

/** A statement with what to do about it: the name, a lead sentence and a line under it. */
function notice(lead: string, text: string): HTMLElement {
  return plane(
    'notice',
    false,
    head(),
    h('p', { class: 'cache-explorer__lead' }, lead),
    h('p', { class: 'cache-explorer__text' }, text),
  );
}

/** Grey rows where the workbench will be; no spinner. */
export function loadingState(): HTMLElement {
  const rows = Array.from({ length: SKELETON_ROWS }, () => h('div'));
  return plane(
    'loading',
    true,
    head(copy.loading),
    h('div', { class: 'cache-explorer__skeleton', 'aria-hidden': 'true' }, ...rows),
  );
}

export function noWebAssemblyState(): HTMLElement {
  return notice(copy.notices.noWebAssembly.lead, copy.notices.noWebAssembly.text);
}

export function narrowState(): HTMLElement {
  return notice(copy.notices.narrow.lead, copy.notices.narrow.text);
}

/** The page's files could not be started, most often a stale cached script beside a new wasm. */
export function couldNotStartState(reload: () => void): HTMLElement {
  const button = h(
    'button',
    { type: 'button', class: 'cache-explorer__button cache-explorer__button--primary' },
    copy.reload,
  );
  button.addEventListener('click', reload);
  return plane(
    'failed',
    false,
    head(copy.couldNotStart),
    h('p', { class: 'cache-explorer__problem', role: 'alert' }, copy.couldNotStartMessage),
    h('div', { class: 'cache-explorer__actions' }, button),
  );
}

/** A shared run being replayed: the core's line about it, and a tick for each step replayed so far. */
export function replayingState(summary: ShareSummary | null, from: SourceKind): HTMLElement {
  return plane(
    'loading',
    true,
    head(copy.loading),
    h(
      'p',
      { class: 'cache-explorer__lead' },
      from === 'json' ? copy.opening.fileLead : copy.opening.lead,
    ),
    summary && h('p', { class: 'cache-explorer__text' }, summary.text),
    summary && replayedTicks(summary.ticks),
    h(
      'div',
      { class: 'cache-explorer__skeleton', 'aria-hidden': 'true' },
      ...Array.from({ length: SKELETON_ROWS }, () => h('div')),
    ),
  );
}

function action(name: string, label: string, primary = false): HTMLElement {
  return h(
    'button',
    {
      type: 'button',
      class: primary
        ? 'cache-explorer__button cache-explorer__button--primary'
        : 'cache-explorer__button',
      'data-action': name,
    },
    label,
  );
}

/**
 * A link or a file that cannot be opened: the core's lead, its message with the field in mono, and
 * the ways forward it found. `keep` adds the way back to the run the page was showing.
 */
export function failedLoadState(
  failure: LoadFailure,
  from: SourceKind,
  keep: boolean,
  contentsShown: boolean,
): HTMLElement {
  return plane(
    'failed',
    false,
    head(failure.title),
    h('p', { class: 'cache-explorer__lead' }, failure.lead),
    h('p', { class: 'cache-explorer__problem', role: 'alert' }, ...segments(failure.message)),
    h(
      'div',
      { class: 'cache-explorer__actions' },
      action('load-fresh', copy.opening.startFresh, true),
      failure.repair && action('load-repair', failure.repair.label),
      failure.prefix && action('load-prefix', failure.prefix.label),
      keep && action('load-keep', copy.opening.keepRun),
      failure.contents !== undefined &&
        h(
          'button',
          {
            type: 'button',
            class: 'cache-explorer__text-button',
            'data-action': 'load-contents',
            'aria-expanded': String(contentsShown),
          },
          contentsShown
            ? from === 'json'
              ? copy.opening.hideFileContents
              : copy.opening.hideContents
            : from === 'json'
              ? copy.opening.showFileContents
              : copy.opening.showContents,
        ),
    ),
    contentsShown &&
      failure.contents !== undefined &&
      h('pre', { class: 'cache-explorer__contents', tabindex: 0 }, failure.contents),
  );
}
