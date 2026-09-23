import L from "leaflet";
import { setPressed } from "./dom";

// Tile layers shared by the F&B marker map and the Efforts map, plus
// the wiring for the Street/Terrain switcher both pages render, and the
// reading of the `center` both includes pass through.

export const BASEMAPS = {
  osm: {
    url: "https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png",
    attribution: "&copy; OpenStreetMap contributors",
    subdomains: "abc",
    maxZoom: 19,
  },
  topo: {
    url: "https://{s}.tile.opentopomap.org/{z}/{x}/{y}.png",
    attribution: "&copy; OpenStreetMap contributors, SRTM | &copy; OpenTopoMap",
    subdomains: "abc",
    maxZoom: 17,
  },
};

export const DEFAULT_BASEMAP = "osm";

function createLayer(name) {
  const basemap = BASEMAPS[name] || BASEMAPS[DEFAULT_BASEMAP];
  return L.tileLayer(basemap.url, basemap);
}

// Adds the default tile layer to `map` and, if a toggle is present in
// `scope`, wires its buttons to swap layers. Returns the active layer.
export function attachBasemaps(map, scope) {
  let layer = createLayer(DEFAULT_BASEMAP).addTo(map);

  const toggle = scope ? scope.querySelector("[data-basemap-toggle]") : null;
  if (!toggle) return layer;

  toggle.querySelectorAll("button").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.getAttribute("data-basemap");
      if (!BASEMAPS[name]) return;

      toggle.querySelectorAll("button").forEach((other) => {
        setPressed(other, other === button);
      });

      map.removeLayer(layer);
      layer = createLayer(name).addTo(map);
    });
  });

  return layer;
}

// A map include's `center`: a [lat, lng] pair, or that pair as JSON text,
// which is how a Liquid include passes it. Anything else is logged and
// ignored, and the caller falls back to its default view.
export function parseCenter(center, mapId) {
  if (center === undefined || center === null || center === "") return null;

  let value = center;
  if (typeof value === "string") {
    try {
      value = JSON.parse(value);
    } catch {
      value = null;
    }
  }

  if (Array.isArray(value) && value.length === 2 && value.every(Number.isFinite)) return value;

  console.warn(`Map "${mapId}": ignoring center ${JSON.stringify(center)}; expected [lat, lng].`);
  return null;
}
