import { describe, expect, it } from 'vitest';
import { h } from '../src/dom';
import { FLASH_MS, flashAge } from '../src/motion';

describe('flashAge', () => {
  it('is null while no flash is showing', () => {
    expect(flashAge(h('div'), null, 500)).toBeNull();
  });

  it('is 0 for the render that first draws a flash, then how long ago that was', () => {
    const root = h('div');
    expect(flashAge(root, 'lineSize', 1000)).toBe(0);
    expect(flashAge(root, 'lineSize', 1250)).toBe(250);
    expect(flashAge(root, 'lineSize', 1250 + FLASH_MS - 251)).toBe(FLASH_MS - 1);
  });

  it('starts again for another cause', () => {
    const root = h('div');
    flashAge(root, 'lineSize', 1000);
    expect(flashAge(root, 'timing', 1400)).toBe(0);
    expect(flashAge(root, 'timing', 1500)).toBe(100);
  });

  it('starts again once no flash was showing, even for the same cause', () => {
    const root = h('div');
    flashAge(root, 'writePolicy', 1000);
    expect(flashAge(root, null, 1200)).toBeNull();
    expect(flashAge(root, 'writePolicy', 1300)).toBe(0);
  });

  it('starts again when the same cause comes back after a whole flash, though nothing ended it', () => {
    const root = h('div');
    flashAge(root, 'replacement', 1000);
    expect(flashAge(root, 'replacement', 1000 + FLASH_MS)).toBe(0);
  });

  it('keeps a flash for each root', () => {
    const one = h('div');
    const other = h('div');
    flashAge(one, 'lineSize', 1000);
    expect(flashAge(other, 'lineSize', 1300)).toBe(0);
    expect(flashAge(one, 'lineSize', 1300)).toBe(300);
  });
});
