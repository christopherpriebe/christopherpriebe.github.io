// The modules the dev pages and the tests read: the shipped folder as it is, and three small fixtures,
// the files the core's own tests read, imported from them so the two cannot drift, and served from
// memory in place of the folder a page names. Nothing here is in the build.

import miss from '../../crates/cache-explorer-core/tests/fixtures/modules/01-fixture-miss-then-hit.json?raw';
import config from '../../crates/cache-explorer-core/tests/fixtures/modules/02-fixture-switch-config.json?raw';
import stale from '../../crates/cache-explorer-core/tests/fixtures/modules/03-fixture-stale-memory.json?raw';
import type { ModuleSource } from '../src/modules';

/** The modules' files, by id. */
export const FIXTURE_MODULES: ReadonlyMap<string, string> = new Map([
  ['01-fixture-miss-then-hit', miss],
  ['02-fixture-switch-config', config],
  ['03-fixture-stale-memory', stale],
]);

/** The index of those files, as the build would write it: each module's id, title, text and level. */
export const FIXTURE_INDEX: string = JSON.stringify(
  [...FIXTURE_MODULES].map(([id, file]) => {
    const { title, description, advanced } = JSON.parse(file) as Record<string, unknown>;
    return { id, title, description, advanced };
  }),
);

/** A source that answers at once from the fixtures. */
export function fixtureSource(): ModuleSource {
  return {
    index: () => Promise.resolve(FIXTURE_INDEX),
    module: (id) => {
      const file = FIXTURE_MODULES.get(id);
      return file === undefined
        ? Promise.reject(new Error(`no module ${id}`))
        : Promise.resolve(file);
    },
  };
}

const SHIPPED = import.meta.glob<string>('../../modules/*.json', {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** A source that answers at once from the shipped modules folder as it is: the dev pages' default. */
export function shippedSource(): ModuleSource {
  const file = (name: string): Promise<string> => {
    const text = SHIPPED[`../../modules/${name}.json`];
    return text === undefined
      ? Promise.reject(new Error(`no module ${name}`))
      : Promise.resolve(text);
  };
  return { index: () => file('index'), module: (id) => file(id) };
}
