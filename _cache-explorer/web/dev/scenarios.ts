// Opens the tool in one of the states of sessions.ts, on the development page: the page picks one
// with `?scenario=<name>`, may move the cursor with `&pos=<n>`, and may type an address into the
// memory table's jump box with `&jump=<address>`. `&do=click:<selector>` and
// `&do=type:<selector>=<text>`, as many as wanted, then press a control and type into a field, in
// order, to reach a state of the editors. Nothing here is in the build.

import '../src/styles/tokens.css';
import '../src/styles/cache-explorer.css';
import { start } from '../src/controller';
import { measureLayout, watchLayout } from '../src/layout';
import { replayingState } from '../src/page-states';
import { init } from '../src/wasm';
import { fixtureSource } from './modules';
import { SCENARIOS } from './sessions';

const root = document.getElementById('cache-explorer');
if (!root) throw new Error('the page has no #cache-explorer');
const params = new URLSearchParams(location.search);
const name = params.get('scenario') ?? 'frame-3a';
const build = SCENARIOS[name];
if (!build) throw new Error(`no scenario ${name}: ${Object.keys(SCENARIOS).join(', ')}`);

root.classList.add('cache-explorer');
await init();
const session = build();
// `&state=loading` draws the state of a run being replayed, from this scenario's own summary.
if (params.get('state') === 'loading') {
  root.replaceChildren(replayingState(session.shareSummary(), 'share'));
  await new Promise<never>(() => undefined);
}
const pos = params.get('pos');
if (pos !== null) session.seek(Number(pos));
const controller = start(root, session, measureLayout(root), fixtureSource());
watchLayout(root, (layout) => {
  controller.setLayout(layout);
});

const jump = params.get('jump');
const box = root.querySelector<HTMLInputElement>('input[data-action="jump"]');
if (jump !== null && box) {
  box.value = jump;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  box.dispatchEvent(new Event('change', { bubbles: true }));
}

// The fixture modules arrive on a promise, and so does a module that is started: each step waits for
// what the one before it asked for.
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 50));
await settle();

for (const step of params.getAll('do')) {
  await settle();
  const verb = step.slice(0, step.indexOf(':'));
  const rest = step.slice(step.indexOf(':') + 1);
  if (verb === 'click') {
    root.querySelector<HTMLElement>(rest)?.click();
  } else if (verb === 'type') {
    const at = rest.lastIndexOf('=');
    const field = root.querySelector<HTMLInputElement>(rest.slice(0, at));
    if (field) {
      field.value = rest.slice(at + 1);
      field.dispatchEvent(new Event('input', { bubbles: true }));
      field.dispatchEvent(new Event('change', { bubbles: true }));
    }
  }
}
