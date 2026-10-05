// Opening a run: a link, a saved file or a trace. A link or a saved file replaces the session, so it
// follows the load-memory rule (9.4): the page takes the current run as JSON, lets the session go,
// and only then lets the core replay the new one, so the old and the new run are never in memory
// together. The replay goes a slice a frame (`ShareLoad.advance`); the loading state shows once
// 200 ms have passed since the page began opening it. If the run cannot be opened, the page builds
// the old one again from the JSON and shows the core's account of why.

import type { ActionHandler, Context } from './context';
import { isFatal, refusal } from './context';
import { copy } from './copy';
import { isTrace, sizeForCore } from './files';
import { LOADING_DELAY_MS } from './motion';
import { releaseModule } from './module-actions';
import type { SourceKind, UiState } from './ui-state';
import { Session, ShareLoad, runFileTooLarge, traceFileTooLarge } from './wasm';

/** What a run is opened from: the text after `#s=`, or a saved file's JSON. */
export interface Source {
  kind: SourceKind;
  text: string;
}

export interface OpenOptions {
  /** When the page began opening it, in the milliseconds `performance.now` counts; the loading state shows 200 ms later. */
  since?: number;
  /** Whether the run being given up is one to offer a way back to when this one cannot be opened. */
  keep?: boolean;
  /** Whether to write the opened run into the address: not for a link that is already there. */
  write?: boolean;
  /** The file it came from, for the status line. */
  name?: string;
}

/** Whether the loading state is due: it shows only for a load that has taken a while. */
export function loadingDue(since: number, now: number = performance.now()): boolean {
  return now - since >= LOADING_DELAY_MS;
}

/** Stops a load in progress, if there is one, and frees what it holds. */
export function cancelLoading(context: Pick<Context, 'ui'>): void {
  const loading = context.ui.loading;
  context.ui.loading = null;
  try {
    loading?.cancel();
  } catch (error) {
    // A fault can leave the load borrowed: it is let go of, and not freed again.
    if (!isFatal(error)) throw error;
  }
}

/**
 * Opens `source` in place of the run on the page. The first slice is replayed at once, so a link
 * that opens in one never shows the loading state; the rest follow a frame apart.
 */
export function openSource(context: Context, source: Source, options: OpenOptions = {}): void {
  const { ui } = context;
  const { since = performance.now(), keep = true, write = true, name } = options;
  context.setRunning(false);
  cancelLoading(context);
  // The run as it is, before anything is let go: what the page goes back to if this one fails.
  const backup = context.session.toJson();
  // What the student typed, which a run cannot rebuild: it goes back if this one fails.
  const carried = {
    program: { ...ui.program },
    draft: { ...ui.draft },
    configOpen: ui.configOpen,
    addressOpen: ui.addressOpen,
  };
  releaseModule(ui);
  context.takeSession().free();
  const load =
    source.kind === 'json'
      ? ShareLoad.fromJson(source.text)
      : ShareLoad.fromShareString(source.text);
  let frame: number | undefined;
  let owned = true;
  /** Lets the load go, once: from its end, from a new load and from the page's disposal. */
  const release = (): void => {
    if (frame !== undefined) cancelAnimationFrame(frame);
    frame = undefined;
    if (!owned) return;
    owned = false;
    load.free();
  };
  const loading: NonNullable<UiState['loading']> = {
    since,
    from: source.kind,
    summary: null,
    cancel: release,
  };
  ui.loading = loading;
  ui.failure = null;
  ui.status = '';

  function finish(): void {
    release();
    ui.loading = null;
  }

  /** One slice, run as a click is: a fault in the core goes back to the run this began from. */
  function step(): void {
    frame = undefined;
    context.guard(slice, backup);
  }

  function slice(): void {
    if (context.disposed || ui.loading !== loading) return;
    const progress = load.advance();
    if (progress.status === 'replaying') {
      loading.summary = progress.summary;
      if (loadingDue(since)) context.refresh();
      frame = requestAnimationFrame(step);
      return;
    }
    if (progress.status === 'done') {
      const next = load.takeSession();
      finish();
      context.adoptSession(next);
      const note = write ? context.persist.write(next) : null;
      if (!write) context.persist.baseline(next);
      ui.status = note ?? (name === undefined ? '' : copy.transport.opened(name));
      context.refresh();
      return;
    }
    const { failure } = progress;
    finish();
    const restored = Session.fromJson(backup);
    context.adoptSession(restored, carried);
    context.persist.baseline(restored);
    ui.failure = { failure, from: source.kind, keep, contents: false };
    context.refresh();
  }

  step();
}

/** Starts again from a first visit's run: what "Start fresh" opens, and it leaves the address bare. */
function startFresh(context: Context): void {
  cancelLoading(context);
  releaseModule(context.ui);
  const next = Session.firstVisit();
  context.adoptSession(next);
  context.persist.clear();
  context.persist.baseline(next);
  context.refresh();
}

/** Opens the share string of the failure's repair or prefix, the core's own alternative. */
function openAlternative(which: 'repair' | 'prefix'): ActionHandler {
  return (context) => {
    const { failure } = context.ui;
    const option = failure?.failure[which];
    if (!failure || !option) return;
    openSource(context, { kind: 'share', text: option.share }, { keep: failure.keep });
  };
}

function toggleContents(context: Context): void {
  const { failure } = context.ui;
  if (!failure) return;
  failure.contents = !failure.contents;
  context.refresh();
}

function keepRun(context: Context): void {
  context.ui.failure = null;
  context.refresh();
}

/** Reads a trace into the run, as the program drawer's text would be loaded: one `Program` op in trace format. */
function loadTrace(context: Context, text: string, name: string): void {
  const { ui } = context;
  context.setRunning(false);
  try {
    const parsed = context.session.parseProgram(text, 'trace');
    if (parsed.status === 'invalid') {
      // The drawer shows the core's own line-marked account of the text, to fix in place.
      ui.program = { ...ui.program, format: 'trace', text, editing: true, open: true };
      ui.status = parsed.summary;
    } else {
      context.session.applyOp({ op: 'program', source: text, format: 'trace' });
      ui.program = { ...ui.program, format: 'trace', text, editing: false, open: true };
      ui.status = copy.transport.traceLoaded(name);
    }
  } catch (error) {
    ui.status = refusal(error);
  }
  context.refresh();
}

/**
 * Whether the core has Load switched off, as it does under an unanswered prediction: the guard
 * goes to the status line and nothing is opened (10.4).
 */
export function loadBlocked(context: Context): boolean {
  const { controls } = context.session.state();
  if (controls.enabled.includes('load')) return false;
  context.ui.status = controls.guard ?? '';
  context.refresh();
  return true;
}

/**
 * Opens a file the student chose: a `.txt` is a trace, which goes into the run; any other is a
 * saved run, which replaces it. A file too large for the core to open is refused before it is
 * read, since wasm memory taken for its text is never given back.
 */
export async function openFile(context: Context, file: File): Promise<void> {
  const { ui } = context;
  const trace = isTrace(file.name);
  const tooLarge = (trace ? traceFileTooLarge : runFileTooLarge)(sizeForCore(file.size));
  if (tooLarge !== undefined) {
    ui.status = tooLarge;
    context.refresh();
    return;
  }
  const since = performance.now();
  const text = await file.text();
  if (context.disposed || ui.loading) return;
  // The page can have changed while the file was read.
  if (loadBlocked(context)) return;
  context.guard(() => {
    if (trace) loadTrace(context, text, file.name);
    else openSource(context, { kind: 'json', text }, { since, name: file.name });
  });
}

/** The handlers of the failure state's actions, by `data-action`. */
export const loadActions = {
  clicks: new Map<string, ActionHandler>([
    ['load-fresh', startFresh],
    ['load-repair', openAlternative('repair')],
    ['load-prefix', openAlternative('prefix')],
    ['load-keep', keepRun],
    ['load-contents', toggleContents],
  ]),
};
