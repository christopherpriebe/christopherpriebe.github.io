// Sizes the script counts in and the stylesheet draws to, which have to be the same number.

/**
 * The height of a row of the memory table with the one-pixel gap above it: 28px of line and 1px
 * of margin. The blank rows that stand in for the rows that are not built are this many pixels a
 * row, and a scroll position is divided by it to find the row it is at.
 */
export const MEMORY_ROW_PX = 29;

/**
 * What the log's blank rows count an entry as: a header, a changed line, and the rules between,
 * which is what a past entry is. Entries differ in height (the current one shows its events, a
 * future one has no changed line), so this is an estimate, and the log keeps the row at the top of
 * the scroller where it is when it draws again.
 */
export const LOG_ROW_PX = 67;

/**
 * The timeline's ticks: the narrowest a tick is drawn, the gap between two, the height of the
 * strip (which is a tick that starts an entry) and of a tick that does not.
 */
export const TICK_PX = 2;
export const TICK_GAP_PX = 2;
export const TICKS_HEIGHT_PX = 18;
export const TICK_SHORT_PX = 9;
