// What an action handler works with, and what every handler needs to know about the core's answers.
// A handler is a function of the context and the action of the element the event came from; it
// changes the session and the UI state, then asks for a draw.

import type { Cause } from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { UiState } from './ui-state';
import type { Session } from './wasm';

export interface Context {
  readonly session: Session;
  readonly ui: UiState;
  /** Asks the session again and draws what it says. */
  refresh(): void;
  /** Opens the configuration bar and flashes the group `cause` names. */
  flash(cause: Cause): void;
}

/** What an event on an element with a `data-action` does. */
export type ActionHandler = (context: Context, action: Action) => void;

/**
 * The core's reason for refusing what was asked, which it throws as an ordinary error. A trap in
 * the wasm is not a refusal, and neither is anything that is not an error: both are thrown on.
 */
export function refusal(error: unknown): string {
  if (error instanceof WebAssembly.RuntimeError || !(error instanceof Error)) throw error;
  return error.message;
}
