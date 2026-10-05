// The machine: main memory, the transfer lane, the cache with its address strip and wires, the
// request lane and the CPU, top to bottom, as the design stacks them. Every part is an element the
// root keeps, and a render replaces what is in each, so a part that has state of its own (the memory
// scroller's position, the jump box, the cache box's size watcher) is never taken out of the page.

import { h } from '../dom';
import { beatOf } from '../motion';
import type { UiState } from '../ui-state';
import { createCache, renderCache, type CacheBox } from './cache';
import { renderCpu } from './cpu';
import { memoryEditor, memoryEditorOf, wayEditor, wayEditorOf } from './editor';
import type { Snapshot } from './index';
import { renderRequestLane, renderTransferLane } from './lanes';
import { marksOf } from './marks';
import { createMemory, renderMemory, type MemoryBox } from './memory';
import { addressSource } from './source';
import { stopWires } from './wires';

export interface Machine {
  element: HTMLElement;
  memory: MemoryBox;
  transfer: HTMLElement;
  cache: CacheBox;
  request: HTMLElement;
  cpu: HTMLElement;
}

/** `resized` is called when the cache box changes size. */
export function createMachine(resized: () => void): Machine {
  const memory = createMemory();
  const cache = createCache(resized);
  const transfer = h('div', { class: 'cache-explorer__lane cache-explorer__lane--transfer' });
  const request = h('div', { class: 'cache-explorer__lane cache-explorer__lane--request' });
  const cpu = h('div', { class: 'cache-explorer__cpu' });
  const element = h(
    'div',
    { class: 'cache-explorer__machine' },
    memory.element,
    transfer,
    cache.element,
    request,
    cpu,
  );
  return { element, memory, transfer, cache, request, cpu };
}

export function renderMachine(
  root: HTMLElement,
  machine: Machine,
  snap: Snapshot,
  ui: UiState,
): void {
  const { state } = snap;
  const source = addressSource(snap, ui);
  const marks = marksOf(snap, ui);
  const beat = beatOf(state.position.pos, ui.lastPos);
  // A step brings the line on the bus into view; the student's own jump brings the row they asked for.
  const reveal = beat === null ? ui.memoryReveal : (state.memory.busRow ?? null);
  const word = memoryEditorOf(snap, ui);
  renderMemory(
    root,
    machine.memory,
    snap,
    ui,
    marks,
    reveal,
    word && ((put) => memoryEditor(put, root, word.editor, word.draft)),
  );
  renderTransferLane(machine.transfer, state.bus, beat);
  const way = wayEditorOf(snap, ui);
  renderCache(
    machine.cache,
    snap,
    source,
    marks,
    way && {
      set: way.editor.set,
      way: way.editor.way,
      build: (put) => wayEditor(put, root, snap, way.editor, way.draft),
    },
  );
  renderRequestLane(machine.request, source);
  renderCpu(machine.cpu, snap, marks);
}

/** Stops what the machine watches. */
export function stopMachine(machine: Machine): void {
  stopWires(machine.cache.wires);
}
