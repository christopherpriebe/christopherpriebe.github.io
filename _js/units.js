import { setPressed } from "./dom";

// Metric/imperial switching for the routes page.
//
// Values are stored and compared in metric everywhere — the data file, the
// filter sliders, the sort order. This module only changes how they are
// *displayed*, so switching units can never change which routes match a
// filter. Liquid renders the metric text server-side; this rewrites it.
//
// Markup contract:
//   data-unit-value="<metric number>" data-unit-kind="distance|elevation|pace"
//       -> element text becomes the converted, rounded number
//   data-unit-label="distance|elevation|pace"
//       -> element text becomes the unit string ("km" / "mi", …)

const STORAGE_KEY = "priebe:units";
const KM_PER_MILE = 1.609344;
const M_PER_FOOT = 0.3048;

export const METRIC = "metric";
export const IMPERIAL = "imperial";

const listeners = [];
let current = METRIC;

function readStored() {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    return stored === IMPERIAL || stored === METRIC ? stored : null;
  } catch (error) {
    // Private browsing and blocked storage both throw; the toggle still works
    // for the session, it just will not be remembered.
    return null;
  }
}

function writeStored(units) {
  try {
    window.localStorage.setItem(STORAGE_KEY, units);
  } catch (error) {
    /* not fatal — see readStored */
  }
}

export function distanceUnit() {
  return current === IMPERIAL ? "mi" : "km";
}

export function elevationUnit() {
  return current === IMPERIAL ? "ft" : "m";
}

export function paceUnit() {
  return current === IMPERIAL ? "/mi" : "/km";
}

export function formatDistance(km) {
  const value = current === IMPERIAL ? km / KM_PER_MILE : km;
  // One decimal, but no trailing ".0" on whole numbers.
  return String(Math.round(value * 10) / 10);
}

export function formatElevation(metres) {
  const value = current === IMPERIAL ? metres / M_PER_FOOT : metres;
  return String(Math.round(value));
}

// Pace is carried as seconds per kilometre; a mile takes proportionally longer.
export function formatPace(secondsPerKm) {
  if (!secondsPerKm) return "—";

  const seconds = Math.round(current === IMPERIAL ? secondsPerKm * KM_PER_MILE : secondsPerKm);
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

const FORMATTERS = {
  distance: formatDistance,
  elevation: formatElevation,
  pace: formatPace,
};

const LABELS = {
  distance: distanceUnit,
  elevation: elevationUnit,
  pace: paceUnit,
};

export function applyUnits(root = document) {
  root.querySelectorAll("[data-unit-value]").forEach((el) => {
    const format = FORMATTERS[el.getAttribute("data-unit-kind")];
    if (format) el.textContent = format(Number(el.getAttribute("data-unit-value")));
  });

  root.querySelectorAll("[data-unit-label]").forEach((el) => {
    const label = LABELS[el.getAttribute("data-unit-label")];
    if (label) el.textContent = label();
  });
}

export function onUnitsChange(callback) {
  listeners.push(callback);
}

export function setUnits(units, { persist = true } = {}) {
  current = units === IMPERIAL ? IMPERIAL : METRIC;
  if (persist) writeStored(current);

  document.querySelectorAll("[data-unit-toggle] button").forEach((button) => {
    setPressed(button, button.getAttribute("data-units") === current);
  });

  applyUnits();
  listeners.forEach((callback) => callback(current));
}

export function initUnits() {
  const toggles = document.querySelectorAll("[data-unit-toggle]");
  if (!toggles.length) return;

  toggles.forEach((toggle) => {
    toggle.querySelectorAll("button").forEach((button) => {
      button.addEventListener("click", () => setUnits(button.getAttribute("data-units")));
    });
  });

  // A stored preference wins; otherwise keep what the server rendered.
  setUnits(readStored() || current, { persist: false });
}
