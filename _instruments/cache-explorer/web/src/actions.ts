// What an actionable element says about itself: `data-action` names it, and the attributes below
// say which set, way, position, group, option or address it acts on. Render writes them, the
// delegated listeners read them, and focus is kept across a render by them.

/** The attributes that parameterise an action, without their `data-` prefix. */
export const PARAMS = [
  'set',
  'way',
  'pos',
  'step',
  'group',
  'option',
  'module',
  'addr',
  'line',
  'column',
  'length',
] as const;
export type Param = (typeof PARAMS)[number];

export interface Action {
  readonly name: string;
  /** The parameters the element carries, as the text render wrote. */
  readonly params: Readonly<Partial<Record<Param, string>>>;
  readonly element: HTMLElement | SVGElement;
}

/** The action an element is, or null when it carries no `data-action`. */
export function readAction(element: Element): Action | null {
  if (!(element instanceof HTMLElement || element instanceof SVGElement)) return null;
  const name = element.dataset.action;
  if (name === undefined) return null;
  const params: Partial<Record<Param, string>> = {};
  for (const param of PARAMS) {
    const value = element.dataset[param];
    if (value !== undefined) params[param] = value;
  }
  return { name, params, element };
}

/** The action of the nearest element at or above `target` inside `root`, or null when none is. */
export function actionAt(target: EventTarget | null, root: HTMLElement): Action | null {
  if (!(target instanceof Element)) return null;
  const element = target.closest('[data-action]');
  return element && root.contains(element) ? readAction(element) : null;
}
