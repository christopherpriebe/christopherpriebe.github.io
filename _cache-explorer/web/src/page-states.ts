// The states of the page that are not the workbench: loading, a browser without WebAssembly, a
// container too narrow for the workbench, and a start that failed. Each is one plane, built here
// and put into the root by whoever knows which state the page is in.

import { copy } from './copy';
import { h } from './dom';

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

function plane(modifier: string, busy: boolean, ...content: Node[]): HTMLElement {
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
