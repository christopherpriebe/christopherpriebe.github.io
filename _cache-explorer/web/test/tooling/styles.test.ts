// @vitest-environment node
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const read = (name: string): string =>
  readFileSync(fileURLToPath(new URL(`../../src/styles/${name}`, import.meta.url)), 'utf8');
const tokensCss = read('tokens.css');
const componentsCss = read('cache-explorer.css');

/** The custom properties a stylesheet declares, by name without the `--cache-explorer-` prefix. */
function declared(css: string): Record<string, string> {
  const found: Record<string, string> = {};
  for (const [, name, value] of css.matchAll(/--cache-explorer-([\w-]+):\s*([^;]+);/g)) {
    if (name === undefined || value === undefined) continue;
    found[name] = value.replace(/\s+/g, ' ').replace(/\( /g, '(').replace(/ \)/g, ')').trim();
  }
  return found;
}

const MONO = "var(--site-font-mono, 'IBM Plex Mono', ui-monospace, Menlo, monospace)";

/** The token table: each token, and what it is in terms of the site's property and the design's value. */
const TOKENS: Record<string, string> = {
  plane: 'var(--site-concrete, #f1f1ef)',
  surface: 'var(--site-paper, #fbfbfa)',
  empty: 'var(--site-concrete-deep, #e4e5e2)',
  rule: 'var(--site-rule, #c8cac6)',
  text: 'var(--site-slate, #2c4467)',
  strong: 'var(--site-navy, #16243a)',
  ink: 'var(--site-ink, #0b1420)',
  tag: 'var(--site-ramp-3, #335d9b)',
  index: 'var(--site-ramp-2, #5d87c5)',
  offset: 'var(--site-ramp-1, #8eadda)',
  hit: 'var(--site-navy, #16243a)',
  miss: 'var(--site-navy, #16243a)',
  evict: 'var(--site-signal-red, #af3c3a)',
  fill: 'var(--site-navy, #16243a)',
  writeback: 'var(--site-signal-teal, #007e81)',
  writethrough: 'var(--site-signal-blue, #3568b2)',
  dirty: 'var(--site-signal-amber, #a07100)',
  stale: 'var(--cache-explorer-dirty)',
  hatch:
    'repeating-linear-gradient(45deg, var(--cache-explorer-dirty) 0 3px, var(--cache-explorer-plane) 3px 5px)',
  changed: '#f2e2a0',
  'changed-ring': 'var(--site-ink, #0b1420)',
  correct: 'var(--site-signal-green, #247c3a)',
  incorrect: 'var(--site-signal-red, #af3c3a)',
  'shadow-pop': '0 9px 27px rgb(11 20 32 / 0.16)',
  'font-title':
    "500 24px/30px var(--site-font-display, Archivo, 'Helvetica Neue', Arial, sans-serif)",
  'font-body':
    "400 15px/23px var(--site-font-body, 'Public Sans', system-ui, -apple-system, 'Segoe UI', sans-serif)",
  'font-label': `400 13px/21px ${MONO}`,
  'font-figure': `400 21px/29px ${MONO}`,
  'font-bit': `400 17px/34px ${MONO}`,
  'z-plane': '0',
  'z-mark': '10',
  'z-popover': '30',
  'z-overlay': '40',
  'z-sheet': '50',
  'z-status': '60',
};

/** The properties `_sass/_properties.sass` will emit on `:root`. */
const SITE_PROPERTIES = [
  'paper',
  'concrete',
  'concrete-deep',
  'rule',
  'slate',
  'navy',
  'ink',
  'signal-blue',
  'signal-teal',
  'signal-green',
  'signal-amber',
  'signal-red',
  'ramp-1',
  'ramp-2',
  'ramp-3',
  'ramp-4',
  'font-display',
  'font-body',
  'font-mono',
  'page-pad-left',
  'page-pad-right',
];

describe('tokens.css', () => {
  it('defines every token, each with the site property and the design value it falls back to, and no other', () => {
    expect(declared(tokensCss)).toEqual(TOKENS);
  });

  it('defines them on the tool root, where the components read them', () => {
    expect(tokensCss).toMatch(/^\.cache-explorer\s*\{/m);
    expect(tokensCss.match(/^[^\s/*][^{]*\{/gm)).toEqual(['.cache-explorer {']);
  });

  it('falls back only to properties the site emits', () => {
    for (const css of [tokensCss, componentsCss]) {
      const named = [...css.matchAll(/--site-([\w-]+)/g)].map(([, name]) => name);
      expect(named.filter((name) => !SITE_PROPERTIES.includes(name ?? ''))).toEqual([]);
    }
  });
});

describe('cache-explorer.css', () => {
  it('reads only tokens that tokens.css defines', () => {
    const used = [...componentsCss.matchAll(/var\(--cache-explorer-([\w-]+)/g)].map(
      ([, name]) => name ?? '',
    );
    expect(used.length).toBeGreaterThan(0);
    expect(used.filter((name) => !Object.hasOwn(TOKENS, name))).toEqual([]);
  });

  it('declares none of its own, and no z-index number', () => {
    expect(declared(componentsCss)).toEqual({});
    expect(componentsCss).not.toMatch(/z-index:\s*\d/);
  });

  it('takes the animation and the transition off everything for a student who asks for less motion', () => {
    const rule = /@media \(prefers-reduced-motion: reduce\)\s*\{([^@]*)\}\s*$/m.exec(componentsCss);
    expect(rule).not.toBeNull();
    const body = rule?.[1] ?? '';
    expect(body).toContain('.cache-explorer *');
    expect(body).toContain('.cache-explorer *::before');
    expect(body).toContain('.cache-explorer *::after');
    expect(body).toMatch(/animation:\s*none\s*!important/);
    expect(body).toMatch(/transition:\s*none\s*!important/);
  });

  it('sets the layout thresholds through data-layout, which the entry measures', () => {
    expect(componentsCss).toContain(".cache-explorer[data-layout='narrow']");
    expect(componentsCss).toContain(".cache-explorer[data-layout='tablet']");
  });
});
