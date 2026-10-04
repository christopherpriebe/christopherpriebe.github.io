import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { defineConfig, type Plugin } from 'vite';

const MODULES = '../modules';

/** Hash of the wasm and the stylesheet sources, appended as ?v= to both URLs (cache consistency after a deploy). */
function buildHash(): string {
  const h = createHash('sha256').update(readFileSync('pkg/cache_explorer_bg.wasm'));
  for (const f of readdirSync('src/styles').sort()) h.update(readFileSync(join('src/styles', f)));
  return h.digest('hex').slice(0, 12);
}

/**
 * The build copies the shipped modules/*.json beside the bundle as modules/*.json; the dev server
 * serves them at /modules/. A build without the folder or its index is an error.
 */
function modules(): Plugin {
  const files = () =>
    existsSync(MODULES)
      ? readdirSync(MODULES)
          .filter((f) => f.endsWith('.json'))
          .sort()
      : [];
  return {
    name: 'cache-explorer-modules',
    generateBundle() {
      if (!existsSync(join(MODULES, 'index.json'))) {
        this.error(`the shipped modules are missing: ${join(MODULES, 'index.json')} not found`);
      }
      for (const f of files()) {
        this.emitFile({
          type: 'asset',
          fileName: `modules/${f}`,
          source: readFileSync(join(MODULES, f)),
        });
      }
    },
    configureServer(server) {
      server.middlewares.use('/modules', (req, res, next) => {
        const f = (req.url ?? '').replace(/^\//, '');
        if (!files().includes(f)) {
          next();
          return;
        }
        res.setHeader('Content-Type', 'application/json');
        res.end(readFileSync(join(MODULES, f)));
      });
    },
  };
}

export default defineConfig({
  base: './',
  define: { __BUILD_HASH__: JSON.stringify(buildHash()) },
  plugins: [modules()],
  // The dev pages read the core's module fixtures, which live outside this folder.
  server: { fs: { allow: ['..'] } },
  build: {
    outDir: '../../assets/cache-explorer',
    emptyOutDir: true,
    target: 'es2022',
    assetsInlineLimit: 0,
    cssCodeSplit: false,
    modulePreload: false,
    sourcemap: false,
    rolldownOptions: {
      input: 'src/main.ts',
      output: {
        codeSplitting: false,
        entryFileNames: 'cache-explorer.js',
        assetFileNames: (a) =>
          a.names.some((name) => name.endsWith('.css')) ? 'cache-explorer.css' : '[name][extname]',
      },
    },
  },
});
