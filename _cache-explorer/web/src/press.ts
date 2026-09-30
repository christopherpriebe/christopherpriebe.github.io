// A press on a control that has not been released. A render rebuilds the elements it draws, and a
// browser dispatches a click only when the element the button went down on is still in the page
// when it comes up. Leaving a text field commits it on `change`, which fires between the press on
// the next control and its release; a render then would replace that control under the pointer,
// and the click would be dropped. The controller holds its redraws back while a press is down and
// makes them once it is over, which is after the click.

/** The elements a press has to leave in place: the ones a click on would do something. */
const CONTROLS = '[data-action], button, input, textarea, label, a, summary';

/** Controls that act on `input` and are never rebuilt, so a redraw cannot drop a click on them. */
const LIVE = 'input[type="range"]';

/**
 * How long a press may be held back from redrawing before it is taken to be over. A release that
 * never reaches the page would otherwise leave the tool as it was drawn; no click takes this long.
 */
export const PRESS_LIMIT_MS = 3000;

export interface Press {
  /** Whether a press that began on a control inside the root is still down. */
  readonly down: boolean;
  /** Stops watching. */
  stop(): void;
}

/**
 * Watches the primary button, which is the only one that clicks. `released` is called once the
 * press is over, after the click it makes, if it makes one; it is not called for a press on
 * anything but a control. A list (`<select>`) and a slider (`<input type=range>`) are not: the first it hands the press to a native popup,
 * which may keep the release from the page, and it acts on `change`, not on a click; the slider is drawn again as it is dragged. A release
 * outside the root, or a press that never sees one, ends it too.
 */
export function watchPress(root: HTMLElement, released: () => void): Press {
  const stop = new AbortController();
  const { signal } = stop;
  const view = root.ownerDocument.defaultView;
  let down = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let limit: ReturnType<typeof setTimeout> | undefined;

  function release(): void {
    if (!down) return;
    down = false;
    clearTimeout(limit);
    // The click is dispatched right after the release, in the same task; a timer runs after it.
    timer = setTimeout(released, 0);
  }

  root.addEventListener(
    'mousedown',
    (event) => {
      if (event.button !== 0 || !(event.target instanceof Element)) return;
      const control = event.target.closest(CONTROLS);
      if (!control || control instanceof HTMLSelectElement || control.matches(LIVE)) return;
      clearTimeout(timer);
      clearTimeout(limit);
      down = true;
      limit = setTimeout(release, PRESS_LIMIT_MS);
    },
    { capture: true, signal },
  );
  // Anything that ends a press without a `mouseup` reaching the root: a release elsewhere, a drag
  // that took the press over, a context menu, and the window losing focus.
  for (const type of ['mouseup', 'dragend', 'contextmenu'] as const) {
    root.ownerDocument.addEventListener(type, release, { capture: true, signal });
  }
  view?.addEventListener('blur', release, { signal });

  return {
    get down() {
      return down;
    },
    stop() {
      clearTimeout(timer);
      clearTimeout(limit);
      stop.abort();
    },
  };
}
