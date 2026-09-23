import L from "leaflet";
import { setPressed } from "./dom";

// Tile layers shared by the F&B marker map and the Efforts map, plus
// the wiring for the Street/Terrain switcher both pages render.

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
