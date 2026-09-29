// @vitest-environment node
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const script = fileURLToPath(new URL('../../../scripts/check-tokens.mjs', import.meta.url));

/** Runs the script over a folder holding one file. */
function check(name: string, text: string): { passed: boolean; report: string } {
  const dir = mkdtempSync(join(tmpdir(), 'check-tokens-'));
  try {
    writeFileSync(join(dir, name), text);
    const run = spawnSync(process.execPath, [script, dir], { encoding: 'utf8' });
    return { passed: run.status === 0, report: run.stderr };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe('signal colours as type', () => {
  it.each([
    ['a stylesheet declaration', 'a.css', '.x { color: var(--cache-explorer-evict); }'],
    ['a style string', 'a.ts', "el.setAttribute('style', 'color: var(--cache-explorer-tag)');"],
    ['a style property', 'a.ts', "el.style.color = 'var(--cache-explorer-evict)';"],
    ['setProperty', 'a.ts', "el.style.setProperty('color', 'var(--cache-explorer-tag)');"],
    ['a colour attribute', 'a.ts', "el.setAttribute('color', 'var(--cache-explorer-index)');"],
  ])('fail for %s', (_, name, text) => {
    const { passed, report } = check(name, text);
    expect(passed).toBe(false);
    expect(report).toContain('used as type colour');
  });

  it.each([
    ['a stylesheet rule on text', 'a.css', 'g text { fill: var(--cache-explorer-tag); }'],
    [
      'a text element built with a fill',
      'a.ts',
      "s('text', { fill: 'var(--cache-explorer-index)' });",
    ],
    [
      'setAttribute on a text element',
      'a.ts',
      "const t = s('text', null, 'tag');\nt.setAttribute('fill', 'var(--cache-explorer-index)');",
    ],
    [
      'a style property on a text element',
      'a.ts',
      "const t: SVGTextElement = document.createElementNS(NS, 'text');\nt.style.fill = 'var(--cache-explorer-offset)';",
    ],
    [
      'setProperty on a text element',
      'a.ts',
      "let t = s('text');\nt.style.setProperty('fill', 'var(--cache-explorer-evict)');",
    ],
  ])('fail for %s', (_, name, text) => {
    const { passed, report } = check(name, text);
    expect(passed).toBe(false);
    expect(report).toContain('used as the fill of text');
  });

  it.each([
    ['a fill on a shape', 'a.css', 'rect { fill: var(--cache-explorer-evict); }'],
    ['a background', 'a.css', '.x { background-color: var(--cache-explorer-evict); }'],
    ['a background property', 'a.ts', "el.style.backgroundColor = 'var(--cache-explorer-tag)';"],
    ['an ink colour', 'a.ts', "el.style.color = 'var(--cache-explorer-text)';"],
    ['a comparison', 'a.ts', "if (el.color == 'var(--cache-explorer-tag)') run();"],
    [
      'a fill set on a shape',
      'a.ts',
      "const r = s('rect');\nr.setAttribute('fill', 'var(--cache-explorer-tag)');",
    ],
    [
      'a signal fill set on another variable than the text',
      'a.ts',
      "const t = s('text');\nconst r = s('rect');\nr.setAttribute('fill', 'var(--cache-explorer-tag)');",
    ],
  ])('pass for %s', (_, name, text) => {
    expect(check(name, text)).toEqual({ passed: true, report: '' });
  });
});

describe("the design export's token names", () => {
  it('fail for a --cl-* name, in a stylesheet or a script', () => {
    for (const [name, text] of [
      ['a.css', '.x { background: var(--cl-plane); }'],
      ['a.ts', "el.style.background = 'var(--cl-plane)';"],
    ] as const) {
      const { passed, report } = check(name, text);
      expect(passed).toBe(false);
      expect(report).toContain('a --cl-* token name');
    }
  });
});
