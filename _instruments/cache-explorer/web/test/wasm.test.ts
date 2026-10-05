import { beforeAll, describe, expect, it } from 'vitest';
import { buildInfo, Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

describe('the shipped wasm under Vitest', () => {
  beforeAll(loadWasm);

  it('reports the crates build', () => {
    const info = buildInfo();
    expect(info.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(info.schemaVersion).toBeGreaterThan(0);
  });

  it('opens the first visit on the direct-mapped cache with pattern memory', () => {
    const session = Session.firstVisit();
    try {
      expect(session.state().config).toMatchObject({
        addressBits: 12,
        cache: { sizeBytes: 128, lineBytes: 16, associativity: 1 },
        memoryInit: 'pattern',
      });
    } finally {
      session.free();
    }
  });

  it('saves a run and opens it again through the glue', () => {
    const first = Session.firstVisit();
    let json: string;
    try {
      json = first.toJson();
    } finally {
      first.free();
    }
    const second = Session.fromJson(json);
    try {
      expect(second.toJson()).toBe(json);
    } finally {
      second.free();
    }
  });
});
