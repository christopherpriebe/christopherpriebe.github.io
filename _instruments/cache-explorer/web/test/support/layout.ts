import { vi } from 'vitest';

/** Records what it watches, and reports a size when a test says the element has one. */
class FakeResizeObserver implements ResizeObserver {
  static readonly all: FakeResizeObserver[] = [];
  readonly watching = new Set<Element>();

  constructor(readonly callback: ResizeObserverCallback) {
    FakeResizeObserver.all.push(this);
  }

  observe(target: Element): void {
    this.watching.add(target);
  }

  unobserve(target: Element): void {
    this.watching.delete(target);
  }

  disconnect(): void {
    this.watching.clear();
  }
}

export interface Widths {
  /** Sets the width `root` measures, without telling the observers. */
  set(root: HTMLElement, width: number): void;
  /** Sets the width and reports it to every observer watching `root`, as a resize does. */
  resize(root: HTMLElement, width: number): void;
  /** How many observers are watching `root` now. */
  observers(root: HTMLElement): number;
}

/**
 * jsdom does no layout and has no ResizeObserver. This gives the tool's elements the widths a test
 * sets (1,000px, a desktop, until it does) and observers that fire when the test resizes them.
 * Vitest undoes it after each test.
 */
export function stubLayout(defaultWidth = 1000): Widths {
  const widths = new WeakMap<Element, number>();
  const widthOf = (element: Element): number => widths.get(element) ?? defaultWidth;
  vi.stubGlobal('ResizeObserver', FakeResizeObserver);
  FakeResizeObserver.all.length = 0;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    return new DOMRect(0, 0, widthOf(this), 100);
  });
  return {
    set(root, width) {
      widths.set(root, width);
    },
    resize(root, width) {
      widths.set(root, width);
      const entry = { target: root, contentRect: new DOMRect(0, 0, width, 100) };
      for (const observer of FakeResizeObserver.all) {
        if (observer.watching.has(root)) {
          observer.callback([entry as unknown as ResizeObserverEntry], observer);
        }
      }
    },
    observers(root) {
      return FakeResizeObserver.all.filter((observer) => observer.watching.has(root)).length;
    },
  };
}
