// Small DOM helpers shared across the interactive modules.

// Marks one control in a group as chosen, pairing the visual class with the
// matching ARIA state so the two cannot drift apart. Used by the basemap,
// unit, blog/project filter, route chip and instrument toggles.
// TODO: Move the F&B filter chips in map.js onto this; they still set both by
// hand.
export function setPressed(element, active) {
  element.classList.toggle("is-active", active);
  element.setAttribute("aria-pressed", String(active));
}
