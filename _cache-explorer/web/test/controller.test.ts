import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { buildSnapshot, start, type Controller } from '../src/controller';
import { h } from '../src/dom';
import { PLANE_CLASS } from '../src/focus';
import { initialUiState, SAMPLE_PROGRAM } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
/** The session the test owns, until it hands it to a controller. */
let session: Session | null;
/** The controller a test started; it is disposed after the test, which frees its session. */
let controller: Controller | null;

function current(): Session {
  if (!session) throw new Error('the session has been handed over');
  return session;
}

/** The session a controller owns, which a test may still use to move the run, but never free. */
let lent: Session;

/** Gives the session to a controller, as the page does, and keeps the controller for the test. */
function startWith(layout: 'desktop' | 'tablet' | 'narrow'): Controller {
  const owned = current();
  lent = owned;
  session = null;
  controller = start(root, owned, layout);
  return controller;
}

const program = { source: SAMPLE_PROGRAM, format: 'asm' } as const;

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  session = Session.firstVisit();
  controller = null;
});

afterEach(() => {
  controller?.dispose();
  const owned = session;
  session = null;
  owned?.free();
});

describe('buildSnapshot', () => {
  it('asks the session for everything a render draws, at a first visit', () => {
    const { snapshot, log } = buildSnapshot(current(), initialUiState('desktop'));
    expect(snapshot.state.config.addressBits).toBe(12);
    expect(snapshot.timeline).toMatchObject({ stepCount: 0, headerText: 'no steps yet' });
    expect(snapshot.ticks).toHaveLength(0);
    expect(snapshot.memoryRows).toEqual([]);
    expect(snapshot.config.summary).toBe(snapshot.state.labels.configSummary);
    expect(log).toEqual([]);
  });

  it('reads the typed address, decoded, with the comparator waiting on its set', () => {
    const { snapshot } = buildSnapshot(current(), initialUiState('desktop'));
    expect(snapshot.draft.address).toMatchObject({
      status: 'valid',
      decoded: { addrText: '0x0A4' },
    });
    expect(snapshot.draft.selectedSet).toBe(2);
    expect(snapshot.draft.compare).toEqual(['pending']);
    expect(snapshot.draft.requestText).toBe('Load 0x0A4');
  });

  it('reads what is typed now, not what was typed before', () => {
    const ui = initialUiState('desktop');
    ui.draft.addrText = '0x0';
    expect(buildSnapshot(current(), ui).snapshot.draft.requestText).toBe('Load 0x000');
    ui.draft.addrText = 'xyz';
    const { draft } = buildSnapshot(current(), ui).snapshot;
    expect(draft.address.status).toBe('invalid');
    expect(draft.requestText).toBe('No request');
  });

  it("parses the drawer's sample program into the first visit's summary", () => {
    const { snapshot } = buildSnapshot(current(), initialUiState('desktop'));
    expect(snapshot.programDraft).toEqual({
      status: 'ok',
      instructions: 5,
      summary: '5 instructions · next: ld r1, [0x0A4]',
    });
  });

  it('parses nothing while the drawer is empty', () => {
    const ui = initialUiState('desktop');
    ui.program.text = '';
    expect('programDraft' in buildSnapshot(current(), ui).snapshot).toBe(false);
  });

  it('shows the pending configuration in the controls, and the run in the state', () => {
    const ui = initialUiState('desktop');
    const { config } = current().state();
    ui.pending = { ...config, cache: { ...config.cache, associativity: 2 } };
    const { snapshot } = buildSnapshot(current(), ui);
    expect(snapshot.config.summary).toContain('2-way');
    expect(snapshot.state.config.cache.associativity).toBe(1);
  });

  it('draws the memory editor the student opened, and no other', () => {
    const ui = initialUiState('desktop');
    expect(buildSnapshot(current(), ui).snapshot.editor).toBeUndefined();
    ui.editor = { kind: 'memory', addr: 0xa4, addrText: '0x0A4', valueText: '9' };
    expect(buildSnapshot(current(), ui).snapshot.editor).toBeDefined();
  });

  it('windows the log from where the student has scrolled', () => {
    const ui = initialUiState('desktop');
    for (let i = 0; i < 3; i += 1) current().stepAccess(program);
    const { entryCount } = current().timeline();
    expect(entryCount).toBeGreaterThan(3);
    expect(buildSnapshot(current(), ui).log).toHaveLength(entryCount);
    ui.logFirst = 2;
    expect(buildSnapshot(current(), ui).log).toHaveLength(entryCount - 2);
  });

  it('gives the ticks of the run as it is now, with no copy kept from before', () => {
    const ui = initialUiState('desktop');
    current().stepAccess(program);
    current().stepAccess(program);
    const first = buildSnapshot(current(), ui).snapshot.ticks.slice();
    expect(first).toEqual(current().ticks());

    // Undo the second access and issue another: the ticks are those of the run as it now is.
    current().undo();
    current().applyOp({ op: 'issue', kind: 'read', addr: 0x1a4 });
    const second = buildSnapshot(current(), ui).snapshot.ticks;
    expect(second).not.toEqual(first);
    expect(second).toEqual(current().ticks());
  });
});

describe('start', () => {
  it('draws the first visit into the root', () => {
    startWith('desktop');
    expect(root.querySelector('section')?.getAttribute('aria-label')).toBe(
      'Interactive Cache Explorer',
    );
    expect(root.textContent).toContain('Basics · 0 steps in this run');
  });

  it('draws again from the session when asked', () => {
    const started = startWith('desktop');
    lent.stepEvent(program);
    started.refresh();
    expect(root.textContent).toContain(lent.state().labels.statusText);
    expect(root.textContent).not.toContain('0 steps in this run');
  });

  it('draws the notice in a narrow root and the workbench again when it widens', () => {
    const started = startWith('narrow');
    expect(root.querySelector('.cache-explorer__lead')?.textContent).toContain('wider screen');
    started.setLayout('tablet');
    expect(root.querySelector(`.${PLANE_CLASS}`)?.getAttribute('aria-label')).toBe(
      'Interactive Cache Explorer',
    );
    expect(root.querySelector('.cache-explorer__lead')).toBeNull();
    started.setLayout('narrow');
    expect(root.querySelector('.cache-explorer__lead')).not.toBeNull();
  });

  it('does not draw again for a layout it is already in', () => {
    const started = startWith('desktop');
    const watch = new MutationObserver(() => undefined);
    watch.observe(root, { subtree: true, childList: true, characterData: true });
    started.setLayout('desktop');
    expect(watch.takeRecords()).toEqual([]);
    watch.disconnect();
  });

  it('gives focus to the plane when the element that had it is rebuilt away', () => {
    const started = startWith('desktop');
    const button = h('button', { 'data-action': 'gone' }, 'in the head');
    root.querySelector('.cache-explorer__head')?.append(button);
    button.focus();
    started.refresh();
    expect(document.activeElement).toBe(root.querySelector(`.${PLANE_CLASS}`));
  });

  it('leaves focus outside the tool where it was', () => {
    const outside = h('button');
    document.body.append(outside);
    const started = startWith('desktop');
    outside.focus();
    started.refresh();
    expect(document.activeElement).toBe(outside);
  });
});

describe('the action table', () => {
  it('finds nothing for a name it does not hold, whatever names an object inherits', () => {
    startWith('desktop');
    const failures: Event[] = [];
    const record = (event: Event): void => {
      event.preventDefault();
      failures.push(event);
    };
    window.addEventListener('error', record);
    for (const name of ['__proto__', 'constructor', 'toString', 'hasOwnProperty', 'valueOf']) {
      const element = h('button', { 'data-action': name });
      root.append(element);
      element.click();
    }
    window.removeEventListener('error', record);
    expect(failures).toEqual([]);
  });
});

describe('ownership', () => {
  it('frees the session once, when disposed, however often that is asked', () => {
    const free = vi.spyOn(current(), 'free');
    const started = startWith('desktop');
    expect(free).not.toHaveBeenCalled();
    started.dispose();
    started.dispose();
    expect(free).toHaveBeenCalledOnce();
  });

  it('refuses to draw once disposed', () => {
    const started = startWith('desktop');
    started.dispose();
    expect(() => {
      started.refresh();
    }).toThrow('disposed');
  });

  it('frees the session, and throws the error, when the first draw fails', () => {
    const owned = current();
    const free = vi.spyOn(owned, 'free');
    vi.spyOn(owned, 'state').mockImplementation(() => {
      throw new Error('the core said no');
    });
    expect(() => startWith('desktop')).toThrow('the core said no');
    expect(free).toHaveBeenCalledOnce();
  });
});
