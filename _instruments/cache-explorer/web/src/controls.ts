// Form controls that outlive a render. The address, value, jump and timing inputs, the
// address-width select, the program textarea and the editors' inputs are created once for a root
// and put into their region on every render by `commit`, so what the student has typed in them,
// their undo history and their selection are never rebuilt from state.
//
// Putting an element somewhere new normally removes it first, and removing the element that has
// focus drops focus and any composition in progress. `commit` therefore moves a control with
// `moveBefore` where the browser has it, which keeps both; elsewhere the control loses focus in
// the move, and focus.ts gives it back and puts the caret where it was.

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

function isValueControl(element: HTMLElement): element is ValueControl {
  return (
    element instanceof HTMLInputElement ||
    element instanceof HTMLTextAreaElement ||
    element instanceof HTMLSelectElement
  );
}

/**
 * Writes `value` into a control only when it differs and the student is not typing in it, so a
 * render never moves the caret or interrupts a composition.
 */
export function syncValue(target: ValueControl, value: string): void {
  if (target.value === value || target.ownerDocument.activeElement === target) return;
  target.value = value;
}

/**
 * Stands in, inside the content being built, for a persistent control; `commit` swaps it for the
 * control. With a `value`, the control is given that text first, as `syncValue` does.
 */
export interface Put {
  (control: HTMLElement): Node;
  (control: ValueControl, value: string): Node;
}

/** An element that may have `moveBefore`, which the DOM typings declare as always there. */
type Mover = Node & { moveBefore?: (node: Node, child: Node | null) => void };

/** Puts `node` before `before` in `parent`, without removing it first when the browser can. */
function move(parent: ParentNode, node: HTMLElement, before: Node): void {
  const mover: Mover = parent;
  // `moveBefore` refuses a node and a parent that are not in the same tree, as a control that has
  // never been placed, or one left in content that was taken out of the page, is not.
  if (typeof mover.moveBefore === 'function' && node.getRootNode() === parent.getRootNode()) {
    mover.moveBefore(node, before);
  } else {
    parent.insertBefore(node, before);
  }
}

/**
 * Replaces the content of `region` with the nodes `build` returns, as `replaceChildren` does, but
 * for the persistent controls in them: `build` gets `put` and puts its result where a control
 * belongs. The new content is added beside the old, each control moves into its place, and only
 * then is the old content removed, so a control is never removed from the page while it is
 * focused. The value a control is given is written before it moves, when it can still tell that
 * the student is typing in it.
 */
export function commit(region: HTMLElement, build: (put: Put) => Node[]): void {
  const placements: { stand: Comment; control: HTMLElement; value: string | undefined }[] = [];
  const put: Put = (control: HTMLElement, value?: string) => {
    const stand = region.ownerDocument.createComment('');
    placements.push({ stand, control, value });
    return stand;
  };
  const next = build(put);
  const previous = Array.from(region.childNodes);
  region.append(...next);
  for (const { stand, control, value } of placements) {
    const parent = stand.parentNode;
    if (!parent) throw new Error('a control was asked for and not put in the content');
    if (value !== undefined && isValueControl(control)) syncValue(control, value);
    move(parent, control, stand);
    stand.remove();
  }
  const kept = new Set<Node>(next);
  for (const node of previous) {
    if (!kept.has(node)) node.remove();
  }
}
