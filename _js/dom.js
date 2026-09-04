// Small DOM helpers shared across the interactive modules.

// Marks one control in a group as chosen, pairing the visual class with the
// matching ARIA state so the two cannot drift apart. Used by the basemap,
// unit, blog/project filter and route chip toggles. The F&B filter chips in
// map.js still set both by hand and have not been migrated.
export function setPressed(element, active) {
  element.classList.toggle("is-active", active);
  element.setAttribute("aria-pressed", String(active));
}
