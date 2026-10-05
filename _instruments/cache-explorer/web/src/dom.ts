// Element builders. Text goes in as text nodes, never as markup, so nothing a student types or the
// core returns can become HTML.

/** An attribute value: `true` writes the bare attribute, `false`, `null` and `undefined` omit it. */
export type Attr = string | number | boolean | null | undefined;
export type Attrs = Readonly<Record<string, Attr>>;
/** A child: text or a node; `null`, `undefined` and `false` add nothing, so `cond && node` works. */
export type Child = Node | string | number | null | undefined | false;

const SVG_NAMESPACE = 'http://www.w3.org/2000/svg';

function fill<E extends Element>(el: E, attrs: Attrs | null, children: readonly Child[]): E {
  for (const [name, value] of Object.entries(attrs ?? {})) {
    if (value === undefined || value === null || value === false) continue;
    el.setAttribute(name, value === true ? '' : String(value));
  }
  for (const child of children) {
    if (child === undefined || child === null || child === false) continue;
    el.append(typeof child === 'number' ? String(child) : child);
  }
  return el;
}

/** An HTML element with attributes and children. */
export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs | null = null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  return fill(document.createElement(tag), attrs, children);
}

/** An SVG element with attributes and children. */
export function s<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Attrs | null = null,
  ...children: Child[]
): SVGElementTagNameMap[K] {
  return fill(document.createElementNS(SVG_NAMESPACE, tag), attrs, children);
}

/** A class attribute from the names that are set: `false`, `null` and `undefined` leave one out. */
export function cls(...names: (string | false | null | undefined)[]): string {
  return names.filter((name) => typeof name === 'string' && name !== '').join(' ');
}
