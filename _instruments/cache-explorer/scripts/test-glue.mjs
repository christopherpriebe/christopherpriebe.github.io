// Checks the release build in web/pkg, glue included, against arguments and calls JavaScript could
// send to break the module: each must throw an Error, and the module must keep working after all
// of them. Run through `npm run test:glue` in web/, after `npm run build:wasm`.
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const pkg = resolve(import.meta.dirname, '..', 'web', 'pkg');
if (!existsSync(resolve(pkg, 'cache_explorer_bg.wasm'))) {
  console.error('test:glue: web/pkg has no build; run `npm run build:wasm` first');
  process.exit(1);
}
const glue = await import(pathToFileURL(resolve(pkg, 'cache_explorer.js')).href);
glue.initSync({ module: readFileSync(resolve(pkg, 'cache_explorer_bg.wasm')) });
const { ModuleRun, Session, ShareLoad, evalPredicate, parseModule } = glue;

const throws = (call, message) => assert.throws(call, (e) => e.constructor === Error && message.test(e.message));
const kept = Session.firstVisit();
kept.applyOp({ op: 'issue', kind: 'read', addr: 164 });
const json = kept.toJson();

// Text, number and boolean arguments are type-checked by the glue.
const session = new Session();
for (const value of [5, true, 7n, {}, [], null]) {
  throws(() => Session.fromJson(value), /^expected a string argument/);
  throws(() => ShareLoad.fromJson(value), /^expected a string argument/);
  throws(() => parseModule(value), /^expected a string argument/);
  throws(() => session.readAddress(value), /^expected a string argument/);
}
throws(() => session.scrubTo('5'), /^expected a number argument/);
throws(() => session.setAdvanced('true'), /^expected a boolean argument/);

// A freed object refuses every method call before it reaches wasm, however many there are.
const freed = new Session();
freed.free();
for (let i = 0; i < 20_000; i += 1) throws(() => freed.state(), /^Attempt to use a moved value/);

// Releasing an object twice is not refused by the glue: a second free(), or [Symbol.dispose](),
// which `using` calls when its block ends, passes a null pointer into wasm and throws from Rust.
// Each such throw leaks its stack frame, and a few hundred break the module, so the page releases
// every object exactly once. Checked here, a few times only, so that a change in the glue shows.
const modules = resolve(import.meta.dirname, '..', '..', 'crates', 'cache-explorer-core', 'tests', 'fixtures', 'modules');
const moduleJson = readFileSync(resolve(modules, '01-fixture-miss-then-hit.json'), 'utf8');
for (const make of [() => new Session(), () => ShareLoad.fromJson(json), () => ModuleRun.fromJson(moduleJson)]) {
  const twice = make();
  twice.free();
  throws(() => twice.free(), /^null pointer passed to rust$/);
  const disposed = make();
  disposed.free();
  throws(() => disposed[Symbol.dispose](), /^null pointer passed to rust$/);
}
throws(() => {
  using scoped = new Session();
  scoped.free();
}, /^null pointer passed to rust$/);

// A watch list is read whole: nothing is allocated from a length, and a non-list throws.
const sparse = [];
sparse.length = 2 ** 30 + 1;
for (const value of [sparse, 5, {}, null]) throws(() => session.setMemoryWatch(value), /^invalid LineAddr\[\]: /);

// A predicate nested far deeper than the stack could follow is refused before it is read.
let deep = { counter: 'accesses', op: '==', value: 0 };
for (let i = 0; i < 10_000; i += 1) deep = { not: deep };
throws(() => evalPredicate(session, deep), /^invalid Predicate: predicates nest at most /);
throws(() => session.findEvent(deep), /^invalid Predicate: predicates nest at most /);

// A getter that throws while a predicate nested inside a `not` chain is read leaves no nesting
// depth behind: afterwards, a predicate nested exactly as deep as the limit allows still reads,
// and so does a module whose steps hold predicates. A throw mid-argument leaves the session
// passed alongside the predicate borrowed, as `borrowed` below shows, so this uses one of its own.
const poisonedSession = new Session();
let poisoned = { get counter() { throw new Error('boom'); } };
for (let i = 0; i < 5; i += 1) poisoned = { not: poisoned };
const errorLog = console.error;
console.error = () => {}; // the glue logs the exception the getter threw
try {
  throws(() => evalPredicate(poisonedSession, poisoned), /^boom$/);
} finally {
  console.error = errorLog;
}
// 128, the core's MAX_PREDICATE_DEPTH: the JavaScript API has no constant for it, so the limit is
// spelled out here.
let deepest = { counter: 'accesses', op: '==', value: 0 };
for (let i = 0; i < 128; i += 1) deepest = { not: deepest };
assert.equal(typeof evalPredicate(session, deepest), 'boolean');
assert.ok(parseModule(moduleJson));

// The page passes only plain data: an exception thrown while an argument is read leaves the
// object borrowed, and it refuses every later call. Other objects keep working.
const borrowed = new Session();
const log = console.error;
console.error = () => {}; // the glue logs the exception an import threw
try {
  throws(() => borrowed.applyOp({ get op() { throw new Error('boom'); } }), /^boom$/);
} finally {
  console.error = log;
}
throws(() => borrowed.state(), /^recursive use of an object detected/);

assert.equal(kept.toJson(), json);
assert.equal(Session.fromJson(json).toJson(), json);
session.applyOp({ op: 'issue', kind: 'read', addr: 164 });
assert.equal(session.opCount(), 1);
console.log('test:glue: ok');
