// Where the guided modules come from: the index and one file for each module, fetched from the
// folder the page names. The text goes straight to the core, which reads it; nothing here looks
// inside it.

/** Fetches the index and a module's file as text. Each rejects when the file cannot be had. */
export interface ModuleSource {
  index(): Promise<string>;
  module(id: string): Promise<string>;
}

/**
 * The modules in the folder the index at `indexUrl` is in. The URL is resolved against the page, so
 * a page under a base path finds them where the build put them; each module's file is named by its
 * id, beside the index.
 */
export function fetchModules(
  indexUrl: string,
  fetcher: typeof fetch = fetch,
  base: string = document.baseURI,
): ModuleSource {
  const index = new URL(indexUrl, base);
  async function text(url: URL): Promise<string> {
    const response = await fetcher(url);
    if (!response.ok) throw new Error(`${url.pathname}: ${String(response.status)}`);
    return response.text();
  }
  return {
    index: () => text(index),
    module: (id) => text(new URL(`${id}.json`, index)),
  };
}
