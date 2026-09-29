import { describe, expect, it, vi } from 'vitest';
import {
  couldNotStartState,
  loadingState,
  narrowState,
  noWebAssemblyState,
} from '../src/page-states';

describe('the loading state', () => {
  it('is a busy plane with the name, the status and four grey rows, and no spinner', () => {
    const plane = loadingState();
    expect(plane.tagName).toBe('SECTION');
    expect(plane.getAttribute('aria-label')).toBe('Interactive Cache Explorer');
    expect(plane.getAttribute('aria-busy')).toBe('true');
    expect(plane.querySelector('.cache-explorer__head')?.textContent).toBe(
      'Interactive Cache ExplorerLoading',
    );
    const rows = plane.querySelector('.cache-explorer__skeleton');
    expect(rows?.getAttribute('aria-hidden')).toBe('true');
    expect(rows?.children).toHaveLength(4);
    expect(plane.querySelector('svg, [role="progressbar"]')).toBeNull();
  });
});

describe('the notices', () => {
  it('say that WebAssembly is missing and which browsers have it', () => {
    const plane = noWebAssemblyState();
    expect(plane.getAttribute('aria-label')).toBe('Interactive Cache Explorer');
    expect(plane.getAttribute('aria-busy')).toBeNull();
    expect(plane.querySelector('.cache-explorer__head')?.textContent).toBe(
      'Interactive Cache Explorer',
    );
    expect(plane.querySelector('.cache-explorer__lead')?.textContent).toBe(
      'The Interactive Cache Explorer needs WebAssembly, which this browser does not provide.',
    );
    expect(plane.querySelector('.cache-explorer__text')?.textContent).toBe(
      'Any current version of Chrome, Edge, Firefox or Safari will run it.',
    );
  });

  it('say that the screen is too narrow, and how wide it has to be', () => {
    const plane = narrowState();
    expect(plane.querySelector('.cache-explorer__lead')?.textContent).toBe(
      'The Interactive Cache Explorer needs a wider screen.',
    );
    expect(plane.querySelector('.cache-explorer__text')?.textContent).toBe(
      'The cache table, memory and log have to sit side by side to be readable. Open this page on a tablet or a computer, at least 700 pixels wide.',
    );
  });
});

describe('the could-not-start state', () => {
  it('says what happened and offers Reload', () => {
    const plane = couldNotStartState(vi.fn());
    expect(plane.querySelector('.cache-explorer__head')?.textContent).toBe(
      'Interactive Cache ExplorerCould not start',
    );
    const problem = plane.querySelector('.cache-explorer__problem');
    expect(problem?.textContent).toBe(
      "The page's files are out of step, usually after an update. Reload to fix it.",
    );
    expect(problem?.getAttribute('role')).toBe('alert');
    expect(plane.querySelector('button')?.textContent).toBe('Reload');
  });

  it('reloads when the button is pressed, and not before', () => {
    const reload = vi.fn();
    const plane = couldNotStartState(reload);
    expect(reload).not.toHaveBeenCalled();
    plane.querySelector('button')?.click();
    expect(reload).toHaveBeenCalledOnce();
  });
});
