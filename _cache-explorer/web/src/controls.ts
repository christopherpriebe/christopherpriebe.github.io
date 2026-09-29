// Form controls that outlive a render. The address, value, jump and timing inputs, the
// address-width select, the program textarea and the editors' inputs are created once for a root
// and put into their region on every render, so what the student has typed in them, their undo
// history and their selection are never rebuilt from state.

const stores = new WeakMap<HTMLElement, Map<string, HTMLElement>>();

/**
 * The control filed under `key` for this root, made by `make` the first time it is asked for. A
 * key always goes with the same kind of element.
 */
export function control<E extends HTMLElement>(root: HTMLElement, key: string, make: () => E): E {
  let store = stores.get(root);
  if (!store) {
    store = new Map();
    stores.set(root, store);
  }
  const existing = store.get(key);
  if (existing) return existing as E;
  const created = make();
  store.set(key, created);
  return created;
}

type ValueControl = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;

/**
 * Writes `value` into a control only when it differs and the student is not typing in it, so a
 * render never moves the caret or interrupts a composition.
 */
export function syncValue(target: ValueControl, value: string): void {
  if (target.value === value || target.ownerDocument.activeElement === target) return;
  target.value = value;
}
