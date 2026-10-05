// The page's address: what `#s=` and `#m=` ask for when the page opens, and the link the page keeps
// up to date as the run goes on. The link is the core's (`toShareString`); this file only decides
// when it is written and where.

import { refusal } from './context';
import type { Session } from './wasm';

/** How long the cursor must stand still before its place is written into the address (9.2). */
export const HASH_DEBOUNCE_MS = 250;

/** What the address asks the page to open, or null when it asks for nothing it knows. */
export type HashTarget = { kind: 'share'; text: string } | { kind: 'module'; id: string } | null;

/** The target `hash`, as `location.hash` has it, names. The text after the `=` is passed on as it is. */
export function readHash(hash: string): HashTarget {
  if (hash.startsWith('#s=') && hash.length > 3) return { kind: 'share', text: hash.slice(3) };
  if (hash.startsWith('#m=') && hash.length > 3) return { kind: 'module', id: hash.slice(3) };
  return null;
}

/** The page's address as a link to the run in `text`: the page, without its old hash, and `#s=`. */
export function linkTo(page: string, text: string): string {
  return `${page.split('#')[0] ?? page}#s=${text}`;
}

/** Where the hash is written. Writing replaces the history entry: the back button never steps through a run. */
export interface HashStore {
  /** The page's address without its hash. */
  page(): string;
  /** Sets the hash to `hash`, or drops it with null. */
  replace(hash: string | null): void;
}

/** The browser's address bar, written with `history.replaceState`. */
export function browserStore(): HashStore {
  const page = (): string => location.origin + location.pathname + location.search;
  return {
    page,
    replace(hash) {
      const url = location.pathname + location.search + (hash ?? '');
      try {
        history.replaceState(history.state as unknown, '', url);
      } catch {
        // A browser limits how often the address may change. The address is a convenience: a
        // recovery reads the kept copy of the run, not the hash.
      }
    },
  };
}

/** What the page looked at last when it wrote: another of these means the address is out of date. */
interface Mark {
  ops: number;
  pos: number;
  advanced: boolean;
}

export interface Persistence {
  /** Takes `session` as it is without writing: the address already says it, or has nothing to say yet. */
  baseline(session: Session): void;
  /**
   * Writes the address now, whatever has changed, and keeps the run. Returns the core's note when
   * the run has outgrown a link, the one that points to Save, and null otherwise.
   */
  write(session: Session): string | null;
  /**
   * Called after each change. An op writes at once; a move of the cursor, or of the mode, writes
   * once the cursor has been still for `HASH_DEBOUNCE_MS`. Returns the note of a write made now.
   */
  sync(session: Session, mark: Mark): string | null;
  /** Drops the hash from the address, leaving the page as it is. */
  clear(): void;
  /** Writes what a debounce is waiting to write. */
  flush(): void;
  /** The last run kept: the saved file's text, from which the session is rebuilt after a fault. */
  lastGood(): string | null;
  /** Gives up what is waiting. */
  stop(): void;
}

/**
 * Keeps the address, and a copy of the run for a recovery, up to date. `current` is the session
 * to write when a debounce fires; `late` is told the note of a write that no op asked for.
 */
export function createPersistence(
  store: HashStore,
  current: () => Session | null,
  late: (note: string) => void,
): Persistence {
  let marked: Mark | null = null;
  let kept: string | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function cancel(): void {
    clearTimeout(timer);
    timer = undefined;
  }

  function write(session: Session): string | null {
    cancel();
    // The saved file first: it always exists, and it is what a recovery opens.
    kept = session.toJson();
    let hash: string | null;
    let note: string | null = null;
    try {
      hash = `#s=${session.toShareString()}`;
    } catch (error) {
      // The run has outgrown a link. The address must not keep an older one, which would open a
      // run the student has left behind.
      note = refusal(error);
      hash = null;
    }
    put(hash);
    return note;
  }

  /** Writes the address; a store that refuses leaves it as it was, and the page goes on. */
  function put(hash: string | null): void {
    try {
      store.replace(hash);
    } catch {
      // The address is a convenience, and the kept copy is what a recovery opens.
    }
  }

  return {
    baseline(session) {
      cancel();
      kept = session.toJson();
      marked = null;
    },
    write(session) {
      marked = null;
      return write(session);
    },
    sync(session, mark) {
      const before = marked;
      if (before?.ops === mark.ops && before.pos === mark.pos && before.advanced === mark.advanced)
        return null;
      marked = mark;
      // The first look only learns what the run is; the address already says it.
      if (before === null) return null;
      if (before.ops !== mark.ops) return write(session);
      cancel();
      timer = setTimeout(() => {
        timer = undefined;
        const session = current();
        if (!session) return;
        const note = write(session);
        if (note !== null) late(note);
      }, HASH_DEBOUNCE_MS);
      return null;
    },
    clear() {
      cancel();
      put(null);
    },
    flush() {
      if (timer === undefined) return;
      const session = current();
      if (session) write(session);
    },
    lastGood: () => kept,
    stop: cancel,
  };
}
