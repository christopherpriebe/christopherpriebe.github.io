import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';

const script = join(import.meta.dirname, '..', '..', '..', '..', 'scripts', 'check-wasm-paths.mjs');
const dir = mkdtempSync(join(tmpdir(), 'wasm-paths-'));
afterAll(() => {
  rmSync(dir, { recursive: true, force: true });
});

const run = (content: string) => {
  const file = join(dir, 'x.wasm');
  writeFileSync(file, Buffer.from(content, 'latin1'));
  return spawnSync('node', [script, 'cache-explorer', file], { encoding: 'utf8' });
};

describe('check-wasm-paths', () => {
  it('passes a wasm with only remapped paths', () => {
    expect(
      run('\0/rustc/48a229ce/library/core/src/slice.rs\0/cargo/registry/x.rs\0/src/crates/a.rs')
        .status,
    ).toBe(0);
  });
  it('fails on a home directory in a toolchain path', () => {
    const result = run(
      '\0/Users/someone/.rustup/toolchains/1.98.1/lib/rustlib/src/rust/library/alloc/src/str.rs\0',
    );
    expect(result.status).toBe(1);
    expect(result.stderr).toContain('/Users/someone');
  });
  it('fails on a Linux home directory', () => {
    expect(run('/home/runner/.cargo/registry/x.rs').status).toBe(1);
  });
});
