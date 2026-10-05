import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import tseslint from 'typescript-eslint';

const BARE_SPECIFIER = {
  regex: '^[^./]',
  message: 'The UI imports its own files and the wasm package only: no runtime dependency.',
};

/** The generated package's runtime exports come through src/wasm.ts; everything else takes its types. */
const GENERATED_PACKAGE = {
  group: ['**/pkg/*'],
  allowTypeImports: true,
  message: 'Import from the wasm package through src/wasm.ts; other files take only its types.',
};

const NUMBERS_AND_JSON_COME_FROM_THE_CORE =
  'Numbers and JSON go through the core (readAddress, readWord, the draft functions, parseModule, Session.fromJson), which keeps 64-bit values exact.';

/** The same names are banned when reached through the global object: `globalThis.parseInt`, `window.JSON.parse`. */
const GLOBAL_OBJECTS = ['globalThis', 'window', 'self'];
/**
 * The rule matches an object by its name, so `globalThis.JSON.parse` and `globalThis.Number.parseInt`
 * (a member of a member) are stopped at `globalThis.JSON` and `globalThis.Number`. Plain
 * `JSON.stringify` and `Number(...)` stay available.
 */
const GLOBAL_NAMES = ['parseInt', 'parseFloat', 'BigInt', 'JSON', 'Number'];

export default defineConfig(
  // wasm-bindgen's glue is generated, and ESLint's flat config never reads .gitignore.
  { ignores: ['pkg/**'] },
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.json', './tsconfig.node.json'],
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },
  { files: ['**/*.js'], extends: [tseslint.configs.disableTypeChecked] },
  {
    files: ['src/**/*.ts'],
    rules: {
      '@typescript-eslint/no-restricted-imports': [
        'error',
        { patterns: [BARE_SPECIFIER, GENERATED_PACKAGE] },
      ],
      'no-restricted-properties': [
        'error',
        ...[
          { object: 'JSON', property: 'parse' },
          { object: 'Number', property: 'parseInt' },
          { object: 'Number', property: 'parseFloat' },
          ...GLOBAL_OBJECTS.flatMap((object) =>
            GLOBAL_NAMES.map((property) => ({ object, property })),
          ),
        ].map((banned) => ({ ...banned, message: NUMBERS_AND_JSON_COME_FROM_THE_CORE })),
      ],
      'no-restricted-globals': [
        'error',
        ...['parseInt', 'parseFloat', 'BigInt'].map((name) => ({
          name,
          message: NUMBERS_AND_JSON_COME_FROM_THE_CORE,
        })),
      ],
    },
  },
  {
    files: ['src/wasm.ts'],
    rules: {
      '@typescript-eslint/no-restricted-imports': ['error', { patterns: [BARE_SPECIFIER] }],
    },
  },
  {
    files: ['test/**/*.ts'],
    rules: {
      '@typescript-eslint/no-restricted-imports': ['error', { patterns: [GENERATED_PACKAGE] }],
    },
  },
);
