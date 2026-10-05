// The time motion takes. Render rebuilds the elements it draws, and a rebuilt element starts its CSS
// animation over; the controller keeps when a flash began, so that the element built in the middle
// of one can carry on from where it had got.

/** How long a configuration group's flash lasts: 400 ms in, a hold, and 400 ms out. */
export const FLASH_MS = 1600;

/** How long the ring round a changed cell takes to fade, and a packet to cross its lane. */
export const PULSE_MS = 900;
export const PACKET_MS = 900;
/** How far a packet crosses its lane, in the lane's own pixels; the keyframes say the same. */
export const PACKET_TRAVEL_PX = 58;

/**
 * What a render plays: the pulse and the packet run once, when the cursor has moved since the
 * previous render, and never on a render that typing or a panel caused. Two identical keyframes,
 * `a` and `b`, alternate by the position's parity, so a step straight after another restarts the
 * animation instead of leaving the earlier one running.
 */
export type Beat = 'a' | 'b';

/** The beat of this render, or null when the cursor has not moved since the last. */
export function beatOf(pos: number, lastPos: number): Beat | null {
  if (pos === lastPos) return null;
  return pos % 2 === 0 ? 'b' : 'a';
}

/** How long the Run button waits between one micro-event and the next. */
export const RUN_MS = 450;

/** How long a page may take to open before the loading state shows: most finish first (12.8). */
export const LOADING_DELAY_MS = 200;
