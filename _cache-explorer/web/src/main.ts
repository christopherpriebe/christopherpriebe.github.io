import './styles/tokens.css';
import './styles/cache-explorer.css';
import { copy } from './copy';
import { h } from './dom';
import { buildInfo, init } from './wasm';

/** How long the wasm may take before the loading state shows: most loads finish first. */
const LOADING_DELAY_MS = 200;
const SKELETON_ROWS = 4;

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

/** The plane: its head with a status at the right, then the body. */
function plane(status: string, busy: boolean, ...body: Node[]): HTMLElement {
  return h(
    'section',
    { class: 'cache-explorer__plane', 'aria-label': copy.title, 'aria-busy': String(busy) },
    h(
      'div',
      { class: 'cache-explorer__head' },
      h('span', { class: 'cache-explorer__label' }, copy.title),
      h('span', { class: 'cache-explorer__label' }, status),
    ),
    ...body,
  );
}

/** Grey rows where the workbench will be; no spinner. */
function showLoading(root: HTMLElement): void {
  const rows = Array.from({ length: SKELETON_ROWS }, () => h('div'));
  root.replaceChildren(
    plane(
      copy.loading,
      true,
      h('div', { class: 'cache-explorer__skeleton', 'aria-hidden': 'true' }, ...rows),
    ),
  );
}

function showReady(root: HTMLElement): void {
  const info = buildInfo();
  root.replaceChildren(
    plane(
      copy.ready,
      false,
      h('p', { class: 'cache-explorer__note' }, `Core version ${info.version}.`),
    ),
  );
}

/** Exported for the tests; the page only ever runs it through the last two lines. */
export async function boot(root: HTMLElement): Promise<void> {
  root.classList.add('cache-explorer');
  // In production the build extracts the stylesheet and nothing links it; in development Vite injects it.
  if (import.meta.env.PROD) await linkStylesheet(assetUrl('cache-explorer.css'));
  const loading = setTimeout(() => {
    showLoading(root);
  }, LOADING_DELAY_MS);
  try {
    // The development server finds the wasm where the generated glue looks for it.
    await init(
      import.meta.env.PROD ? { module_or_path: assetUrl('cache_explorer_bg.wasm') } : undefined,
    );
  } finally {
    clearTimeout(loading);
  }
  showReady(root);
}

const root = document.getElementById('cache-explorer');
if (root) await boot(root);
