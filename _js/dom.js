// Small DOM helpers shared across the interactive modules.

// Marks one control in a group as chosen. Every toggle on the site pairs a
// visual class with the matching ARIA state, so they are set together here
// rather than being kept in step by hand in five different files.
export function setPressed(element, active) {
  element.classList.toggle("is-active", active);
  element.setAttribute("aria-pressed", String(active));
}
