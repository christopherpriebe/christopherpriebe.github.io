// Instrument 01: cache address decomposition.
//
// Real cache geometry, not a mock. sets = size / (associativity × block), and
// the address fields fall out of it. This is the part of the cache simulator
// that needs no simulation, so it can be exact.
//
// Markup lives in _includes/instruments/cache.liquid; colours are classes
// from _sass/_instruments.sass so the palette stays in the stylesheet.

import { setPressed } from "./dom";

const ADDRESS_BITS = 32;

const NUMERIC = new Set(["sizeKB", "assoc", "blockB", "cores"]);

// State bits per line for each protocol the legend knows. MOESI's five states
// need three bits; the others fit in two.
const STATE_BITS = { MSI: 2, MOESI: 3, Directory: 2 };

function isPowerOfTwo(value) {
  return Number.isInteger(value) && value > 0 && (value & (value - 1)) === 0;
}

// Why a combination of controls has no real geometry, or null when it does.
// The data file can offer, say, a 48K cache or a 1K one with 16 ways of 128B
// blocks; the readout should say so rather than print fractional bits.
function geometryProblem({ sizeKB, assoc, blockB, cores, proto }) {
  const missing = Object.entries({ sizeKB, assoc, blockB, cores, proto })
    .filter(([, value]) => value === undefined || Number.isNaN(value))
    .map(([name]) => name);
  if (missing.length) return `missing ${missing.join(", ")}`;
  if (![sizeKB, assoc, blockB].every(isPowerOfTwo)) return "size, associativity and block must be powers of two";
  if (!Number.isInteger(cores) || cores < 1) return "cores must be a whole number";
  if (!(proto in STATE_BITS)) return `unknown protocol ${proto}`;
  if ((sizeKB * 1024) / (assoc * blockB) < 1) return "fewer than one set";
  return null;
}

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

function renderInvalid(root, problem) {
  console.warn(`Cache instrument: ${problem}.`);
  root.querySelectorAll("[data-field]").forEach((segment) => {
    segment.style.flexGrow = "1";
  });
  root.querySelectorAll("[data-bits], [data-out]").forEach((element) => {
    element.textContent = "\u2014";
  });
  const status = root.querySelector("[data-status]");
  if (status) status.textContent = "Invalid configuration";
  const legend = root.querySelector("[data-state-legend]");
  if (legend) legend.replaceChildren();
}

function render(root, state) {
  const problem = geometryProblem(state);
  if (problem) {
    renderInvalid(root, problem);
    return;
  }

  const status = root.querySelector("[data-status]");
  if (status) status.textContent = "Working";

  const { sizeKB, assoc, blockB, cores, proto } = state;

  const sets = (sizeKB * 1024) / (assoc * blockB);
  const offsetBits = Math.log2(blockB);
  const indexBits = Math.log2(sets);
  const tagBits = ADDRESS_BITS - indexBits - offsetBits;
  const stateBits = STATE_BITS[proto];
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

    if (!buttons.length) {
      console.warn(`Cache instrument: control "${param}" has no options.`);
      return;
    }

    // With no default matching an option, start on the first and show it
    // pressed, so the readout and the buttons agree.
    let initial = buttons.find((button) => button.classList.contains("is-active"));
    if (!initial) {
      initial = buttons[0];
      setPressed(initial, true);
    }
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
