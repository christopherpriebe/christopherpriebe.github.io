import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { h } from '../src/dom';
import { boot } from '../src/main';
import type * as wasmModule from '../src/wasm';
import { Session } from '../src/wasm';
import { stubLayout, type Widths } from './support/layout';
import { loadWasm } from './support/wasm';

// Only the download of the wasm is under the test's control; the wasm that runs is the shipped one.
const wasm = vi.hoisted(() => ({ init: vi.fn() }));
vi.mock('../src/wasm', async () => {
  const actual: typeof wasmModule = await vi.importActual('../src/wasm');
  return { ...actual, init: wasm.init };
});

/** A promise the test settles by hand, standing for the wasm's download and compile. */
function deferred(): {
  promise: Promise<void>;
  resolve: () => void;
  reject: (reason: Error) => void;
} {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const LOADING = '.cache-explorer__plane--loading';
const WORKBENCH = '.cache-explorer__plane--workbench';

let root: HTMLElement;
let widths: Widths;

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { id: 'cache-explorer' });
  document.body.replaceChildren(root);
  document.head.replaceChildren();
  widths = stubLayout(1000);
  vi.spyOn(console, 'error').mockImplementation(() => undefined);
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('boot', () => {
  it('shows the loading state only once 200 ms have passed, then the workbench', async () => {
    const started = deferred();
    wasm.init.mockReturnValue(started.promise);
    const booted = boot(root);

    await vi.advanceTimersByTimeAsync(199);
    expect(root.querySelector('.cache-explorer__plane')).toBeNull();

    await vi.advanceTimersByTimeAsync(1);
    const plane = root.querySelector(LOADING);
    expect(plane?.getAttribute('aria-label')).toBe('Interactive Cache Explorer');
    expect(plane?.getAttribute('aria-busy')).toBe('true');
    expect(plane?.textContent).toContain('Loading');
    expect(root.querySelectorAll('.cache-explorer__skeleton > div')).toHaveLength(4);

    started.resolve();
    await booted;
    expect(root.querySelector(LOADING)).toBeNull();
    expect(root.querySelector(WORKBENCH)?.textContent).toContain('Basics · 0 steps in this run');
  });

  it('goes straight to the workbench when the wasm is quick, with no flash of the loading state', async () => {
    const started = deferred();
    wasm.init.mockReturnValue(started.promise);
    const seen: MutationRecord[] = [];
    const watch = new MutationObserver((records) => seen.push(...records));
    watch.observe(root, { childList: true });
    const booted = boot(root);

    await vi.advanceTimersByTimeAsync(100);
    started.resolve();
    await booted;
    await vi.advanceTimersByTimeAsync(1000);

    seen.push(...watch.takeRecords());
    watch.disconnect();
    const added = seen.flatMap((record) => [...record.addedNodes]);
    expect(added).toHaveLength(1);
    expect((added[0] as Element).matches(WORKBENCH)).toBe(true);
  });

  it('gives the root its block class and its layout', async () => {
    wasm.init.mockResolvedValue(undefined);
    await boot(root);
    expect(root.classList.contains('cache-explorer')).toBe(true);
    expect(root.dataset.layout).toBe('desktop');
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
    expect(root.querySelector(WORKBENCH)).not.toBeNull();
  });
});

describe('a browser without WebAssembly', () => {
  it('gets the notice, and the wasm is never fetched', async () => {
    vi.stubGlobal('WebAssembly', undefined);
    await boot(root);
    expect(root.querySelector('.cache-explorer__lead')?.textContent).toBe(
      'The Interactive Cache Explorer needs WebAssembly, which this browser does not provide.',
    );
    expect(root.textContent).toContain('Any current version of Chrome, Edge, Firefox or Safari');
    expect(wasm.init).not.toHaveBeenCalled();
  });
});

describe('a start that fails', () => {
  it('shows "Could not start" with Reload when the wasm cannot be loaded', async () => {
    const failure = new Error('LinkError: import object field is not a Function');
    wasm.init.mockRejectedValue(failure);
    const reload = vi.fn();
    await boot(root, reload);

    expect(root.querySelector('.cache-explorer__head')?.textContent).toContain('Could not start');
    expect(root.querySelector('.cache-explorer__problem')?.textContent).toBe(
      'The Interactive Cache Explorer could not start. Reload to try again.',
    );
    expect(console.error).toHaveBeenCalledWith(expect.any(String), failure);

    expect(reload).not.toHaveBeenCalled();
    root.querySelector('button')?.click();
    expect(reload).toHaveBeenCalledOnce();
  });

  it('shows it in place of the loading state when the wasm fails after 200 ms', async () => {
    const started = deferred();
    wasm.init.mockReturnValue(started.promise);
    const booted = boot(root);
    await vi.advanceTimersByTimeAsync(300);
    expect(root.querySelector(LOADING)).not.toBeNull();

    started.reject(new Error('network'));
    await booted;
    expect(root.querySelector(LOADING)).toBeNull();
    expect(root.textContent).toContain('Could not start');
  });

  it('shows it, and starts no loading state later, when the wasm fails at once', async () => {
    wasm.init.mockRejectedValue(new Error('network'));
    await boot(root);
    await vi.advanceTimersByTimeAsync(1000);
    expect(root.querySelector(LOADING)).toBeNull();
    expect(root.textContent).toContain('Could not start');
  });

  it('shows it when the first session cannot be made', async () => {
    wasm.init.mockResolvedValue(undefined);
    vi.spyOn(Session, 'firstVisit').mockImplementation(() => {
      throw new Error('unreachable');
    });
    await boot(root);
    expect(root.textContent).toContain('Could not start');
  });

  it('frees the session, and shows it, when the first draw fails', async () => {
    wasm.init.mockResolvedValue(undefined);
    const free = vi.spyOn(Session.prototype, 'free');
    vi.spyOn(Session.prototype, 'state').mockImplementation(() => {
      throw new Error('the core said no');
    });
    await boot(root);
    expect(root.textContent).toContain('Could not start');
    expect(free).toHaveBeenCalledOnce();
  });

  it('keeps the state through a change of layout', async () => {
    wasm.init.mockRejectedValue(new Error('network'));
    await boot(root);
    widths.resize(root, 500);
    expect(root.dataset.layout).toBe('narrow');
    expect(root.textContent).toContain('Could not start');
  });
});

describe('a root too narrow for the workbench', () => {
  it('is shown the notice, not the loading state, and then the notice again once running', async () => {
    widths.set(root, 500);
    const started = deferred();
    wasm.init.mockReturnValue(started.promise);
    const booted = boot(root);
    expect(root.dataset.layout).toBe('narrow');

    await vi.advanceTimersByTimeAsync(300);
    expect(root.querySelector(LOADING)).toBeNull();
    expect(root.querySelector('.cache-explorer__lead')?.textContent).toContain('wider screen');

    started.resolve();
    await booted;
    expect(root.querySelector('.cache-explorer__lead')?.textContent).toContain('wider screen');
    expect(root.querySelector(WORKBENCH)).toBeNull();
  });

  it('gets the workbench when the root widens, and the notice when it narrows again', async () => {
    widths.set(root, 500);
    wasm.init.mockResolvedValue(undefined);
    await boot(root);

    widths.resize(root, 700);
    expect(root.dataset.layout).toBe('tablet');
    expect(root.querySelector(WORKBENCH)).not.toBeNull();

    widths.resize(root, 550);
    expect(root.querySelector(WORKBENCH)).toBeNull();
    expect(root.querySelector('.cache-explorer__lead')).not.toBeNull();

    widths.resize(root, 1200);
    expect(root.dataset.layout).toBe('desktop');
    expect(root.querySelector(WORKBENCH)).not.toBeNull();
  });

  it('swaps the loading state and the notice as the root changes width while loading', async () => {
    wasm.init.mockReturnValue(deferred().promise);
    void boot(root);
    await vi.advanceTimersByTimeAsync(300);
    expect(root.querySelector(LOADING)).not.toBeNull();

    widths.resize(root, 400);
    expect(root.querySelector(LOADING)).toBeNull();
    expect(root.querySelector('.cache-explorer__lead')).not.toBeNull();

    widths.resize(root, 900);
    expect(root.querySelector(LOADING)).not.toBeNull();
  });
});
