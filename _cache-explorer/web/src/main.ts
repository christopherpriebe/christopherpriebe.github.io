import './styles/tokens.css';
import './styles/cache-explorer.css';
import { start, type Controller } from './controller';
import { h } from './dom';
import { fetchModules } from './modules';
import { measureLayout, watchLayout, type Layout } from './layout';
import { couldNotStartState, loadingState, narrowState, noWebAssemblyState } from './page-states';
import { init, Session } from './wasm';

/** How long the wasm may take before the loading state shows: most loads finish first. */
const LOADING_DELAY_MS = 200;

/** A URL beside this script, carrying the build's hash so a script, a stylesheet and a wasm from different builds never meet. */
function assetUrl(name: string): URL {
  const url = new URL(/* @vite-ignore */ name, import.meta.url);
  url.searchParams.set('v', __BUILD_HASH__);
  return url;
}

/** Adds the stylesheet and resolves when it has loaded, so the loading state is styled. A stylesheet that fails leaves the tool unstyled, which beats no tool. */
function linkStylesheet(href: URL): Promise<void> {
  return new Promise((resolve) => {
    const link = h('link', { rel: 'stylesheet', href: href.href });
    link.addEventListener('load', () => {
      resolve();
    });
    link.addEventListener('error', () => {
      resolve();
    });
    document.head.append(link);
  });
}

/** What the page shows until the workbench takes over: nothing, then one of the states. */
type Phase =
  | { name: 'starting' }
  | { name: 'loading' }
  | { name: 'unsupported' }
  | { name: 'failed' }
  | { name: 'running'; controller: Controller };

function reloadPage(): void {
  location.reload();
}

/** Exported for the tests; the page only ever runs it through the last two lines. */
export async function boot(root: HTMLElement, reload: () => void = reloadPage): Promise<void> {
  root.classList.add('cache-explorer');
  let phase: Phase = { name: 'starting' };
  let layout: Layout = measureLayout(root);

  /** Puts the state for the phase into the root; the workbench draws itself. */
  function paint(): void {
    switch (phase.name) {
      case 'starting':
        break;
      case 'loading':
        root.replaceChildren(layout === 'narrow' ? narrowState() : loadingState());
        break;
      case 'unsupported':
        root.replaceChildren(noWebAssemblyState());
        break;
      case 'failed':
        root.replaceChildren(couldNotStartState(reload));
        break;
      case 'running':
        phase.controller.setLayout(layout);
        break;
    }
  }

  watchLayout(root, (next) => {
    layout = next;
    paint();
  });

  // In production the build extracts the stylesheet and nothing links it; in development Vite injects it.
  if (import.meta.env.PROD) await linkStylesheet(assetUrl('cache-explorer.css'));
  if (typeof WebAssembly !== 'object') {
    phase = { name: 'unsupported' };
    paint();
    return;
  }

  const loading = setTimeout(() => {
    phase = { name: 'loading' };
    paint();
  }, LOADING_DELAY_MS);
  try {
    // The development server finds the wasm where the generated glue looks for it.
    await init(
      import.meta.env.PROD ? { module_or_path: assetUrl('cache_explorer_bg.wasm') } : undefined,
    );
    const controller = start(
      root,
      Session.firstVisit(),
      layout,
      root.dataset.modules === undefined ? null : fetchModules(root.dataset.modules),
    );
    clearTimeout(loading);
    phase = { name: 'running', controller };
  } catch (error) {
    clearTimeout(loading);
    console.error('The Interactive Cache Explorer could not start.', error);
    phase = { name: 'failed' };
    paint();
  }
}

const root = document.getElementById('cache-explorer');
if (root) await boot(root);
