import { beforeEach, describe, expect, it, vi } from 'vitest';
import { h } from '../src/dom';
import { layoutFor, measureLayout, watchLayout } from '../src/layout';
import { stubLayout, type Widths } from './support/layout';

describe('layoutFor', () => {
  it.each([
    [2000, 'desktop'],
    [880, 'desktop'],
    [879.5, 'tablet'],
    [600, 'tablet'],
    [599.5, 'narrow'],
    [0, 'narrow'],
  ] as const)('puts %fpx in %s', (width, layout) => {
    expect(layoutFor(width)).toBe(layout);
  });
});

describe('the watched root', () => {
  let widths: Widths;
  let root: HTMLElement;

  beforeEach(() => {
    widths = stubLayout();
    root = h('div');
    document.body.replaceChildren(root);
  });

  it('is measured on its own width, not the window', () => {
    vi.stubGlobal('innerWidth', 300);
    widths.set(root, 700);
    expect(measureLayout(root)).toBe('tablet');
  });

  it('gets its layout at once, and at each crossing of a threshold', () => {
    widths.set(root, 1000);
    const changes: string[] = [];
    watchLayout(root, (layout) => changes.push(layout));
    expect(root.dataset.layout).toBe('desktop');

    widths.resize(root, 850);
    expect(root.dataset.layout).toBe('tablet');
    widths.resize(root, 700);
    widths.resize(root, 500);
    expect(root.dataset.layout).toBe('narrow');
    widths.resize(root, 1200);
    expect(root.dataset.layout).toBe('desktop');

    // 850 to 700 crosses nothing, so it is not reported.
    expect(changes).toEqual(['tablet', 'narrow', 'desktop']);
  });

  it('reports nothing for a resize inside one layout', () => {
    const onChange = vi.fn();
    watchLayout(root, onChange);
    widths.resize(root, 1300);
    widths.resize(root, 900);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('stops watching when told to', () => {
    const stop = watchLayout(root);
    expect(widths.observers(root)).toBe(1);
    stop();
    expect(widths.observers(root)).toBe(0);
  });
});
