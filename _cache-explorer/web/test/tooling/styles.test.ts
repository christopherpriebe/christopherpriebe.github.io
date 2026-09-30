// @vitest-environment node
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { FLASH_MS, PACKET_MS, PACKET_TRAVEL_PX, PULSE_MS } from '../../src/motion';
import { MEMORY_ROW_PX } from '../../src/metrics';

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
  ways: '1',
  words: '4',
  'way-min': '292px',
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

  it('has the why flash fade in, hold and fade out over the time the script gives it', () => {
    const keyframes = /@keyframes cache-explorer-why\s*\{([\s\S]*?)\n\}/.exec(componentsCss)?.[1];
    expect(keyframes).toMatch(/0%,\s*100%\s*\{\s*background: transparent;/);
    expect(keyframes).toMatch(/25%,\s*75%\s*\{\s*background: var\(--cache-explorer-changed\);/);
    expect(componentsCss).toMatch(
      new RegExp(
        `\\.cache-explorer__group--why\\s*\\{\\s*animation: cache-explorer-why ${String(FLASH_MS)}ms`,
      ),
    );
  });

  it('keeps the pressed option of a read-only group whole and dims only the others', () => {
    const rules = [
      ...componentsCss.matchAll(/\.cache-explorer__seg--read-only [^{]*\{([^}]*)\}/g),
    ].map(([selector, body]) => [selector.split('{')[0]?.trim(), body?.trim()]);
    expect(rules).toEqual([
      ['.cache-explorer__seg--read-only .cache-explorer__seg-option', 'cursor: default;'],
      [
        ".cache-explorer__seg--read-only .cache-explorer__seg-option:not([aria-pressed='true'])",
        'opacity: 0.45;',
      ],
    ]);
  });

  it('gives a timing field the metrics of the design’s input, a normal line height', () => {
    const field = /^\.cache-explorer__field\s*\{([^}]*)\}/m.exec(componentsCss)?.[1];
    expect(field).toContain('line-height: normal');
  });

  it('shows the why flash’s ground still, without its fade, for a student who asks for less motion', () => {
    const reduced = /@media \(prefers-reduced-motion: reduce\)\s*\{([^@]*)\}\s*$/m.exec(
      componentsCss,
    );
    expect(reduced?.[1]).toMatch(
      /\.cache-explorer__group--why\s*\{\s*background: var\(--cache-explorer-changed\);/,
    );
  });

  it('draws a button in the secondary look, and the primary one over it', () => {
    const block = (selector: string): string =>
      new RegExp(`^${selector.replace('.', '\\.')}\\s*\\{([^}]*)\\}`, 'm').exec(
        componentsCss,
      )?.[1] ?? '';
    const base = block('.cache-explorer__button');
    expect(base).toContain('border: 1px solid var(--cache-explorer-rule)');
    expect(base).toContain('background: var(--cache-explorer-surface)');
    expect(base).toContain('color: var(--cache-explorer-text)');
    expect(base).toContain('border-radius: 0');
    const primary = block('.cache-explorer__button--primary');
    expect(primary).toContain('border-color: var(--cache-explorer-strong)');
    expect(primary).toContain('background: var(--cache-explorer-strong)');
    expect(primary).toContain('color: var(--cache-explorer-surface)');
  });

  /** The declarations of every rule whose selector list ends with `selector`, together. */
  const rule = (selector: string): string =>
    [
      ...componentsCss.matchAll(
        new RegExp(
          `(?:^|,\\s*)${selector.replace(/[.[\]()*+?^$|]/g, '\\$&')}\\s*\\{([^}]*)\\}`,
          'gm',
        ),
      ),
    ]
      .map(([, body]) => body)
      .join('');

  it('rings a changed cell for as long as the script says, with two identical keyframes', () => {
    for (const beat of ['a', 'b']) {
      expect(rule(`.cache-explorer__pulse-${beat}`)).toContain(
        `animation: cache-explorer-pulse-${beat} ${String(PULSE_MS)}ms ease-out`,
      );
    }
    const keyframes = (name: string): string | undefined =>
      new RegExp(`@keyframes ${name}\\s*\\{([\\s\\S]*?)\\n\\}`).exec(componentsCss)?.[1];
    expect(keyframes('cache-explorer-pulse-a')).toMatch(
      /from\s*\{\s*box-shadow: inset 0 0 0 2px var\(--cache-explorer-changed-ring\);/,
    );
    expect(keyframes('cache-explorer-pulse-a')).toMatch(
      /to\s*\{\s*box-shadow: inset 0 0 0 2px transparent;/,
    );
    expect(keyframes('cache-explorer-pulse-b')).toBe(keyframes('cache-explorer-pulse-a'));
  });

  it('crosses a packet down or up the lane in the time and the distance the script says', () => {
    for (const [direction, from] of [
      ['down', -PACKET_TRAVEL_PX],
      ['up', PACKET_TRAVEL_PX],
    ] as const) {
      for (const beat of ['a', 'b']) {
        expect(rule(`.cache-explorer__packet--${direction}-${beat}`)).toContain(
          `animation: cache-explorer-${direction}-${beat} ${String(PACKET_MS)}ms ease-out`,
        );
        expect(componentsCss).toMatch(
          new RegExp(
            `@keyframes cache-explorer-${direction}-${beat}\\s*\\{\\s*from\\s*\\{\\s*transform: translateY\\(${String(from)}px\\);`,
          ),
        );
      }
    }
  });

  it('gives a memory row the height the blank rows are counted in', () => {
    const row = rule('.cache-explorer__mem-row');
    expect(row).toContain('margin-top: 1px');
    expect(row).toContain('line-height: 28px');
    expect(28 + 1).toBe(MEMORY_ROW_PX);
  });

  it('gives the changed ground the last word among the cells that have a ground of their own', () => {
    const at = (selector: string): number => componentsCss.indexOf(`\n${selector} {`);
    for (const own of [
      '.cache-explorer__mem-word',
      '.cache-explorer__register',
      '.cache-explorer__way',
    ]) {
      expect(at(own)).toBeGreaterThan(-1);
      expect(at('.cache-explorer__changed')).toBeGreaterThan(at(own));
    }
  });

  it('lays the columns of a way out as the design does, dropping the ones that are hidden', () => {
    expect(rule('.cache-explorer__way--rank.cache-explorer__way--dirty')).toContain(
      'grid-template-columns: 24px 58px minmax(0, 1fr) 50px 24px',
    );
    expect(rule('.cache-explorer__way--rank')).toContain(
      'grid-template-columns: 24px 58px minmax(0, 1fr) 50px;',
    );
    expect(rule('.cache-explorer__way--dirty')).toContain(
      'grid-template-columns: 24px 58px minmax(0, 1fr) 24px;',
    );
  });

  it('gives the tiles of a wrapped set, and the heads over them, the same columns', () => {
    const shared = /\.cache-explorer__tiles,\s*\.cache-explorer__wrap-heads\s*\{([^}]*)\}/.exec(
      componentsCss,
    )?.[1];
    expect(shared).toContain(
      'grid-template-columns: repeat(auto-fill, minmax(var(--cache-explorer-way-min), 1fr))',
    );
    // Only the first row of heads has a height; the copies that fall on later rows have none.
    const heads = rule('.cache-explorer__wrap-heads');
    expect(heads).toContain('grid-template-rows: auto');
    expect(heads).toContain('grid-auto-rows: 0');
  });

  it('sets the layout thresholds through data-layout, which the entry measures', () => {
    expect(componentsCss).toContain(".cache-explorer[data-layout='narrow']");
    expect(componentsCss).toContain(".cache-explorer[data-layout='tablet']");
  });
});
