// Files in and out of the page: Save hands the browser a file to download, and Load asks the student
// for one. Reading what is in a file is the core's work; this only moves the bytes.

const SUFFIX = '.cache-explorer.json';

/** The name Save gives the file: `run-2026-09-24.cache-explorer.json`, for the student's local date. */
export function runFileName(now: Date = new Date()): string {
  const pad = (n: number): string => String(n).padStart(2, '0');
  return `run-${String(now.getFullYear())}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}${SUFFIX}`;
}

/** Whether a file is a trace, which is read as text and loaded into the run; any other is a saved run. */
export function isTrace(name: string): boolean {
  return name.toLowerCase().endsWith('.txt');
}

/** A file's size as the core takes it: a count of bytes that fits 32 bits, a larger file being as too large as a smaller one. */
export function sizeForCore(size: number): number {
  return Math.min(size, 0xffffffff);
}

/** Offers `text` to the browser as a download called `name`. */
export function download(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.hidden = true;
  document.body.append(link);
  link.click();
  link.remove();
  // The download has started by the time the click returns; the address can go.
  setTimeout(() => {
    URL.revokeObjectURL(url);
  }, 0);
}

/** Asks the student for a file, and resolves with it, or with null when they close the dialog without one. */
export function pickFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.hidden = true;
    input.addEventListener('change', () => {
      resolve(input.files?.[0] ?? null);
      input.remove();
    });
    // A dialog closed without a choice, where the browser says so.
    input.addEventListener('cancel', () => {
      resolve(null);
      input.remove();
    });
    document.body.append(input);
    input.click();
  });
}
