import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { buildSnapshot } from '../src/controller';
import { h } from '../src/dom';
import { PLANE_CLASS } from '../src/focus';
import { render } from '../src/render';
import { initialUiState, SAMPLE_PROGRAM, type UiState } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let session: Session | null;
let ui: UiState;

/** The session the test owns; it is freed after the test. */
function current(): Session {
  if (!session) throw new Error('no session');
  return session;
}

/** One render of the session as it is now. */
function draw(): void {
  const { snapshot, log } = buildSnapshot(current(), ui);
  render(root, snapshot, log, ui);
}

/** Space with the drawer's program: the first step of a first visit. */
function step(): void {
  current().stepEvent({ source: SAMPLE_PROGRAM, format: 'asm' });
}

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

beforeAll(loadWasm);

beforeEach(() => {
  session = Session.firstVisit();
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  ui = initialUiState('desktop');
});

afterEach(() => {
  const owned = session;
  session = null;
  owned?.free();
});

describe('the workbench plane', () => {
  it('is the only child of the root: a section named for the tool that can take focus', () => {
    draw();
    expect(root.children).toHaveLength(1);
    const plane = query('section');
    expect(plane.classList.contains(PLANE_CLASS)).toBe(true);
    expect(plane.getAttribute('aria-label')).toBe('Interactive Cache Explorer');
    expect(plane.getAttribute('tabindex')).toBe('-1');
  });

  it('has the tool name at the left of the head and the core status at the right', () => {
    draw();
    const [name, status] = query('.cache-explorer__head').children;
    expect(name?.textContent).toBe('Interactive Cache Explorer');
    expect(status?.textContent).toBe('Basics · 0 steps in this run');
  });

  it('follows the session on the next render', () => {
    draw();
    step();
    draw();
    expect(query('.cache-explorer__head').textContent).toContain(
      current().state().labels.statusText,
    );
    expect(query('.cache-explorer__head').textContent).not.toContain('0 steps in this run');
  });

  it('keeps the same plane across renders, without rebuilding the root', () => {
    draw();
    const plane = query('section');
    const watch = new MutationObserver(() => undefined);
    watch.observe(root, { childList: true });
    step();
    draw();
    draw();
    expect(watch.takeRecords()).toEqual([]);
    watch.disconnect();
    expect(query('section')).toBe(plane);
  });
});

describe('the live region', () => {
  it('is polite and atomic, and inside the plane', () => {
    draw();
    const live = query('.cache-explorer__live');
    expect(live.getAttribute('aria-live')).toBe('polite');
    expect(live.getAttribute('aria-atomic')).toBe('true');
    expect(live.parentElement).toBe(query('section'));
  });

  it('is empty at the start of a run', () => {
    draw();
    expect(query('.cache-explorer__live').textContent).toBe('');
  });

  it('holds what the core says the event at the cursor reads', () => {
    step();
    draw();
    const said = current().timeline().liveText;
    expect(said).not.toBe('');
    expect(query('.cache-explorer__live').textContent).toBe(said);
  });

  it('follows the cursor back: undo empties it again', () => {
    step();
    draw();
    current().undo();
    draw();
    expect(query('.cache-explorer__live').textContent).toBe(current().timeline().liveText);
  });

  it('is the same element every render, and is written only when the text changes', () => {
    step();
    draw();
    const live = query('.cache-explorer__live');
    const watch = new MutationObserver(() => undefined);
    watch.observe(live, { childList: true, characterData: true, subtree: true });

    draw();
    draw();
    expect(watch.takeRecords()).toEqual([]);

    step();
    draw();
    expect(watch.takeRecords().length).toBeGreaterThan(0);
    watch.disconnect();
    expect(query('.cache-explorer__live')).toBe(live);
  });
});

describe('the narrow layout', () => {
  it('draws the notice in place of the workbench', () => {
    ui.layout = 'narrow';
    draw();
    expect(root.querySelector(`.${PLANE_CLASS}--workbench`)).toBeNull();
    expect(root.querySelector('.cache-explorer__live')).toBeNull();
    expect(query('.cache-explorer__lead').textContent).toBe(
      'The Interactive Cache Explorer needs a wider screen.',
    );
  });

  it('brings the same plane and live region back when the root widens', () => {
    draw();
    const plane = query('section');
    const live = query('.cache-explorer__live');

    ui.layout = 'narrow';
    draw();
    ui.layout = 'tablet';
    draw();

    expect(root.children).toHaveLength(1);
    expect(query('section')).toBe(plane);
    expect(query('.cache-explorer__live')).toBe(live);
  });
});
