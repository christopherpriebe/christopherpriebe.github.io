import L from "leaflet";
import { setPressed } from "./dom";
import "leaflet/dist/leaflet.css";
import { attachBasemaps } from "./basemaps";
import { formatDistance, formatElevation, distanceUnit, elevationUnit, onUnitsChange } from "./units";

// Running/hiking route map. The marker map in map.js plots points; this plots
// tracks. Line colour, dash pattern and weight are left to the stylesheet
// (_sass/_running_routes.sass) — Leaflet writes `stroke` as a presentation
// attribute, which a stylesheet rule overrides — so the skin stays the one
// place colours are defined.

const DEFAULT_CENTER = [32.9, -117.1];

function parseGpx(xml) {
  const doc = new DOMParser().parseFromString(xml, "application/xml");
  if (doc.getElementsByTagName("parsererror").length) return [];

  const points = doc.getElementsByTagName("trkpt");
  const coordinates = [];
  for (let i = 0; i < points.length; i += 1) {
    const lat = Number(points[i].getAttribute("lat"));
    const lng = Number(points[i].getAttribute("lon"));
    if (Number.isFinite(lat) && Number.isFinite(lng)) coordinates.push([lat, lng]);
  }
  return coordinates;
}

function endpointIcon(isFinish) {
  return L.divIcon({
    className: "",
    html: `<span class="route-endpoint${isFinish ? " route-endpoint--finish" : ""}"></span>`,
    iconSize: [11, 11],
    iconAnchor: [5.5, 5.5],
  });
}

export function initRouteMap(config) {
  const mapElement = document.getElementById(config.mapId);
  if (!mapElement) return;

  const entries = (window.__ROUTE_MAP_DATA__ || {})[config.mapId] || [];
  const center = Array.isArray(config.center) && config.center.length === 2
    ? config.center
    : DEFAULT_CENTER;

  const map = L.map(mapElement, { zoomControl: false }).setView(center, config.zoom || 11);
  L.control.zoom({ position: "bottomright" }).addTo(map);

  attachBasemaps(map, mapElement.parentNode);

  const listElement = document.getElementById(config.listId);
  const rows = new Map();
  if (listElement) {
    listElement.querySelectorAll(".route-item").forEach((row) => {
      rows.set(row.getAttribute("data-slug"), row);
    });
  }

  // slug -> { line, markers, visible }
  const tracks = new Map();
  let activeSlug = null;

  function applyLineState() {
    tracks.forEach((track, slug) => {
      const element = track.line.getElement();
      if (!element) return;
      element.classList.toggle("route-line--active", slug === activeSlug);
      element.classList.toggle("route-line--muted", activeSlug !== null && slug !== activeSlug);
    });
  }

  function selectRoute(slug) {
    activeSlug = activeSlug === slug ? null : slug;

    rows.forEach((row, rowSlug) => {
      row.classList.toggle("is-active", rowSlug === activeSlug);
    });
    applyLineState();

    const track = tracks.get(activeSlug);
    if (track) {
      map.fitBounds(track.line.getBounds(), { padding: [40, 40] });
    } else if (!activeSlug) {
      fitVisible();
    }
  }

  function fitVisible() {
    const bounds = L.latLngBounds([]);
    tracks.forEach((track) => {
      if (track.visible) bounds.extend(track.line.getBounds());
    });
    if (bounds.isValid()) map.fitBounds(bounds, { padding: [30, 30] });
  }

  function addTrack(entry, coordinates) {
    if (!coordinates || coordinates.length < 2) return;

    const isHike = entry.type === "hike";
    const line = L.polyline(coordinates, {
      className: `route-line route-line--${isHike ? "hike" : "run"}`,
      interactive: true,
    });
    line.bindTooltip(entry.name, { sticky: true, direction: "top" });
    line.on("click", () => selectRoute(entry.slug));
    line.addTo(map);

    const markers = [
      L.marker(coordinates[0], { icon: endpointIcon(false), interactive: false }),
      L.marker(coordinates[coordinates.length - 1], { icon: endpointIcon(true), interactive: false }),
    ];
    markers.forEach((marker) => marker.addTo(map));

    tracks.set(entry.slug, { line, markers, visible: true });

    const row = rows.get(entry.slug);
    if (row) row.classList.add("has-track");
  }

  const loaded = entries.map((entry) => {
    if (Array.isArray(entry.polyline) && entry.polyline.length) {
      addTrack(entry, entry.polyline);
      return Promise.resolve();
    }
    if (!entry.gpx) return Promise.resolve();

    return fetch(entry.gpx)
      .then((response) => {
        if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
        return response.text();
      })
      .then((xml) => addTrack(entry, parseGpx(xml)))
      .catch((error) => {
        // A missing or malformed GPX should cost one route, not the whole map.
        console.error(`Could not load track for "${entry.slug}" from ${entry.gpx}:`, error);
      });
  });

  Promise.all(loaded).then(() => {
    applyLineState();
    fitVisible();
  });

  rows.forEach((row, slug) => {
    row.addEventListener("click", (event) => {
      event.preventDefault();
      selectRoute(slug);
    });
  });

  const searchInput = document.getElementById("routes-filter-q");
  const clearButton = document.getElementById("routes-filter-clear");
  const emptyMessage = listElement ? listElement.querySelector("[data-routes-empty]") : null;
  const countLabel = document.querySelector("[data-routes-count]");
  const chipGroups = Array.from(document.querySelectorAll("[data-routes-filter]"));
  const ranges = {
    distance: document.querySelector('[data-routes-range="distance"]'),
    elevation: document.querySelector('[data-routes-range="elevation"]'),
  };
  const rangeLabels = {
    distance: document.querySelector("[data-routes-distance-label]"),
    elevation: document.querySelector("[data-routes-elevation-label]"),
  };

  function selectedValues(name) {
    const group = chipGroups.find((element) => element.getAttribute("data-routes-filter") === name);
    if (!group) return [];
    return Array.from(group.querySelectorAll(".chip-btn.is-active"))
      .map((button) => button.getAttribute("data-filter-value"));
  }

  function applyFilters() {
    const query = (searchInput ? searchInput.value : "").trim().toLowerCase();
    const types = selectedValues("type");
    const surfaces = selectedValues("surface");
    const maxDistance = ranges.distance ? Number(ranges.distance.value) : Infinity;
    const maxElevation = ranges.elevation ? Number(ranges.elevation.value) : Infinity;

    let shown = 0;

    rows.forEach((row, slug) => {
      const type = row.getAttribute("data-type");
      const surface = row.getAttribute("data-surface");
      const haystack = [
        row.getAttribute("data-name"),
        row.getAttribute("data-region"),
        type,
        surface,
      ].join(" ").toLowerCase();

      const visible = (!query || haystack.includes(query))
        && (!types.length || types.includes(type))
        && (!surfaces.length || surfaces.includes(surface))
        && Number(row.getAttribute("data-distance")) <= maxDistance
        && Number(row.getAttribute("data-elevation")) <= maxElevation;

      row.style.display = visible ? "" : "none";
      if (visible) shown += 1;

      const track = tracks.get(slug);
      if (!track) return;

      track.visible = visible;
      if (visible && !map.hasLayer(track.line)) {
        track.line.addTo(map);
        track.markers.forEach((marker) => marker.addTo(map));
        applyLineState();
      } else if (!visible && map.hasLayer(track.line)) {
        map.removeLayer(track.line);
        track.markers.forEach((marker) => map.removeLayer(marker));
      }
    });

    // A filtered-out route should not stay selected.
    if (activeSlug) {
      const activeRow = rows.get(activeSlug);
      if (activeRow && activeRow.style.display === "none") {
        activeRow.classList.remove("is-active");
        activeSlug = null;
        applyLineState();
      }
    }

    if (countLabel) countLabel.textContent = shown;
    if (emptyMessage) emptyMessage.style.display = shown ? "none" : "block";
  }

  if (searchInput) searchInput.addEventListener("input", applyFilters);

  chipGroups.forEach((group) => {
    group.querySelectorAll(".chip-btn").forEach((button) => {
      button.addEventListener("click", () => {
        setPressed(button, !button.classList.contains("is-active"));
        applyFilters();
      });
    });
  });

  // Sliders stay in metric; only their labels convert. Keeping the comparison
  // in one unit means switching units cannot change what the filter matches.
  function renderRangeLabel(name) {
    const input = ranges[name];
    const label = rangeLabels[name];
    if (!input || !label) return;

    const value = Number(input.value);
    label.textContent = name === "distance"
      ? `${formatDistance(value)} ${distanceUnit()}`
      : `${formatElevation(value)} ${elevationUnit()}`;
  }

  Object.keys(ranges).forEach((name) => {
    const input = ranges[name];
    if (!input) return;
    input.addEventListener("input", () => {
      renderRangeLabel(name);
      applyFilters();
    });
  });

  onUnitsChange(() => Object.keys(ranges).forEach(renderRangeLabel));

  if (clearButton) {
    clearButton.addEventListener("click", () => {
      if (searchInput) searchInput.value = "";
      chipGroups.forEach((group) => {
        group.querySelectorAll(".chip-btn").forEach((button) => {
          setPressed(button, false);
        });
      });
      Object.keys(ranges).forEach((name) => {
        const input = ranges[name];
        if (!input) return;
        input.value = input.max;
        renderRangeLabel(name);
      });
      applyFilters();
      fitVisible();
    });
  }
}
