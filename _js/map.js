import L from "leaflet";
import "leaflet/dist/leaflet.css";

// Quick MVP fix for default marker icons when bundling
delete L.Icon.Default.prototype._getIconUrl;
L.Icon.Default.mergeOptions({
  iconRetinaUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-icon-2x.png",
  iconUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-icon.png",
  shadowUrl: "https://unpkg.com/leaflet@1.9.4/dist/images/marker-shadow.png",
});

function escapeHtml(s) {
  return String(s || "").replace(/[&<>"']/g, (m) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"
  }[m]));
}

function buildGenericPopup(item) {
  const title = item.name ? `<strong>${escapeHtml(item.name)}</strong>` : `<strong>Place</strong>`;
  const whereBits = ["neighborhood","city","state","country"].map(k => item[k]).filter(Boolean);
  const where = whereBits.length ? `<div class="muted">${escapeHtml(whereBits.join(", "))}</div>` : "";
  const tldr = item.tldr ? `<div style="margin-top:6px;">${escapeHtml(item.tldr)}</div>` : "";
  return `<div>${title}${where}${tldr}</div>`;
}

export function initMap(config) {
  const { mapId, listId, popupType = "generic", zoom = 2, center = "" } = config;

  const mapEl = document.getElementById(mapId);
  if (!mapEl) return;

  const dataset =
    (window.__MAP_DATA__ && window.__MAP_DATA__[mapId]) ? window.__MAP_DATA__[mapId] : [];

  const map = L.map(mapId, { scrollWheelZoom: true });

  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    maxZoom: 19,
    attribution: "&copy; OpenStreetMap contributors"
  }).addTo(map);

  // No clustering: just a plain layer group
  const markerLayer = L.layerGroup().addTo(map);
  const markerBySlug = {};

  dataset.forEach((d) => {
    const item = d._item || {};
    const popupHtml = buildGenericPopup(item); // later: switch on popupType

    const m = L.marker([d.lat, d.lng]).bindPopup(popupHtml);
    m.addTo(markerLayer);
    markerBySlug[d.slug] = m;
  });

  // Set initial view
  if (center) {
    try {
      const parsed = JSON.parse(center);
      if (Array.isArray(parsed) && parsed.length === 2) map.setView(parsed, zoom);
      else map.setView([20, 0], zoom);
    } catch {
      map.setView([20, 0], zoom);
    }
  } else if (dataset.length) {
    const latlngs = dataset.map(d => [d.lat, d.lng]);
    map.fitBounds(latlngs, { padding: [30, 30] });
  } else {
    map.setView([20, 0], zoom);
  }

  // List click -> open marker
  const list = document.getElementById(listId);
  if (list) {
    list.addEventListener("click", (e) => {
      let el = e.target;
      while (el && el !== document && !el.getAttribute("data-slug")) el = el.parentNode;
      if (!el || !el.getAttribute) return;

      const slug = el.getAttribute("data-slug");
      const m = markerBySlug[slug];
      if (m) {
        const ll = m.getLatLng();
        map.setView(ll, Math.max(map.getZoom(), 14));
        m.openPopup();
      }
    });
  }
}
