// The time motion takes. Render rebuilds the elements it draws, and a rebuilt element starts its CSS
// animation over; the controller keeps when a flash began, so that the element built in the middle
// of one can carry on from where it had got.

/** How long a configuration group's flash lasts: 400 ms in, a hold, and 400 ms out. */
export const FLASH_MS = 1600;
