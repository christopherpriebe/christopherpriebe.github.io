// The states the development page can open the tool in, each built the way a student would reach
// it, from calls to the facade: a session, then the accesses and steps that lead to the state. The
// tests build the same states. Nothing here is in the build.

import type { Config, ProgramSource } from '../pkg/cache_explorer';
import { SAMPLE_PROGRAM } from '../src/ui-state';
import { Session } from '../src/wasm';

export const SAMPLE: ProgramSource = { source: SAMPLE_PROGRAM, format: 'asm' };

/** Frame 3b's program: six instructions that each make an access. */
export const FRAME_3B: ProgramSource = {
  source: [
    '  st  [0x0B0], #5',
    '  ld  r1, [0x0A4]',
    '  st  [0x1A4], #7',
    '  ld  r2, [0x0A8]',
    '  ld  r3, [0x2A4]',
    '  ld  r4, [0x1A4]',
  ].join('\n'),
  format: 'asm',
};

/** A configuration with the design's 12-bit addresses, 4-byte words and pattern memory. */
export function config(
  size: number,
  line: number,
  ways: number,
  extra: Partial<Config> = {},
): Config {
  return {
    addressBits: 12,
    wordBytes: 4,
    cache: {
      sizeBytes: size,
      lineBytes: line,
      associativity: ways,
      replacement: 'lru',
      writePolicy: 'writeBack',
      allocatePolicy: 'writeAllocate',
    },
    seed: 12345,
    memoryInit: 'pattern',
    ...extra,
  };
}

/** Loads from `addresses`, one access each, through the facade's own op. */
export function loads(session: Session, addresses: readonly number[]): Session {
  for (const addr of addresses) session.applyOp({ op: 'issue', kind: 'read', addr });
  return session;
}

export const SCENARIOS: Record<string, () => Session> = {
  /** The first visit: nothing issued, `0x0A4` typed. */
  'first-visit': () => Session.firstVisit(),

  /** Frame 3a: direct-mapped, after three loads; the third evicts the first from set 2. */
  'frame-3a': () => {
    const session = Session.firstVisit();
    for (let i = 0; i < 3; i++) session.stepAccess(SAMPLE);
    return session;
  },

  /** Frame 3b: 2-way LRU, write-back, timed, paused on the write-back of a dirty victim. */
  'frame-3b': () => {
    const session = Session.withConfig(
      config(128, 16, 2, { timing: { hitCycles: 1, missCycles: 20 }, advanced: true }),
    );
    for (let i = 0; i < 4; i++) session.stepAccess(FRAME_3B);
    session.stepEvent(FRAME_3B);
    for (let i = 0; i < 40 && session.state().event?.kind !== 'writeBack'; i++) {
      session.stepEvent(FRAME_3B);
    }
    return session;
  },

  /** Two sets of four ways: the most ways a set shows side by side. */
  'four-way': () => loads(Session.withConfig(config(128, 16, 4)), [0x0a4, 0x1a4, 0x2a4, 0x0a8]),

  /** A fully associative cache of eight lines, whose ways wrap, with a hit on the way. */
  'wrapped-fully-associative': () =>
    loads(Session.withConfig(config(128, 16, 8)), [0x0a4, 0x1a4, 0x2a4, 0x0a8]),

  /** Four sets of eight ways, timed, write-back, mid-access on a miss that evicts. */
  'wrapped-four-sets': () => {
    const session = Session.withConfig(
      config(512, 16, 8, { timing: { hitCycles: 1, missCycles: 20 }, advanced: true }),
    );
    loads(session, [0x0a4, 0x1a4, 0x2a4, 0x3a4, 0x4a4, 0x5a4, 0x6a4, 0x7a4, 0x0a4, 0x8a4]);
    session.applyOp({ op: 'issue', kind: 'write', addr: 0x0a8, value: 9n });
    return session;
  },

  /** Forty lines touched, for the memory table's scrolling. */
  'long-memory': () =>
    loads(
      Session.withConfig(config(512, 16, 1)),
      Array.from({ length: 40 }, (_, i) => i * 16),
    ),
};
