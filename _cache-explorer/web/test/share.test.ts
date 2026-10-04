import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { fixtureSource } from '../dev/modules';
import { start, type Controller } from '../src/controller';
import { copy } from '../src/copy';
import { h } from '../src/dom';
import { HASH_DEBOUNCE_MS, linkTo, readHash, type HashStore } from '../src/hash';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

/** The core's words for a run that outgrew a link, as the facade throws them. */
const TOO_LONG =
  'This run is too long for a link: its link would be 70,123 bytes long, and at most 65,536 bytes can be opened. Save it as a file instead.';

/** An address bar of the test's own: what was written, and every write. */
function fakeStore(): HashStore & { hash: string | null; writes: (string | null)[] } {
  const store = {
    hash: null as string | null,
    writes: [] as (string | null)[],
    page: () => 'https://example.test/instruments/cache-explorer/',
    replace(hash: string | null) {
      store.hash = hash;
      store.writes.push(hash);
    },
  };
  return store;
}

let root: HTMLElement;
let controller: Controller | null;
/** The session the controller started with, which a test may move, but never free. */
let lent: Session;
const toFree: Session[] = [];

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  controller = null;
  Element.prototype.scrollIntoView = vi.fn();
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
  })) as typeof window.matchMedia;
});

afterEach(() => {
  vi.useRealTimers();
  controller?.dispose();
  for (const session of toFree.splice(0)) session.free();
  Reflect.deleteProperty(navigator, 'clipboard');
});

function open(
  store: HashStore,
  options: Parameters<typeof start>[4] = {},
  modules: Parameters<typeof start>[3] = null,
): void {
  lent = Session.firstVisit();
  controller = start(root, lent, 'desktop', modules, { store, ...options });
}

const q = (selector: string): HTMLElement | null => root.querySelector<HTMLElement>(selector);
const text = (selector: string): string => q(selector)?.textContent ?? '';
const status = (): string => text('.cache-explorer__status');

function click(name: string): void {
  const button = q(`[data-action="${name}"]`);
  if (!button) throw new Error(`no ${name}`);
  button.click();
}

/** Lets promises and zero-delay timers run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 4; i += 1) await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

/** A run of `count` loads on a first visit's cache, as a saved file and as a link. */
function run(count: number): { json: string; share: string } {
  const session = Session.firstVisit();
  toFree.push(session);
  for (let i = 0; i < count; i += 1) {
    session.applyOp({ op: 'issue', kind: 'read', addr: (i * 16) % 4096 });
  }
  return { json: session.toJson(), share: session.toShareString() };
}

/** The saved run of the first visit with its associativity changed to a value that is no cache. */
function brokenRun(): string {
  return run(2).json.replace('"associativity":1', '"associativity":3');
}

/** The steps the head counts, from "Basics · 15 steps in this run". */
function steps(): number {
  return Number(/(\d+) steps? in this run/.exec(text('.cache-explorer__head'))?.[1]);
}

/** What the head says of a saved run, asked of a session that holds it. */
function headOf(json: string): string {
  const session = Session.fromJson(json);
  toFree.push(session);
  return session.state().labels.statusText;
}

/** Presses Load and has the student choose `file`. */
async function choose(file: File): Promise<void> {
  const pick = vi.spyOn(HTMLInputElement.prototype, 'click').mockImplementation(function (
    this: HTMLInputElement,
  ) {
    Object.defineProperty(this, 'files', { value: [file], configurable: true });
    this.dispatchEvent(new Event('change'));
  });
  click('load');
  await settle();
  pick.mockRestore();
}

describe('the address', () => {
  it('updates the URL hash with replaceState after an op', () => {
    const replace = vi.spyOn(history, 'replaceState');
    lent = Session.firstVisit();
    controller = start(root, lent, 'desktop');
    expect(replace).not.toHaveBeenCalled();
    click('issue');
    expect(replace).toHaveBeenCalledTimes(1);
    expect(location.hash).toBe(`#s=${lent.toShareString()}`);
    click('issue');
    expect(replace).toHaveBeenCalledTimes(2);
    expect(location.hash).toBe(`#s=${lent.toShareString()}`);
    history.replaceState(null, '', '/');
  });

  it('writes a move of the cursor once it has stood still for 250 ms', () => {
    vi.useFakeTimers();
    const store = fakeStore();
    open(store);
    click('issue');
    click('issue');
    const written = store.writes.length;
    click('undo');
    click('undo');
    expect(store.writes).toHaveLength(written);
    vi.advanceTimersByTime(HASH_DEBOUNCE_MS - 1);
    expect(store.writes).toHaveLength(written);
    vi.advanceTimersByTime(1);
    expect(store.writes).toHaveLength(written + 1);
    expect(store.hash).toBe(`#s=${lent.toShareString()}`);
  });

  it('drops #s= and points to Save once the run outgrows a link', () => {
    const store = fakeStore();
    open(store);
    click('issue');
    expect(store.hash).toMatch(/^#s=/);
    vi.spyOn(lent, 'toShareString').mockImplementation(() => {
      throw new Error(TOO_LONG);
    });
    click('issue');
    expect(store.hash).toBeNull();
    expect(status()).toBe(TOO_LONG);
  });

  it('keeps the address free of a link that opens an older run', () => {
    vi.useFakeTimers();
    const store = fakeStore();
    open(store);
    click('issue');
    click('issue');
    vi.spyOn(lent, 'toShareString').mockImplementation(() => {
      throw new Error(TOO_LONG);
    });
    click('undo');
    vi.advanceTimersByTime(HASH_DEBOUNCE_MS);
    expect(store.hash).toBeNull();
    expect(status()).toBe(TOO_LONG);
  });

  it('reads what an address asks for', () => {
    expect(readHash('#s=abc_-')).toEqual({ kind: 'share', text: 'abc_-' });
    expect(readHash('#m=01-a')).toEqual({ kind: 'module', id: '01-a' });
    expect(readHash('')).toBeNull();
    expect(readHash('#s=')).toBeNull();
    expect(readHash('#x=1')).toBeNull();
    expect(linkTo('https://e.test/p/?a=1#old', 'zz')).toBe('https://e.test/p/?a=1#s=zz');
  });
});

describe('Share', () => {
  it("copies the link and says what it holds, in the core's words", async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>().mockResolvedValue();
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    open(fakeStore());
    click('issue');
    click('share');
    await settle();
    expect(writeText).toHaveBeenCalledWith(
      `https://example.test/instruments/cache-explorer/#s=${lent.toShareString()}`,
    );
    expect(status()).toBe(lent.shareSummary().copiedText);
    expect(q('.cache-explorer__share-link')).toBeNull();
  });

  it('shows the link in a selected field when there is no clipboard API', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
    open(fakeStore());
    click('issue');
    click('share');
    await settle();
    const field = root.querySelector<HTMLInputElement>('.cache-explorer__share-link input');
    expect(text('.cache-explorer__share-link')).toContain(copy.transport.copyThisLink);
    expect(field?.value).toBe(
      `https://example.test/instruments/cache-explorer/#s=${lent.toShareString()}`,
    );
    expect(field?.readOnly).toBe(true);
    expect(document.activeElement).toBe(field);
    click('share-close');
    expect(q('.cache-explorer__share-link')).toBeNull();
  });

  it('shows the field too when the clipboard refuses', async () => {
    const writeText = vi.fn<(text: string) => Promise<void>>().mockRejectedValue(new Error('no'));
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    open(fakeStore());
    click('share');
    await settle();
    expect(q('.cache-explorer__share-link input')).not.toBeNull();
  });

  it('says that the run is too long for a link, and offers no field', async () => {
    open(fakeStore());
    vi.spyOn(lent, 'toShareString').mockImplementation(() => {
      throw new Error(TOO_LONG);
    });
    click('share');
    await settle();
    expect(status()).toBe(TOO_LONG);
    expect(q('.cache-explorer__share-link')).toBeNull();
  });
});

describe('Save', () => {
  it('downloads the run as a compact run-date.cache-explorer.json', async () => {
    const blobs: Blob[] = [];
    URL.createObjectURL = vi.fn((blob: Blob) => {
      blobs.push(blob);
      return 'blob:run';
    });
    URL.revokeObjectURL = vi.fn();
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      names.push(this.download);
    });
    open(fakeStore());
    click('issue');
    click('save');
    expect(names).toHaveLength(1);
    expect(names[0]).toMatch(/^run-\d{4}-\d{2}-\d{2}\.cache-explorer\.json$/);
    expect(status()).toBe(`Saved ${names[0] ?? ''}.`);
    expect(await blobs[0]?.text()).toBe(lent.toJson());
  });
});

describe('Load', () => {
  it('replaces the run with a saved file, and writes it into the address', async () => {
    const store = fakeStore();
    open(store);
    const { json, share } = run(3);
    await choose(new File([json], 'mine.cache-explorer.json'));
    expect(status()).toBe(copy.transport.opened('mine.cache-explorer.json'));
    expect(store.hash).toBe(`#s=${share}`);
    expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
    expect(root.querySelectorAll('.cache-explorer__plane')).toHaveLength(1);
    expect(text('.cache-explorer__head')).toContain(headOf(json));
  });

  it('frees the old session before the core replays the new one', async () => {
    open(fakeStore());
    const order: string[] = [];
    const free = lent.free.bind(lent);
    vi.spyOn(lent, 'free').mockImplementation(() => {
      order.push('free');
      free();
    });
    const toJson = vi.spyOn(lent, 'toJson');
    toJson.mockImplementation(() => {
      order.push('toJson');
      return Session.prototype.toJson.call(lent);
    });
    const { json } = run(1);
    await choose(new File([json], 'a.cache-explorer.json'));
    expect(order).toEqual(['toJson', 'free']);
  });

  it('reads a .txt file as a trace into the run', async () => {
    open(fakeStore());
    await choose(new File(['R 0x0A4\nW 0x0A8 7\n'], 'trace.txt'));
    expect(status()).toBe(copy.transport.traceLoaded('trace.txt'));
    expect(lent.opCount()).toBe(1);
    expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
    expect(lent.state().program?.format).toBe('trace');
  });

  it('puts a trace the core cannot read in the status line and leaves the run', async () => {
    open(fakeStore());
    await choose(new File(['this is not a trace'], 'bad.txt'));
    expect(lent.opCount()).toBe(0);
    expect(status()).not.toBe('');
  });

  it('refuses a file larger than the core can open without reading it', async () => {
    open(fakeStore());
    const big = new File(['{}'], 'big.cache-explorer.json');
    Object.defineProperty(big, 'size', { value: 2_000_000 });
    const read = vi.spyOn(big, 'text');
    await choose(big);
    expect(read).not.toHaveBeenCalled();
    expect(status()).toBe(
      'The saved run is 2,000,000 bytes of JSON; at most 1,048,576 bytes can be opened.',
    );
    const trace = new File(['R 0x0A4'], 'big.txt');
    Object.defineProperty(trace, 'size', { value: 20_000 });
    await choose(trace);
    expect(status()).toBe(
      'The trace is 20,000 bytes; traces of at most 16,384 bytes can be opened.',
    );
  });

  describe('a file that cannot be opened', () => {
    async function fail(): Promise<{ store: ReturnType<typeof fakeStore>; head: string }> {
      const store = fakeStore();
      open(store);
      click('issue');
      const head = text('.cache-explorer__head');
      await choose(new File([brokenRun()], 'broken.cache-explorer.json'));
      return { store, head };
    }

    it("shows the core's account of it, with its ways forward", async () => {
      await fail();
      expect(q('.cache-explorer__plane--failed')).not.toBeNull();
      expect(text('.cache-explorer__head')).toContain('Could not open file');
      expect(text('.cache-explorer__lead')).toContain("This file doesn't describe a cache");
      expect(q('.cache-explorer__problem code')?.textContent).toBe('associativity: 3');
      const labels = [...root.querySelectorAll('[data-action^="load-"]')].map((e) => e.textContent);
      expect(labels).toEqual([
        'Start fresh',
        'Open with 2-way instead',
        copy.opening.keepRun,
        copy.opening.showContents,
      ]);
    });

    it('goes back to the run it had when asked to keep it', async () => {
      const { store, head } = await fail();
      const hash = store.hash;
      click('load-keep');
      expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
      expect(text('.cache-explorer__head')).toBe(head);
      expect(store.hash).toBe(hash);
    });

    it('starts fresh from the first visit, with the address bare', async () => {
      const { store } = await fail();
      click('load-fresh');
      expect(steps()).toBe(0);
      expect(store.hash).toBeNull();
      expect(q('.cache-explorer__plane--failed')).toBeNull();
    });

    it("opens the core's repaired alternative", async () => {
      const { store } = await fail();
      click('load-repair');
      expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
      expect(text('.cache-explorer__config')).toContain('2-way');
      expect(store.hash).toMatch(/^#s=/);
    });

    it('shows the contents and hides them again', async () => {
      await fail();
      expect(q('.cache-explorer__contents')).toBeNull();
      click('load-contents');
      expect(text('.cache-explorer__contents')).toContain('"associativity": 3');
      click('load-contents');
      expect(q('.cache-explorer__contents')).toBeNull();
    });

    it('offers the actions before the one that failed, as a prefix', async () => {
      const store = fakeStore();
      open(store);
      const good = run(2).json;
      // A third action that no cache can issue: an address beyond the 12-bit space.
      const broken = good.replace(
        /"ops":\[(.*)\],"at"/,
        '"ops":[$1,{"op":"issue","kind":"read","addr":99999}],"at"',
      );
      await choose(new File([broken], 'late.cache-explorer.json'));
      click('load-prefix');
      expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
      expect(text('.cache-explorer__head')).toContain(headOf(good));
    });
  });
});

describe('opening the page with a link', () => {
  it('opens a link at once, without the loading state, and leaves the address as it is', () => {
    const store = fakeStore();
    const { json, share } = run(2);
    open(store, { open: { source: { kind: 'share', text: share }, since: performance.now() } });
    expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
    expect(q('.cache-explorer__plane--loading')).toBeNull();
    expect(text('.cache-explorer__head')).toContain(headOf(json));
    expect(store.writes).toEqual([]);
  });

  it('restores a link by the core, as the run that was shared', () => {
    const store = fakeStore();
    const { json, share } = run(3);
    open(store, { open: { source: { kind: 'share', text: share }, since: performance.now() } });
    const before = steps();
    expect(before).toBe(Number(/(\d+) steps/.exec(headOf(json))?.[1]));
    click('issue');
    expect(store.hash).toMatch(/^#s=/);
    expect(steps()).toBeGreaterThan(before);
  });

  it('shows the loading state only after 200 ms, its ticks filling as the run replays', () => {
    vi.useFakeTimers();
    const store = fakeStore();
    const { share } = run(900);
    open(store, { open: { source: { kind: 'share', text: share }, since: performance.now() } });
    // The first slice is replayed; no one has waited yet, so nothing of the state is drawn.
    expect(q('.cache-explorer__plane--loading')).toBeNull();
    vi.advanceTimersByTime(250);
    // Late slices follow a frame apart; by now the state shows or the run has opened.
    const shown = q('.cache-explorer__plane--loading');
    if (shown) {
      expect(text('.cache-explorer__lead')).toBe(copy.opening.lead);
      expect(root.querySelectorAll('.cache-explorer__tick').length).toBeGreaterThan(0);
    }
    vi.advanceTimersByTime(5000);
    expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
  });

  it('shows the loading state with its ticks when 200 ms have already passed', () => {
    vi.useFakeTimers();
    const { share } = run(900);
    open(fakeStore(), {
      open: { source: { kind: 'share', text: share }, since: performance.now() - 300 },
    });
    const plane = q('.cache-explorer__plane--loading');
    expect(plane).not.toBeNull();
    expect(plane?.getAttribute('aria-busy')).toBe('true');
    expect(text('.cache-explorer__lead')).toBe(copy.opening.lead);
    const first = root.querySelectorAll('.cache-explorer__tick').length;
    expect(first).toBeGreaterThan(0);
    vi.advanceTimersByTime(16);
    expect(root.querySelectorAll('.cache-explorer__tick').length).toBeGreaterThanOrEqual(first);
    vi.advanceTimersByTime(5000);
    expect(q('.cache-explorer__plane--loading')).toBeNull();
    expect(q('.cache-explorer__plane--workbench')).not.toBeNull();
  });

  it('does not answer the workbench while a run is opening, and lets go of it when disposed', () => {
    vi.useFakeTimers();
    const { share } = run(900);
    open(fakeStore(), {
      open: { source: { kind: 'share', text: share }, since: performance.now() - 300 },
    });
    expect(q('.cache-explorer__plane--loading')).not.toBeNull();
    const ticks = root.querySelectorAll('.cache-explorer__tick').length;
    expect(() => {
      root.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
      root.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', bubbles: true }));
    }).not.toThrow();
    expect(root.querySelectorAll('.cache-explorer__tick')).toHaveLength(ticks);
    controller?.dispose();
    controller = null;
    expect(() => vi.advanceTimersByTime(5000)).not.toThrow();
    expect(q('.cache-explorer__plane--workbench')).toBeNull();
  });

  it('shows the malformed-link state for a link the core cannot read', () => {
    const store = fakeStore();
    open(store, {
      open: { source: { kind: 'share', text: '!!!not a link' }, since: performance.now() },
    });
    expect(text('.cache-explorer__head')).toContain('Could not open link');
    expect(text('.cache-explorer__lead')).toContain("This link doesn't describe");
    expect(text('.cache-explorer__problem')).toContain('base64url');
    const labels = [...root.querySelectorAll('[data-action^="load-"]')].map((e) => e.textContent);
    expect(labels).toEqual(['Start fresh']);
  });

  it('starts a module from its first step for #m=', async () => {
    const store = fakeStore();
    open(store, { module: '01-fixture-miss-then-hit' }, fixtureSource());
    await settle();
    expect(q('.cache-explorer__module-panel')?.textContent).not.toBe('');
    expect(text('.cache-explorer__status')).toContain('Step 1 of');
  });

  it('starts fresh from a failed link and drops it from the address', () => {
    const store = fakeStore();
    store.hash = '#s=!!!not a link';
    open(store, {
      open: { source: { kind: 'share', text: '!!!not a link' }, since: performance.now() },
    });
    click('load-fresh');
    expect(store.hash).toBeNull();
    expect(steps()).toBe(0);
  });
});

describe('a fault inside the core', () => {
  it('restores the run from the last good copy kept in memory', () => {
    const store = fakeStore();
    open(store);
    click('issue');
    click('issue');
    const head = text('.cache-explorer__head');
    vi.spyOn(lent, 'state').mockImplementation(() => {
      throw new WebAssembly.RuntimeError('unreachable');
    });
    click('issue');
    expect(status()).toBe(copy.recovered);
    expect(text('.cache-explorer__head')).toBe(head);
    // The page goes on with the rebuilt run.
    click('step-access');
    expect(steps()).toBeGreaterThan(Number(/(\d+) steps/.exec(head)?.[1]));
  });

  it('restores from the saved file when the run has outgrown a link', () => {
    const store = fakeStore();
    open(store);
    vi.spyOn(lent, 'toShareString').mockImplementation(() => {
      throw new Error(TOO_LONG);
    });
    click('issue');
    click('issue');
    const head = text('.cache-explorer__head');
    vi.spyOn(lent, 'state').mockImplementation(() => {
      throw new Error(
        'recursive use of an object detected which would lead to unsafe aliasing in rust',
      );
    });
    click('issue');
    expect(status()).toBe(copy.recovered);
    expect(text('.cache-explorer__head')).toBe(head);
  });
});
