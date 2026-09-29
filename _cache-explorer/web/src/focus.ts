// Focus kept across a render. Render rebuilds the DOM, so the element that had focus is replaced;
// before rendering the controller records which one it was by its action and parameters, and
// afterwards focuses the new element with the same key, or the plane when there is none. The plane
// takes focus without a tab stop (`tabindex="-1"`), so the next shortcut still reaches the root.

import { PARAMS, readAction, type Action } from './actions';

/** The plane's class: the element that takes focus when the focused element has no successor. */
export const PLANE_CLASS = 'cache-explorer__plane';

/** Which element had focus: its action and parameters, or null for one without an action. */
export type FocusKey = Pick<Action, 'name' | 'params'>;

export interface FocusMemory {
  readonly key: FocusKey | null;
  /** The caret or selection of a text control, put back after focus returns. */
  readonly selection: {
    start: number;
    end: number;
    direction: 'forward' | 'backward' | 'none';
  } | null;
}

/** The key of an element that carries an action; the parameters that are absent stay absent. */
export function focusKey(element: Element): FocusKey | null {
  const action = readAction(element);
  return action && { name: action.name, params: action.params };
}

/** The element inside `root` whose action and parameters are exactly `key`'s, if there is one. */
export function findByKey(root: HTMLElement, key: FocusKey): HTMLElement | null {
  for (const candidate of root.querySelectorAll<HTMLElement>('[data-action]')) {
    const action = readAction(candidate);
    if (action?.name === key.name && PARAMS.every((p) => action.params[p] === key.params[p])) {
      return candidate;
    }
  }
  return null;
}

/** What has focus inside `root`, or null when focus is elsewhere. Call it before rendering. */
export function captureFocus(root: HTMLElement): FocusMemory | null {
  const active = root.ownerDocument.activeElement;
  if (!active || !root.contains(active)) return null;
  const selection =
    (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) &&
    active.selectionStart !== null &&
    active.selectionEnd !== null
      ? {
          start: active.selectionStart,
          end: active.selectionEnd,
          direction: active.selectionDirection ?? 'none',
        }
      : null;
  return { key: focusKey(active), selection };
}

/** Puts focus back after a render: on the element with the recorded key, or else on the plane. */
export function restoreFocus(root: HTMLElement, memory: FocusMemory | null): void {
  if (!memory) return;
  const successor = memory.key && findByKey(root, memory.key);
  const target = successor ?? root.querySelector<HTMLElement>(`.${PLANE_CLASS}`);
  if (!target) return;
  target.focus({ preventScroll: true });
  if (
    memory.selection &&
    (successor instanceof HTMLInputElement || successor instanceof HTMLTextAreaElement)
  ) {
    const { start, end, direction } = memory.selection;
    successor.setSelectionRange(start, end, direction);
  }
}
