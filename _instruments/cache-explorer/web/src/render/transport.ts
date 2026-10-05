// The transport bar: the operation, address and value boxes with Issue, the step, run and undo
// buttons, the "?" that opens the shortcuts sheet, and the status line under them. Which buttons
// are live is the core's word (`controls`); what the boxes say of the typed text is its word too.

import type { ControlAction } from '../../pkg/cache_explorer';
import { commit, control, type Put } from '../controls';
import { copy } from '../copy';
import { h } from '../dom';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';
import { renderSheet } from './sheet';

/** The parts of the bar a root keeps between renders. */
export interface Transport {
  element: HTMLElement;
  /** The bar's content: rebuilt on every render, but for the boxes in it. */
  body: HTMLElement;
  /** The status line: kept, so that a render that changes nothing announces nothing. */
  status: HTMLElement;
}

export function createTransport(): Transport {
  const body = h('div', { class: 'cache-explorer__transport-body' });
  const status = h('div', { class: 'cache-explorer__status', role: 'status' });
  return { element: h('div', { class: 'cache-explorer__transport' }, body, status), body, status };
}

/** A box for typed text: it lives across renders, so that what is typed in it is never rebuilt. */
function box(
  root: HTMLElement,
  key: 'addr' | 'value',
  label: string,
  invalid: boolean,
): HTMLInputElement {
  const input = control(root, key, () =>
    h('input', {
      type: 'text',
      class: `cache-explorer__field cache-explorer__field--${key === 'addr' ? 'address' : 'value'}`,
      'data-action': key,
      'aria-label': label,
      autocomplete: 'off',
      spellcheck: 'false',
    }),
  );
  if (invalid) input.setAttribute('aria-invalid', 'true');
  else input.removeAttribute('aria-invalid');
  return input;
}

function labelled(name: string, input: Node): HTMLElement {
  return h(
    'label',
    { class: 'cache-explorer__typed' },
    h('span', { class: 'cache-explorer__label' }, name),
    input,
  );
}

function operation(ui: UiState): HTMLElement {
  const option = (label: string, kind: string): HTMLElement =>
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__seg-option',
        'data-action': 'kind',
        'data-option': kind,
        'aria-pressed': String(ui.draft.kind === kind),
      },
      label,
    );
  return h(
    'div',
    {
      class: 'cache-explorer__seg cache-explorer__seg--inline',
      role: 'group',
      'aria-label': copy.transport.operation,
    },
    option(copy.transport.load, 'read'),
    option(copy.transport.store, 'write'),
  );
}

/** One of the buttons of the segmented row: its key is its title, and what assistive tech is told. */
function tool(
  label: string,
  action: string,
  keys: string,
  enabled: boolean,
  guard: string | undefined,
  pressed?: boolean,
): HTMLElement {
  return h(
    'button',
    {
      type: 'button',
      class: 'cache-explorer__seg-option',
      'data-action': action,
      'aria-keyshortcuts': keys === '' ? null : keys,
      'aria-pressed': pressed === undefined ? null : String(pressed),
      disabled: !enabled,
      title: !enabled && guard ? guard : keys === '' ? null : keys,
    },
    label,
  );
}

/** Share, Save and Load: the design's three words at the right of the bar, plain text until pointed at. */
function linkButton(label: string, action: string, enabled = true, guard?: string): HTMLElement {
  return h(
    'button',
    {
      type: 'button',
      class: 'cache-explorer__link-button',
      'data-action': action,
      disabled: !enabled,
      title: !enabled && guard ? guard : null,
    },
    label,
  );
}

/** The link in a read-only field, for a page whose clipboard is not there; the page selects it. */
function shareLink(root: HTMLElement, put: Put, link: string): HTMLElement {
  return h(
    'div',
    { class: 'cache-explorer__share-link' },
    h('span', { class: 'cache-explorer__label' }, copy.transport.copyThisLink),
    put(
      control(root, 'share-link', () =>
        h('input', {
          type: 'text',
          class: 'cache-explorer__field',
          'data-action': 'share-link',
          'aria-label': copy.transport.linkLabel,
          readonly: true,
          spellcheck: 'false',
        }),
      ),
      link,
    ),
    h(
      'button',
      { type: 'button', class: 'cache-explorer__link-button', 'data-action': 'share-close' },
      copy.transport.closeLink,
    ),
  );
}

export function renderTransport(
  root: HTMLElement,
  transport: Transport,
  snap: Snapshot,
  ui: UiState,
): void {
  const { controls } = snap.state;
  const on = (control: ControlAction): boolean => controls.enabled.includes(control);
  const { draft } = snap;
  const addressBad = draft.address.status === 'invalid' || draft.address.status === 'misaligned';
  const store = ui.draft.kind === 'write';
  const guard = controls.guard;
  const { running } = ui;
  commit(transport.body, (put: Put) => [
    h(
      'div',
      { class: 'cache-explorer__transport-row' },
      h(
        'div',
        { class: 'cache-explorer__issue' },
        operation(ui),
        labelled(
          copy.transport.address,
          put(box(root, 'addr', copy.transport.addressLabel, addressBad), ui.draft.addrText),
        ),
        store &&
          labelled(
            copy.transport.value,
            put(
              box(root, 'value', copy.transport.valueLabel, draft.value?.status === 'invalid'),
              ui.draft.valueText,
            ),
          ),
        h(
          'button',
          {
            type: 'button',
            class: 'cache-explorer__button cache-explorer__button--primary',
            'data-action': 'issue',
            disabled: !on('issue'),
            title: !on('issue') ? guard : null,
          },
          copy.transport.issue,
        ),
      ),
      h(
        'div',
        {
          class: 'cache-explorer__seg cache-explorer__seg--inline cache-explorer__seg--tools',
          role: 'group',
          'aria-label': copy.transport.group,
        },
        tool(copy.transport.step, 'step', 'Space', on('step'), guard),
        tool(copy.transport.stepAccess, 'step-access', 'Shift+Space', on('stepAccess'), guard),
        tool(
          running ? copy.transport.pause : copy.transport.run,
          'run',
          'R',
          running || on('run'),
          guard,
          running,
        ),
        tool(copy.transport.reset, 'reset', '', on('reset'), guard),
        tool(copy.transport.undo, 'undo', 'Z', on('undo'), guard),
        tool(copy.transport.redo, 'redo', 'Y', on('redo'), guard),
      ),
      h(
        'div',
        { class: 'cache-explorer__transport-end' },
        linkButton(copy.transport.share, 'share'),
        linkButton(copy.transport.save, 'save'),
        linkButton(copy.transport.openFile, 'load', on('load'), guard),
        h(
          'button',
          {
            type: 'button',
            class: 'cache-explorer__keys-button',
            'data-action': 'sheet',
            'aria-label': copy.transport.shortcuts,
            'aria-expanded': String(ui.sheetOpen),
          },
          '?',
        ),
      ),
    ),
    ...(ui.shareFallback === null ? [] : [shareLink(root, put, ui.shareFallback)]),
    ...(ui.sheetOpen ? [renderSheet()] : []),
  ]);
  if (transport.status.textContent !== ui.status) transport.status.textContent = ui.status;
}
