import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { FIXTURE_INDEX, FIXTURE_MODULES, shippedSource } from '../dev/modules';
import type { Context } from '../src/context';
import { start, type Controller } from '../src/controller';
import { h } from '../src/dom';
import { endModule, releaseModule } from '../src/module-actions';
import { fetchModules, type ModuleSource } from '../src/modules';
import { renderModulePanel } from '../src/render/module-panel';
import { initialUiState } from '../src/ui-state';
import { ModuleRun, parseModuleIndex, Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

const MODULES = FIXTURE_MODULES;
const INDEX = FIXTURE_INDEX;

function source(overrides: Partial<ModuleSource> = {}): ModuleSource {
  return {
    index: () => Promise.resolve(INDEX),
    module: (id) => {
      const text = MODULES.get(id);
      return text === undefined ? Promise.reject(new Error(`no ${id}`)) : Promise.resolve(text);
    },
    ...overrides,
  };
}

let root: HTMLElement;
let controller: Controller | null;
/** The session the controller owns, which a test may still move, but never free. */
let lent: Session;

/** Lets the fetches the controller waits for, and the draws they ask for, finish. */
async function settle(): Promise<void> {
  for (let i = 0; i < 4; i += 1) await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function open(modules: ModuleSource | null = source()): Promise<void> {
  lent = Session.firstVisit();
  controller = start(root, lent, 'desktop', modules);
  await settle();
}

const q = (selector: string): HTMLElement | null => root.querySelector<HTMLElement>(selector);
const action = (name: string): HTMLButtonElement | null =>
  root.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);

function click(name: string, extra = ''): void {
  const button = root.querySelector<HTMLElement>(`[data-action="${name}"]${extra}`);
  if (!button) throw new Error(`no ${name}`);
  button.click();
}

/** Clicks Start on a module's row and waits for it to begin. */
async function begin(id: string): Promise<void> {
  click('modules-toggle');
  click('module-start', `[data-module="${id}"]`);
  await settle();
}

/** An option of the open prediction, not an option of the configuration bar. */
const option = (index: number): HTMLElement | null =>
  q(`.cache-explorer__module-option[data-option="${String(index)}"]`);

const text = (selector: string): string => q(selector)?.textContent ?? '';

beforeAll(loadWasm);

/** What the browser has and the test document does not: the scroll to the panel, and the motion query. */
const scrolled = vi.fn<(options?: ScrollIntoViewOptions) => void>();
let reduced = false;

beforeAll(() => {
  Element.prototype.scrollIntoView = scrolled;
  window.matchMedia = ((query: string) => ({
    matches: reduced,
    media: query,
  })) as typeof window.matchMedia;
});

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  controller = null;
  reduced = false;
  scrolled.mockClear();
});

afterEach(() => {
  controller?.dispose();
});

describe('fetchModules', () => {
  it('finds the index where the page says, and each module beside it', async () => {
    const fetcher = vi.fn<typeof fetch>((url) =>
      Promise.resolve(
        new Response(url instanceof Request ? url.url : String(url), { status: 200 }),
      ),
    );
    const modules = fetchModules('/sub/modules/index.json', fetcher, 'https://example.test/page/');
    expect(await modules.index()).toBe('https://example.test/sub/modules/index.json');
    expect(await modules.module('04-lru')).toBe('https://example.test/sub/modules/04-lru.json');
  });

  it('resolves a relative index against the page', async () => {
    const fetcher = vi.fn<typeof fetch>((url) =>
      Promise.resolve(
        new Response(url instanceof Request ? url.url : String(url), { status: 200 }),
      ),
    );
    const modules = fetchModules('modules/index.json', fetcher, 'https://example.test/a/b/');
    expect(await modules.module('01-x')).toBe('https://example.test/a/b/modules/01-x.json');
  });

  it('rejects a file the server does not have', async () => {
    const fetcher = vi.fn<typeof fetch>(() => Promise.resolve(new Response('', { status: 404 })));
    await expect(
      fetchModules('/m/index.json', fetcher, 'https://example.test/').index(),
    ).rejects.toThrow('404');
  });
});

describe('the Guided modules drawer', () => {
  it('draws nothing on a page that offers no modules', async () => {
    await open(null);
    expect(q('.cache-explorer__modules-head')?.childElementCount).toBe(0);
  });

  it('puts the drawer first inside the plane, above the configuration bar, closed', async () => {
    await open();
    const plane = q('.cache-explorer__plane');
    const order = [...(plane?.children ?? [])].map((child) => child.className);
    expect(order.findIndex((name) => name.includes('cache-explorer__modules'))).toBe(1);
    expect(order[2]).toContain('cache-explorer__config');
    expect(q('.cache-explorer__module-list')).toBeNull();
  });

  it("shows the core's summary and, opened, a row for each module", async () => {
    await open();
    expect(text('.cache-explorer__modules-head')).toContain('Guided modules');
    expect(text('.cache-explorer__modules-head')).toContain('3 modules · none running');
    expect(q('.cache-explorer__module-list')).toBeNull();
    click('modules-toggle');
    const rows = root.querySelectorAll('.cache-explorer__module-row');
    expect(rows).toHaveLength(3);
    expect(rows[0]?.textContent).toContain('01');
    expect(rows[0]?.textContent).toContain('Fixture: miss then hit');
    expect(rows[0]?.querySelector('code')?.textContent).toBe('0x0A8');
    expect(rows[0]?.querySelector('em')?.textContent).toBe('misses');
  });

  it('marks a module that needs Advanced with a word, not a colour', async () => {
    await open();
    click('modules-toggle');
    const badges = [...root.querySelectorAll('.cache-explorer__module-badge')].map(
      (badge) => badge.textContent,
    );
    expect(badges).toEqual(['Advanced']);
  });

  it('says the list could not be had, and tries again', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    let works = false;
    await open(
      source({ index: () => (works ? Promise.resolve(INDEX) : Promise.reject(new Error('down'))) }),
    );
    expect(error).toHaveBeenCalled();
    expect(text('.cache-explorer__modules-head')).toContain('could not be loaded');
    click('modules-toggle');
    expect(text('.cache-explorer__modules-body')).toContain('could not be loaded');
    works = true;
    click('modules-retry');
    await settle();
    expect(text('.cache-explorer__modules-head')).toContain('3 modules · none running');
    expect(q('.cache-explorer__module-list')).not.toBeNull();
  });

  it('shows a list the core refuses as one that could not be had', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await open(source({ index: () => Promise.resolve('[{"id":"x"}]') }));
    expect(text('.cache-explorer__modules-head')).toContain('could not be loaded');
  });
});

describe('starting a module', () => {
  it('applies the preset and shows the first step on the panel', async () => {
    await open();
    expect(q('.cache-explorer__module-panel')?.childElementCount).toBe(0);
    await begin('01-fixture-miss-then-hit');
    const panel = q('.cache-explorer__module-panel');
    expect(panel?.getAttribute('aria-label')).toBe('Guided module');
    expect(text('.cache-explorer__module-meta')).toContain('Module · Fixture: miss then hit');
    expect(text('.cache-explorer__module-meta')).toContain('Step 1 of 6 · Explain');
    expect(text('.cache-explorer__module-text')).toContain('The low 4 bits of an address');
    expect(root.querySelectorAll('.cache-explorer__module-dot')).toHaveLength(6);
    expect(root.querySelectorAll('.cache-explorer__module-dot--here')).toHaveLength(1);
    expect(q('.cache-explorer__module-dots')?.getAttribute('aria-hidden')).toBe('true');
    expect(lent.state().config.cache.associativity).toBe(1);
  });

  it("puts the run on the drawer's line and on its row", async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    expect(text('.cache-explorer__modules-head')).toContain('Running 01');
    const row = q('.cache-explorer__module-row--running');
    expect(row?.textContent).toContain('Running');
    expect(row?.querySelector('button')?.textContent).toContain('Restart');
  });

  it("puts the module's program in the drawer", async () => {
    await open();
    await begin('02-fixture-switch-config');
    expect(lent.state().program?.source).toBe('R 0x000\nR 0x080\nR 0x000\nR 0x080');
    expect(text('.cache-explorer__program-body')).toContain('R 0x080');
  });

  it('starts another module in place of the one running', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    click('module-start', '[data-module="02-fixture-switch-config"]');
    await settle();
    expect(text('.cache-explorer__module-meta')).toContain('Fixture: conflicts, then two ways');
    expect(text('.cache-explorer__modules-head')).toContain('Running 02');
  });

  it('brings the panel into view and says which module began', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    expect(scrolled).toHaveBeenCalledWith({ behavior: 'smooth', block: 'start' });
    expect(text('.cache-explorer__status')).toContain('Started the module Fixture: miss then hit');
    expect(text('.cache-explorer__status')).toContain('Step 1 of 6');
  });

  it('scrolls without a glide when the student asked for less motion', async () => {
    reduced = true;
    await open();
    await begin('01-fixture-miss-then-hit');
    expect(scrolled).toHaveBeenCalledWith({ behavior: 'auto', block: 'start' });
  });

  it('keeps the live regions across renders, so a repeated message is not announced again', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await open(source({ module: () => Promise.reject(new Error('offline')) }));
    click('modules-toggle');
    click('module-start', '[data-module="01-fixture-miss-then-hit"]');
    await settle();
    const alert = q('.cache-explorer__modules [role="alert"]');
    expect(alert?.textContent).toBe('The module could not be loaded.');
    click('modules-toggle');
    click('modules-toggle');
    expect(q('.cache-explorer__modules [role="alert"]')).toBe(alert);
  });

  it('says why a module that cannot be fetched did not start', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await open(source({ module: () => Promise.reject(new Error('offline')) }));
    click('modules-toggle');
    click('module-start', '[data-module="01-fixture-miss-then-hit"]');
    await settle();
    expect(text('.cache-explorer__modules [role="alert"]')).toBe('The module could not be loaded.');
    expect(q('.cache-explorer__module-panel')?.childElementCount).toBe(0);
    expect(action('module-start')?.disabled).toBe(false);
  });

  it('says what the core found wrong in a module it cannot read', async () => {
    await open(source({ module: () => Promise.resolve('{"id": 1}') }));
    click('modules-toggle');
    click('module-start', '[data-module="01-fixture-miss-then-hit"]');
    await settle();
    expect(text('.cache-explorer__modules [role="alert"]')).not.toBe('');
    expect(q('.cache-explorer__module-panel')?.childElementCount).toBe(0);
  });

  it('leaves the run as it was when the session refuses the module', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    const before = lent.opCount();
    vi.spyOn(ModuleRun.prototype, 'start').mockImplementationOnce(() => {
      throw new Error('The session is full.');
    });
    click('module-start', '[data-module="02-fixture-switch-config"]');
    await settle();
    expect(text('.cache-explorer__modules [role="alert"]')).toBe('The session is full.');
    expect(text('.cache-explorer__module-meta')).toContain('Fixture: miss then hit');
    expect(lent.opCount()).toBe(before);
  });

  it('does not start a second module while one is being fetched', async () => {
    let release: (text: string) => void = () => undefined;
    const fetched = vi.fn<(id: string) => Promise<string>>(
      () =>
        new Promise<string>((resolve) => {
          release = resolve;
        }),
    );
    await open(source({ module: fetched }));
    click('modules-toggle');
    click('module-start', '[data-module="01-fixture-miss-then-hit"]');
    expect(action('module-start')?.disabled).toBe(true);
    expect(text('.cache-explorer__module-row')).toContain('Starting…');
    click('module-start', '[data-module="02-fixture-switch-config"]');
    expect(fetched).toHaveBeenCalledTimes(1);
    release(MODULES.get('01-fixture-miss-then-hit') ?? '');
    await settle();
    expect(text('.cache-explorer__module-meta')).toContain('Fixture: miss then hit');
  });

  it('reads nothing of a module whose fetch finishes after the page is gone', async () => {
    let release: (text: string) => void = () => undefined;
    await open(
      source({
        module: () =>
          new Promise<string>((resolve) => {
            release = resolve;
          }),
      }),
    );
    click('modules-toggle');
    click('module-start', '[data-module="01-fixture-miss-then-hit"]');
    const read = vi.spyOn(ModuleRun, 'fromJson');
    controller?.dispose();
    controller = null;
    release(MODULES.get('01-fixture-miss-then-hit') ?? '');
    await settle();
    expect(read).not.toHaveBeenCalled();
  });
});

describe('the steps', () => {
  /** Opens module 01 and takes it to its prediction with the buttons a student has. */
  async function atPrediction(): Promise<void> {
    await open();
    await begin('01-fixture-miss-then-hit');
    click('module-next');
    click('module-show-me');
    click('module-show-me');
  }

  it('goes on from an explanation, and completes an instruction the student does or is shown', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    expect(action('module-next')?.classList.contains('cache-explorer__button--primary')).toBe(true);
    expect(action('module-show-me')).toBeNull();
    click('module-next');
    expect(text('.cache-explorer__module-meta')).toContain('Step 2 of 6 · Do this');
    expect(text('.cache-explorer__module-hint')).toContain('Type 0x0A4 in the address box');
    expect(root.querySelector('.cache-explorer__module-hint code')).not.toBeNull();
    const doIt = q('input[data-action="addr"]');
    if (!(doIt instanceof HTMLInputElement)) throw new Error('no address box');
    click('issue');
    expect(text('.cache-explorer__module-meta')).toContain('Step 3 of 6 · Watch');
    expect(lent.state().position.pos).toBeGreaterThan(0);
  });

  it('moves the cursor for "Show me" on a step to watch', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    click('module-next');
    click('module-show-me');
    expect(text('.cache-explorer__module-meta')).toContain('Watch');
    const at = lent.state().position.pos;
    click('module-show-me');
    expect(text('.cache-explorer__module-meta')).toContain('Predict');
    expect(lent.state().position.pos).not.toBe(at);
  });

  it('keeps Check off until an option is chosen, and shows no "Show me" before a wrong answer', async () => {
    await atPrediction();
    expect(text('.cache-explorer__module-meta')).toContain('Step 4 of 6 · Predict');
    expect(action('module-check')?.disabled).toBe(true);
    expect(action('module-show-me')).toBeNull();
    expect(root.querySelectorAll('.cache-explorer__module-option')).toHaveLength(3);
    click('module-select', '[data-option="1"]');
    expect(action('module-check')?.disabled).toBe(false);
    expect(option(1)?.getAttribute('aria-pressed')).toBe('true');
    expect(option(1)?.textContent).toContain('(selected)');
  });

  it('keeps the panel status element while the feedback on it stays the same', async () => {
    await atPrediction();
    const live = q('.cache-explorer__module-panel + [role="status"]');
    click('module-select', '[data-option="1"]');
    click('module-check');
    expect(live?.textContent).toContain('Not quite.');
    const seen = vi.fn();
    const watch = new MutationObserver(seen);
    watch.observe(live as Node, { childList: true, characterData: true, subtree: true });
    click('modules-toggle');
    await settle();
    watch.disconnect();
    expect(q('.cache-explorer__module-panel + [role="status"]')).toBe(live);
    expect(seen).not.toHaveBeenCalled();
  });

  it('locks what the core locks while the prediction is open', async () => {
    await atPrediction();
    const enabled = lent.state().controls.enabled;
    expect(enabled).not.toContain('issue');
    expect(action('issue')?.disabled).toBe(true);
    expect(action('step')?.disabled).toBe(true);
    const guard = lent.state().controls.guard;
    expect(guard).toBe('Answer the prediction in the module panel first.');
  });

  it('disables Load under an unanswered prediction, and a chosen saved run does not replace the run', async () => {
    await atPrediction();
    const guard = lent.state().controls.guard;
    expect(lent.state().controls.enabled).not.toContain('load');
    expect(action('load')?.disabled).toBe(true);
    expect(action('load')?.title).toBe(guard);
    // The page can change while the picker is open: a file chosen now is refused as well.
    const saved = Session.firstVisit();
    const json = saved.toJson();
    saved.free();
    const file = new File([json], 'a.cache-explorer.json');
    const pick = vi.spyOn(HTMLInputElement.prototype, 'click').mockImplementation(function (
      this: HTMLInputElement,
    ) {
      Object.defineProperty(this, 'files', { value: [file], configurable: true });
      this.dispatchEvent(new Event('change'));
    });
    action('load')?.removeAttribute('disabled');
    click('load');
    await settle();
    pick.mockRestore();
    expect(root.querySelector('.cache-explorer__module-panel')).not.toBeNull();
    expect(text('.cache-explorer__status')).toBe(guard);
  });

  it('marks a wrong answer in words and offers "Show me", then takes the right one', async () => {
    await atPrediction();
    click('module-select', '[data-option="1"]');
    click('module-check');
    const note = q('.cache-explorer__module-note--incorrect');
    expect(note?.hasAttribute('role')).toBe(false);
    expect(note?.textContent).toContain('Not quite.');
    expect(text('.cache-explorer__module-panel + [role="status"]')).toContain('Not quite.');
    expect(option(1)?.textContent).toContain('(not the answer)');
    expect(action('module-show-me')).not.toBeNull();
    expect(action('issue')?.disabled).toBe(true);

    click('module-select', '[data-option="0"]');
    expect(q('.cache-explorer__module-panel .cache-explorer__module-note')).toBeNull();
    click('module-check');
    expect(q('.cache-explorer__module-note--correct')?.textContent).toContain('Correct.');
    expect(option(0)?.textContent).toContain('(the answer)');
    expect(action('module-next')).not.toBeNull();
    expect(action('module-check')).toBeNull();
    click('module-select', '[data-option="2"]');
    expect(option(2)?.classList.contains('cache-explorer__module-option--idle')).toBe(true);
    expect(option(0)?.classList.contains('cache-explorer__module-option--correct')).toBe(true);
    expect(action('issue')?.disabled).toBe(false);
  });

  it('shows the answer after a wrong one when asked', async () => {
    await atPrediction();
    click('module-select', '[data-option="2"]');
    click('module-check');
    click('module-show-me');
    expect(q('.cache-explorer__module-note--shown')?.textContent).toContain('Shown.');
    expect(option(0)?.classList.contains('cache-explorer__module-option--shown')).toBe(true);
    expect(option(2)?.classList.contains('cache-explorer__module-option--wrong')).toBe(false);
    // The answer shown was never the student's choice: only the choice is pressed.
    expect(option(0)?.getAttribute('aria-pressed')).toBe('false');
    expect(option(2)?.getAttribute('aria-pressed')).toBe('true');
    expect(q('.cache-explorer__module-dots')?.getAttribute('aria-hidden')).toBe('true');
    expect(action('module-next')).not.toBeNull();
  });

  it("draws the core's notice of a refused move as a bar, and a disabled button disabled", () => {
    const session = Session.firstVisit();
    const run = ModuleRun.fromJson(MODULES.get('01-fixture-miss-then-hit') ?? '');
    try {
      const view = run.view(session);
      const panel = h('section');
      const live = h('div');
      renderModulePanel(
        panel,
        {
          ...view,
          notice: 'The run is at its limit.',
          actions: [{ action: 'next', enabled: false }],
        },
        live,
      );
      expect(live.textContent).toBe('The run is at its limit.');
      const note = panel.querySelector('.cache-explorer__module-note--refused');
      expect(note?.textContent).toBe('The run is at its limit.');
      expect(panel.querySelector<HTMLButtonElement>('[data-action="module-next"]')?.disabled).toBe(
        true,
      );
      renderModulePanel(panel, undefined, live);
      expect(panel.childElementCount).toBe(0);
    } finally {
      run.free();
      session.free();
    }
  });

  it('ends on the done screen with a Restart button, and starts again', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    click('module-next');
    click('module-show-me');
    click('module-show-me');
    click('module-select', '[data-option="0"]');
    click('module-check');
    click('module-next');
    click('module-show-me');
    click('module-next');
    expect(text('.cache-explorer__module-meta')).toContain('6 of 6 · Done');
    expect(text('.cache-explorer__module-text')).toContain('Module complete.');
    expect(root.querySelectorAll('.cache-explorer__module-dot--done')).toHaveLength(6);
    expect(action('module-restart')?.textContent).toBe('Restart');
    click('module-restart');
    expect(text('.cache-explorer__module-meta')).toContain('Step 1 of 6');
  });

  it('leaves with Exit: the panel goes, the gate lifts, the workbench stays', async () => {
    await atPrediction();
    const pos = lent.state().position.pos;
    click('module-exit');
    expect(q('.cache-explorer__module-panel')?.childElementCount).toBe(0);
    expect(text('.cache-explorer__modules-head')).toContain('3 modules · none running');
    expect(lent.state().controls.enabled).toContain('issue');
    expect(lent.state().position.pos).toBe(pos);
    expect(root.querySelector('.cache-explorer__spot')).toBeNull();
  });
});

describe('the outline of a step', () => {
  const spots = (): HTMLElement[] => [
    ...root.querySelectorAll<HTMLElement>('.cache-explorer__spot'),
  ];

  it('outlines the region the step names, and only that one', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    expect(spots()).toHaveLength(1);
    expect(spots()[0]?.classList.contains('cache-explorer__strip')).toBe(true);
    click('module-next');
    expect(spots()).toHaveLength(1);
    expect(spots()[0]).toBe(q('.cache-explorer__transport'));
    click('module-show-me');
    expect(spots()[0]).toBe(q('.cache-explorer__log'));
  });

  it('outlines a way of the cache, outside the cell, for a callout with a way', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    click('module-next');
    click('module-show-me');
    click('module-show-me');
    click('module-select', '[data-option="0"]');
    click('module-check');
    click('module-next');
    click('module-show-me');
    const [spot] = spots();
    expect(spots()).toHaveLength(1);
    expect(spot?.dataset.action).toBe('edit-way');
    expect(spot?.dataset.set).toBe('2');
    expect(spot?.dataset.way).toBe('0');
    expect(spot?.classList.contains('cache-explorer__spot--cell')).toBe(true);
  });

  it('keeps the outline across a redraw that changes nothing', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    controller?.refresh();
    controller?.refresh();
    expect(spots()).toHaveLength(1);
  });
});

describe('a module and the session', () => {
  let ui = initialUiState('desktop');
  let session: Session;

  beforeEach(() => {
    ui = initialUiState('desktop');
    session = Session.firstVisit();
  });

  afterEach(() => {
    releaseModule(ui);
    session.free();
  });

  it('frees the run the state held, once, and lets go of it first', () => {
    const run = ModuleRun.fromJson(MODULES.get('01-fixture-miss-then-hit') ?? '');
    ui.modules.run = run;
    releaseModule(ui);
    expect(ui.modules.run).toBeNull();
    expect(() => run.view(session)).toThrow();
    releaseModule(ui);
  });

  it('ends the module when another run is opened, and says so', () => {
    const run = ModuleRun.fromJson(MODULES.get('01-fixture-miss-then-hit') ?? '');
    ui.modules.run = run;
    const context = { ui } as Context;
    endModule(context);
    expect(ui.modules.run).toBeNull();
    expect(ui.status).toBe('The module ended because another run was opened.');
    expect(() => run.view(session)).toThrow();
  });

  it('says nothing when no module was running', () => {
    endModule({ ui } as Context);
    expect(ui.status).toBe('');
  });
});

describe('the controller', () => {
  it('lets go of the run when it is disposed', async () => {
    await open();
    await begin('01-fixture-miss-then-hit');
    const free = vi.spyOn(ModuleRun.prototype, 'free');
    controller?.dispose();
    controller = null;
    expect(free).toHaveBeenCalledTimes(1);
  });
});

describe('the shipped modules', () => {
  it('are all listed in order, with the Advanced badge on the last three', async () => {
    const index = parseModuleIndex(await shippedSource().index());
    expect(index.modules).toHaveLength(9);
    expect(index.summary).toBe('9 modules · none running');
    await open(shippedSource());
    click('modules-toggle');
    const rows = [...root.querySelectorAll('.cache-explorer__module-row')];
    expect(rows).toHaveLength(9);
    expect(rows.map((r) => r.querySelector('.cache-explorer__module-name')?.textContent)).toEqual(
      index.modules.map((card) => card.title),
    );
    const badges = rows.map(
      (r) => r.querySelector('.cache-explorer__module-badge')?.textContent ?? 'none',
    );
    expect(badges).toEqual([
      ...Array<string>(6).fill('none'),
      ...Array<string>(3).fill('Advanced'),
    ]);
    expect(index.modules.map((card) => card.advanced)).toEqual(badges.map((b) => b === 'Advanced'));
  });

  it('each load and start through the controller', async () => {
    const index = parseModuleIndex(await shippedSource().index());
    for (const card of index.modules) {
      await open(shippedSource());
      await begin(card.id);
      expect(text('.cache-explorer__module-meta')).toContain(card.title);
      expect(text('.cache-explorer__module-meta')).toContain('Step 1 of');
      controller?.dispose();
      controller = null;
    }
  });

  it('each open as a run', async () => {
    const shipped = shippedSource();
    for (const card of parseModuleIndex(await shipped.index()).modules) {
      const run = ModuleRun.fromJson(await shipped.module(card.id));
      expect(run).toBeInstanceOf(ModuleRun);
      run.free();
    }
  });
});
