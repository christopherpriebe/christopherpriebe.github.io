// The tool's width class, measured on its own root and not on the viewport, so it works wherever
// the page puts it. The stylesheet reads the class from the root's `data-layout`; the controller
// reads it to decide between the workbench and the notice.

export type Layout = 'desktop' | 'tablet' | 'narrow';

/** The root's inline size, in CSS pixels, from which the workbench is drawn in full. */
export const DESKTOP_MIN_WIDTH = 880;
/** The root's inline size from which it is drawn stacked and tighter; below it, only a notice. */
export const TABLET_MIN_WIDTH = 600;

export function layoutFor(width: number): Layout {
  if (width >= DESKTOP_MIN_WIDTH) return 'desktop';
  if (width >= TABLET_MIN_WIDTH) return 'tablet';
  return 'narrow';
}

/** The layout of the root as it is now. */
export function measureLayout(root: HTMLElement): Layout {
  return layoutFor(root.getBoundingClientRect().width);
}

/**
 * Sets `data-layout` on the root now and whenever its width crosses a threshold, and calls
 * `onChange` with the new layout at each crossing (not for the first measurement, which the caller
 * takes with `measureLayout`). Returns the function that stops watching.
 */
export function watchLayout(root: HTMLElement, onChange?: (layout: Layout) => void): () => void {
  let current = measureLayout(root);
  root.dataset.layout = current;
  const observer = new ResizeObserver((entries) => {
    const entry = entries.at(-1);
    if (!entry) return;
    const next = layoutFor(entry.contentRect.width);
    if (next === current) return;
    current = next;
    root.dataset.layout = next;
    onChange?.(next);
  });
  observer.observe(root);
  return () => {
    observer.disconnect();
  };
}
