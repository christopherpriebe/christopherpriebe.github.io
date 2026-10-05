import { describe, expect, it } from 'vitest';
import { h, s } from '../src/dom';

describe('h', () => {
  it('builds an element with attributes and children', () => {
    const button = h('button', { type: 'button', 'data-action': 'step' }, 'Step ', 2);
    expect(button.outerHTML).toBe('<button type="button" data-action="step">Step 2</button>');
  });

  it('writes a true attribute bare and leaves out false, null and undefined', () => {
    const input = h('input', { disabled: true, hidden: false, title: null, name: undefined });
    expect(input.outerHTML).toBe('<input disabled="">');
  });

  it('skips children that are false, null or undefined', () => {
    const list = h('ul', null, h('li', null, 'a'), false, null, undefined, h('li', null, 'b'));
    expect(list.children).toHaveLength(2);
  });

  it('takes text as text, never as markup', () => {
    const label = h('span', { title: '"><script>' }, '<img src=x onerror=alert(1)>');
    expect(label.children).toHaveLength(0);
    expect(label.textContent).toBe('<img src=x onerror=alert(1)>');
    expect(label.getAttribute('title')).toBe('"><script>');
  });
});

describe('s', () => {
  it('builds an SVG element in the SVG namespace', () => {
    const rect = s('rect', { width: 12, height: 9 });
    expect(rect.namespaceURI).toBe('http://www.w3.org/2000/svg');
    expect(rect.getAttribute('width')).toBe('12');
  });

  it('nests SVG children', () => {
    const svg = s('svg', { viewBox: '0 0 10 10' }, s('circle', { r: 4 }));
    expect(svg.querySelector('circle')?.namespaceURI).toBe('http://www.w3.org/2000/svg');
  });
});
