// Focus kept across a render. Render rebuilds the DOM, so the element that had focus is usually
// replaced; before rendering the controller records the element and which one it was by its action
// and parameters. Afterwards an element that survived keeps focus, or takes it back when moving it
// cost it; a replaced one is followed by the new element with the same key, or by the plane when
// there is none. The plane takes focus without a tab stop (`tabindex="-1"`), so the next shortcut
// still reaches the root.

import { PARAMS, readAction, type Action } from './actions';

/** The plane's class: the element that takes focus when the focused element has no successor. */
export const PLANE_CLASS = 'cache-explorer__plane';

/** Which element had focus: its action and parameters, or null for one without an action. */
export type FocusKey = Pick<Action, 'name' | 'params'>;

export interface FocusMemory {
  /** The element that had focus. */
  readonly element: HTMLElement | SVGElement;
  /** Which element it was, for finding its successor; null for one without an action. */
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
  if (!(active instanceof HTMLElement || active instanceof SVGElement) || !root.contains(active)) {
    return null;
  }
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
  return { element: active, key: focusKey(active), selection };
}

/**
 * Puts focus back after a render: the element that had it, when it is still in the tool; else the
 * element with the recorded key, or else the plane.
 */
export function restoreFocus(root: HTMLElement, memory: FocusMemory | null): void {
  if (!memory) return;
  const { element } = memory;
  const survived = element.isConnected && root.contains(element);
  // An element that kept focus is left alone: setting its caret again would end a composition.
  if (survived && root.ownerDocument.activeElement === element) return;
  const successor = survived ? element : memory.key && findByKey(root, memory.key);
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
