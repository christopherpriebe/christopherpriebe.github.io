// Instrument 01: cache address decomposition.
//
// Real cache geometry, not a mock. sets = size / (associativity × block), and
// the address fields fall out of it. This is the part of the cache simulator
// that needs no simulation, so it can ship now and be correct.
//
// Markup lives in _includes/instruments/cache.liquid; colours are classes
// from _sass/_instruments.sass so the palette stays in the stylesheet.

import { setPressed } from "./dom";

const ADDRESS_BITS = 32;

const NUMERIC = new Set(["sizeKB", "assoc", "blockB", "cores"]);

function stateLegend(proto, cores) {
  const invalid = { name: "Invalid", note: "Not present", fill: "swatch--invalid" };

  if (proto === "MSI") {
    return [
      { name: "Modified", note: "Dirty, the only copy", fill: "fill-modified" },
      { name: "Shared", note: "Clean, may be replicated", fill: "fill-shared" },
      invalid,
    ];
  }

  if (proto === "Directory") {
    return [
      { name: "Modified", note: "One writer, tracked at the home node", fill: "fill-modified" },
      { name: "Shared", note: "Readers tracked by presence vector", fill: "fill-shared" },
      invalid,
      {
        name: "Presence vector",
        note: `${cores} ${cores === 1 ? "bit" : "bits"} at the home node`,
        fill: "fill-presence",
      },
    ];
  }

  return [
    { name: "Modified", note: "Dirty, the only copy", fill: "fill-modified" },
    { name: "Owned", note: "Dirty and shared; this core answers", fill: "fill-owned" },
    { name: "Exclusive", note: "Clean, the only copy", fill: "fill-exclusive" },
    { name: "Shared", note: "Clean, may be replicated", fill: "fill-shared" },
    invalid,
  ];
}

function bitsLabel(bits) {
  return `${bits} ${bits === 1 ? "bit" : "bits"}`;
}

function render(root, state) {
  const { sizeKB, assoc, blockB, cores, proto } = state;

  const sets = (sizeKB * 1024) / (assoc * blockB);
  const offsetBits = Math.log2(blockB);
  const indexBits = Math.log2(sets);
  const tagBits = ADDRESS_BITS - indexBits - offsetBits;
  // MOESI's five states need three bits; the others fit in two.
  const stateBits = proto === "MOESI" ? 3 : 2;
  const lines = sets * assoc;
  const overheadPct = (lines * (tagBits + stateBits)) / (sizeKB * 1024 * 8) * 100;
  const totalKB = sizeKB * cores;

  const fields = { tag: tagBits, index: indexBits, offset: offsetBits };
  Object.entries(fields).forEach(([field, bits]) => {
    const segment = root.querySelector(`[data-field="${field}"]`);
    if (segment) segment.style.flexGrow = String(bits);
    const label = root.querySelector(`[data-bits="${field}"]`);
    if (label) label.textContent = bitsLabel(bits);
  });

  const out = {
    sets: sets.toLocaleString("en-US"),
    lines: lines.toLocaleString("en-US"),
    lineBits: `${tagBits} + ${stateBits}`,
    overhead: `${overheadPct.toFixed(1)}%`,
    total: totalKB >= 1024 ? `${totalKB / 1024} MB` : `${totalKB} KB`,
    proto,
  };
  Object.entries(out).forEach(([key, value]) => {
    const element = root.querySelector(`[data-out="${key}"]`);
    if (element) element.textContent = value;
  });

  const legend = root.querySelector("[data-state-legend]");
  if (legend) {
    legend.replaceChildren(...stateLegend(proto, cores).map((state) => {
      const item = document.createElement("div");
      item.className = "state-legend__item";

      const swatch = document.createElement("span");
      swatch.className = `swatch swatch--state ${state.fill}`;

      const text = document.createElement("span");
      const name = document.createElement("b");
      name.textContent = state.name;
      text.append(name, state.note);

      item.append(swatch, text);
      return item;
    }));
  }
}

function initCache(root) {
  const state = {};

  root.querySelectorAll("[data-param]").forEach((group) => {
    const param = group.getAttribute("data-param");
    const buttons = Array.from(group.querySelectorAll("[data-value]"));
    const parse = (button) => {
      const raw = button.getAttribute("data-value");
      return NUMERIC.has(param) ? Number(raw) : raw;
    };

    const initial = buttons.find((button) => button.classList.contains("is-active")) || buttons[0];
    state[param] = parse(initial);

    buttons.forEach((button) => {
      button.addEventListener("click", () => {
        buttons.forEach((other) => setPressed(other, other === button));
        state[param] = parse(button);
        render(root, state);
      });
    });
  });

  render(root, state);
}

export function initInstruments() {
  document.querySelectorAll('[data-instrument="cache"]').forEach(initCache);
}
