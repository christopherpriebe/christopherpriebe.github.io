import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { h } from '../src/dom';
import { boot } from '../src/main';

const wasm = vi.hoisted(() => ({ init: vi.fn(), buildInfo: vi.fn() }));
vi.mock('../src/wasm', () => wasm);

/** A promise the test settles by hand, standing for the wasm's download and compile. */
function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

let root: HTMLElement;

beforeEach(() => {
  wasm.buildInfo.mockReturnValue({ version: '1.2.3', schemaVersion: 4 });
  root = h('div', { id: 'cache-explorer' });
  document.body.replaceChildren(root);
  document.head.replaceChildren();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('boot', () => {
  it('shows the loading state only once 200 ms have passed', async () => {
    const started = deferred();
    wasm.init.mockReturnValue(started.promise);
    const booted = boot(root);

    await vi.advanceTimersByTimeAsync(199);
    expect(root.querySelector('.cache-explorer__plane')).toBeNull();

    await vi.advanceTimersByTimeAsync(1);
    const plane = root.querySelector('section.cache-explorer__plane');
    expect(plane?.getAttribute('aria-label')).toBe('Interactive Cache Explorer');
    expect(plane?.getAttribute('aria-busy')).toBe('true');
    expect(plane?.textContent).toContain('Loading');
    expect(root.querySelectorAll('.cache-explorer__skeleton > div')).toHaveLength(4);

    started.resolve();
    await booted;
    expect(root.querySelector('.cache-explorer__skeleton')).toBeNull();
    expect(root.querySelector('.cache-explorer__plane')?.getAttribute('aria-busy')).toBe('false');
  });

  it('goes straight to the ready state when the wasm is quick', async () => {
    const started = deferred();
    wasm.init.mockReturnValue(started.promise);
    const booted = boot(root);

    await vi.advanceTimersByTimeAsync(100);
    started.resolve();
    await booted;
    await vi.advanceTimersByTimeAsync(1000);

    expect(root.querySelector('.cache-explorer__skeleton')).toBeNull();
    expect(root.textContent).toContain('Ready');
    expect(root.textContent).toContain('Core version 1.2.3.');
  });

  it('gives the root its block class', async () => {
    wasm.init.mockResolvedValue(undefined);
    await boot(root);
    expect(root.classList.contains('cache-explorer')).toBe(true);
  });

  it('lets the generated glue find the wasm in development', async () => {
    wasm.init.mockResolvedValue(undefined);
    await boot(root);
    expect(wasm.init).toHaveBeenCalledExactlyOnceWith(undefined);
    expect(document.head.querySelector('link')).toBeNull();
  });

  it('waits for the stylesheet in production, then starts the wasm from beside the script', async () => {
    vi.stubEnv('PROD', true);
    wasm.init.mockResolvedValue(undefined);
    const booted = boot(root);

    const link = document.head.querySelector('link[rel="stylesheet"]');
    expect(link?.getAttribute('href')).toMatch(/\/cache-explorer\.css\?v=test$/);
    await vi.advanceTimersByTimeAsync(1000);
    expect(wasm.init).not.toHaveBeenCalled();

    link?.dispatchEvent(new Event('load'));
    await booted;
    expect(wasm.init).toHaveBeenCalledOnce();
    const [argument] = wasm.init.mock.calls[0] as [{ module_or_path: URL }];
    expect(argument.module_or_path.pathname).toMatch(/\/cache_explorer_bg\.wasm$/);
    expect(argument.module_or_path.searchParams.get('v')).toBe('test');
  });

  it('starts without styles when the stylesheet fails', async () => {
    vi.stubEnv('PROD', true);
    wasm.init.mockResolvedValue(undefined);
    const booted = boot(root);

    document.head.querySelector('link')?.dispatchEvent(new Event('error'));
    await booted;
    expect(root.textContent).toContain('Ready');
  });
});
