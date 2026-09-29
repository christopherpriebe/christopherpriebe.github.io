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
        ...['JSON.parse', 'Number.parseInt', 'Number.parseFloat'].map((name) => {
          const [object, property] = name.split('.');
          return { object, property, message: NUMBERS_AND_JSON_COME_FROM_THE_CORE };
        }),
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
