// One listener per event type on the root, so a render that rebuilds the DOM loses none of them.
// Each handler gets the event and the action of the element it came from, or null when that
// element is not inside an actionable one.

import { actionAt, type Action } from './actions';

type DelegatedEvent = 'click' | 'input' | 'change' | 'keydown' | 'keyup' | 'focusin' | 'scroll';

export type Handlers = {
  [K in DelegatedEvent]?: (event: HTMLElementEventMap[K], action: Action | null) => void;
};

/** Listens on the root for the events in `handlers`; returns the function that removes them. */
export function delegate(root: HTMLElement, handlers: Handlers): () => void {
  const stop = new AbortController();

  function listen<K extends DelegatedEvent>(type: K, handler: Handlers[K]): void {
    if (!handler) return;
    root.addEventListener(
      type,
      (event) => {
        handler(event, actionAt(event.target, root));
      },
      // A scroll does not bubble, so it is heard on the way down.
      { signal: stop.signal, capture: type === 'scroll' },
    );
  }

  listen('click', handlers.click);
  listen('input', handlers.input);
  listen('change', handlers.change);
  listen('keydown', handlers.keydown);
  listen('keyup', handlers.keyup);
  listen('focusin', handlers.focusin);
  listen('scroll', handlers.scroll);
  return () => {
    stop.abort();
  };
}
