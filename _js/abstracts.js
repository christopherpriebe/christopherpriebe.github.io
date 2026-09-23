// Show/hide toggles for publication abstracts. Each button names the
// paragraph it controls in aria-controls; the paragraph starts hidden.
export function initAbstracts() {
  document.querySelectorAll("[data-abstract-toggle]").forEach((button) => {
    const target = document.getElementById(button.getAttribute("aria-controls"));
    if (!target) return;

    button.addEventListener("click", () => {
      const open = target.hidden;
      target.hidden = !open;
      button.setAttribute("aria-expanded", String(open));
    });
  });
}
