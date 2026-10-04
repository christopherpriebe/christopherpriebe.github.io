// Share, Save and Load: the three words at the right of the transport bar. Share copies a link to
// the run, Save hands over the run as a file, and Load asks for a file to open. What a link or a
// file holds, and every sentence about it, is the core's.

import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { copy } from './copy';
import { download, pickFile, runFileName } from './files';
import { linkTo } from './hash';
import { loadBlocked, openFile } from './load-actions';

/** Puts focus in the field that shows the link, with the link selected, for the student to copy. */
function selectLink(context: Context): void {
  const field = context.root.querySelector<HTMLInputElement>('[data-action="share-link"]');
  field?.focus({ preventScroll: true });
  field?.select();
}

/**
 * Copies a link to the run. The clipboard exists only on a secure page, and may refuse; the link
 * is then shown in a field to copy by hand. A run too long for a link says so in the core's words,
 * which point to Save.
 */
export async function share(context: Context): Promise<void> {
  const { ui } = context;
  let link: string | undefined;
  let said = '';
  // Run as a click is: this is called from a handler that does not wait for it.
  context.guard(() => {
    try {
      link = linkTo(context.page(), context.session.toShareString());
      said = context.session.shareSummary().copiedText;
    } catch (error) {
      link = undefined;
      ui.status = refusal(error);
      ui.shareFallback = null;
      context.refresh();
    }
  });
  if (link === undefined) return;
  let copied = false;
  // `navigator.clipboard` is missing on a page served over plain HTTP, and a browser may refuse.
  const clipboard = navigator.clipboard as Clipboard | undefined;
  if (clipboard) {
    try {
      await clipboard.writeText(link);
      copied = true;
    } catch {
      copied = false;
    }
  }
  if (context.disposed) return;
  ui.shareFallback = copied ? null : link;
  ui.status = copied ? said : '';
  context.refresh();
  if (!copied) selectLink(context);
}

/** Downloads the run as a compact `.cache-explorer.json`, which the core never lets outgrow what Load can open. */
export function save(context: Context): void {
  const { ui } = context;
  try {
    const name = runFileName();
    download(name, context.session.toJson());
    ui.status = copy.transport.saved(name);
  } catch (error) {
    ui.status = refusal(error);
  }
  context.refresh();
}

/** Asks for a file: a saved run, or a `.txt` trace. */
async function load(context: Context): Promise<void> {
  if (loadBlocked(context)) return;
  const file = await pickFile('.json,.txt,application/json,text/plain');
  if (file && !context.disposed) await openFile(context, file);
}

function closeLink(context: Context): void {
  context.ui.shareFallback = null;
  context.refresh();
}

/** The handlers of the three words and of the link field's "Done", by `data-action`. */
export const shareActions = {
  clicks: new Map<string, ActionHandler>([
    [
      'share',
      (context) => {
        void share(context);
      },
    ],
    ['save', save],
    [
      'load',
      (context) => {
        void load(context);
      },
    ],
    ['share-close', closeLink],
  ]),
};
