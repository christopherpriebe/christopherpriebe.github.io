// What an action handler works with, and what every handler needs to know about the core's answers.
// A handler is a function of the context and the action of the element the event came from; it
// changes the session and the UI state, then asks for a draw.

import type { Cause } from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { Persistence } from './hash';
import type { ModuleSource } from './modules';
import type { UiState } from './ui-state';
import type { Session } from './wasm';

export interface Context {
  /** The element the tool is drawn into: where focus goes back to after a sheet or an editor closes. */
  readonly root: HTMLElement;
  readonly session: Session;
  readonly ui: UiState;
  /** Where the guided modules are fetched from, or null when the page offers none. */
  readonly modules: ModuleSource | null;
  /** The controller has been disposed: what was waited for is of no use. */
  readonly disposed: boolean;
  /** The address and the copy of the run that a recovery opens. */
  readonly persist: Persistence;
  /** The page's own address without its hash, which a link is made from. */
  page(): string;
  /** Asks the session again and draws what it says. */
  refresh(): void;
  /**
   * Gives up the session: the caller frees it, once, and the controller has none until
   * `adoptSession` gives it one. Nothing draws from the session meanwhile.
   */
  takeSession(): Session;
  /** Takes `next` as the run, with the page's input reset as a new run's is; the old session, if any, is freed. */
  adoptSession(next: Session): void;
  /** Opens the configuration bar and flashes the group `cause` names. */
  flash(cause: Cause): void;
  /** Starts or stops the Run timer, and says so in the UI state; it does not draw. */
  setRunning(on: boolean): void;
}

/** What an event on an element with a `data-action` does. */
export type ActionHandler = (context: Context, action: Action) => void;

/**
 * Whether `error` leaves the session it came from unusable (11.6): the module trapped, or a call
 * was made on an object an earlier fault left borrowed.
 */
export function isFatal(error: unknown): boolean {
  return (
    error instanceof WebAssembly.RuntimeError ||
    (error instanceof Error &&
      /recursive use of an object|attempted to take ownership/.test(error.message))
  );
}

/**
 * The core's reason for refusing what was asked, which it throws as an ordinary error. A trap in
 * the wasm or a call on an object a fault left borrowed is not a refusal, and neither is anything
 * that is not an error: all are thrown on.
 */
export function refusal(error: unknown): string {
  if (isFatal(error) || !(error instanceof Error)) throw error;
  return error.message;
}
