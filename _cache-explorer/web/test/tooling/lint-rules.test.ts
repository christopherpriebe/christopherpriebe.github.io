// @vitest-environment node
import { fileURLToPath } from 'node:url';
import { ESLint } from 'eslint';
import tseslint from 'typescript-eslint';
import { describe, expect, it } from 'vitest';

const webRoot = fileURLToPath(new URL('../../', import.meta.url));

// The rules under test read syntax only, and a probe that is not on disk has no type information.
const eslint = new ESLint({ cwd: webRoot, overrideConfig: tseslint.configs.disableTypeChecked });

/** The banned-number and banned-JSON rules that fire on this code when it sits in the UI's sources. */
async function restrictions(code: string): Promise<string[]> {
  const [result] = await eslint.lintText(code, { filePath: `${webRoot}src/probe.ts` });
  return (result?.messages ?? [])
    .map((message) => message.ruleId ?? message.message)
    .filter((rule) => rule.startsWith('no-restricted-'));
}

describe('the number and JSON bans', () => {
  it.each([
    ["parseInt('3');", 'no-restricted-globals'],
    ["parseFloat('3');", 'no-restricted-globals'],
    ['BigInt(3);', 'no-restricted-globals'],
    ["Number.parseInt('3');", 'no-restricted-properties'],
    ["Number.parseFloat('3');", 'no-restricted-properties'],
    ["JSON.parse('1');", 'no-restricted-properties'],
  ])('stop %s', async (code, rule) => {
    expect(await restrictions(code)).toEqual([rule]);
  });

  it.each([
    "globalThis.parseInt('3');",
    "window.parseFloat('3');",
    'self.BigInt(3);',
    "globalThis['parseInt']('3');",
    "globalThis.JSON.parse('1');",
    "window.JSON.parse('1');",
    "globalThis.Number.parseInt('3');",
    "self.Number.parseFloat('3');",
  ])('stop %s through the global object', async (code) => {
    expect(await restrictions(code)).toEqual(['no-restricted-properties']);
  });

  it('leave the rest of JSON and Number alone', async () => {
    expect(await restrictions('JSON.stringify({});')).toEqual([]);
    expect(await restrictions('Number.isInteger(3);')).toEqual([]);
  });
});
