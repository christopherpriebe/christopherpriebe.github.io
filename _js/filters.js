import { setPressed } from "./dom";

// Chip filtering for the projects list and the blog archive.
//
// Both pages mark their items with `data-post`/`data-project` and a
// `data-categories` list, so one implementation covers them.

const ITEM_SELECTOR = "[data-project], [data-post]";

function categoriesOf(item) {
  return (item.getAttribute("data-categories") || "")
    .split("|")
    .map((value) => value.trim())
    .filter(Boolean);
}

function wire(filterRow) {
  // The row sits above the items it controls, so scope to a shared ancestor.
  const scope = filterRow.closest("[data-filter-root]") || document;
  const items = Array.from(scope.querySelectorAll(ITEM_SELECTOR));
  if (!items.length) return;

  const chips = Array.from(filterRow.querySelectorAll(".chip-btn"));
  const countLabel = filterRow.querySelector("[data-filter-count]");
  const empty = scope.querySelector("[data-filter-empty]");

  function apply(selected) {
    let shown = 0;

    items.forEach((item) => {
      const visible = !selected || categoriesOf(item).includes(selected);
      item.classList.toggle("is-hidden", !visible);
      if (visible) shown += 1;
    });

    if (countLabel) countLabel.textContent = shown;
    if (empty) empty.style.display = shown ? "none" : "block";
  }

  chips.forEach((chip) => {
    chip.addEventListener("click", () => {
      const selected = chip.getAttribute("data-filter-value") || "";

      chips.forEach((other) => {
        setPressed(other, other === chip);
      });

      apply(selected);
    });
  });
}

export function initFilters() {
  document.querySelectorAll("[data-filter-scope]").forEach(wire);
}
