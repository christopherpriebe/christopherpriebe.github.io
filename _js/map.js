import L from "leaflet";
import "leaflet/dist/leaflet.css";

function escapeHtml(s) {
  return String(s || "").replace(/[&<>"']/g, (m) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  }[m]));
}

function getTemplateHtml(templateId) {
  if (!templateId) return "";
  const tpl = document.getElementById(templateId);
  if (!tpl) return "";
  return (tpl.innerHTML || "").trim();
}

function renderTemplate(html, ctx) {
  if (!html) return "";
  return html.replace(/\{\{\s*([a-zA-Z0-9_]+)\s*\}\}/g, (_, key) => {
    return (ctx[key] != null) ? String(ctx[key]) : "";
  });
}

function buildLinksHtml(item) {
  const links = [];
  if (item.map_url) links.push(`<a href="${escapeHtml(item.map_url)}" target="_blank" rel="noopener">Map</a>`);
  if (item.website) links.push(`<a href="${escapeHtml(item.website)}" target="_blank" rel="noopener">Website</a>`);
  if (item.reservation_url) links.push(`<a href="${escapeHtml(item.reservation_url)}" target="_blank" rel="noopener">Reservations</a>`);
  return links.length ? links.join(" · ") : "";
}

export const CUISINES = {
  italian: { label: "Italian", emoji: "🇮🇹" },
  pizza: { label: "Pizza", emoji: "🍕" },
  barbecue: { label: "BBQ", emoji: "🍖" },
  mexican: { label: "Mexican", emoji: "🇲🇽" },
  tacos: { label: "Tacos", emoji: "🌮" },
  japanese: { label: "Japanese", emoji: "🇯🇵" },
  sushi: { label: "Sushi", emoji: "🍣" },
  thai: { label: "Thai", emoji: "🇹🇭" },
  chinese: { label: "Chinese", emoji: "🇨🇳" },
  korean: { label: "Korean", emoji: "🇰🇷" },
  vietnamese: { label: "Vietnamese", emoji: "🇻🇳" },
  indian: { label: "Indian", emoji: "🇮🇳" },
  french: { label: "French", emoji: "🇫🇷" },
  seafood: { label: "Seafood", emoji: "🦞" },
  gelato: { label: "Gelato", emoji: "🍨" },
  bakery: { label: "Bakery", emoji: "🥖" },
  cafe: { label: "Cafe", emoji: "☕" },
};

export function getCuisineDisplay(key) {
  const k = String(key || "").trim();
  const entry = CUISINES[k];

  if (entry) return entry;

  const label = k
    ? (k.charAt(0).toUpperCase() + k.slice(1))
    : "Unknown";

  return { label, emoji: "🍽️" };
}

export function getCuisinesHtml(keys) {
  if (!Array.isArray(keys) || !keys.length) return "";
  const parts = keys.map((k) => {
    const d = getCuisineDisplay(k);
    return `${d.emoji} ${escapeHtml(d.label)}`;
  });
  return `<div class="muted" style="margin-top:4px;">${parts.join(" · ")}</div>`;
}

function getJourneyClass(journeyRating) {
  const n = parseInt(journeyRating, 10) || 0;
  return `jr-${Math.max(0, Math.min(3, n))}`;
}

function makeJourneyIcon(journeyRating) {
  return L.divIcon({
    className: "",
    html: `<div class="jr-marker ${getJourneyClass(journeyRating)}"></div>`,
    iconSize: [18, 18],
    iconAnchor: [9, 9],
    popupAnchor: [0, -10],
  });
}

function getJourneySymbol(journeyRating) {
  const n = parseInt(journeyRating, 10) || 0;
  if (n === 3) return '<i class="fa-solid fa-circle jr-marker jr-3"></i>';
  if (n === 2) return '<i class="fa-regular fa-circle-dot jr-marker jr-2"></i>';
  if (n === 1) return '<i class="fa-regular fa-circle jr-marker jr-1"></i>';
  return '<i class="fa-regular fa-circle jr-marker jr-0"></i>';
}

function getPriceMeter(priceRating) {
  const n = Math.max(0, Math.min(5, parseInt(priceRating, 10) || 0));
  if (!n) return "";

  let out = "Price: ";
  for (let i = 1; i <= 5; i++) {
    out += (i <= n)
      ? '<i class="fa-solid fa-square" style="margin-right:2px;"></i>'
      : '<i class="fa-regular fa-square" style="margin-right:2px;"></i>';
  }
  return out;
}

function normalizeYears(years) {
  if (!Array.isArray(years)) return [];
  return years
    .map((y) => parseInt(y, 10))
    .filter((y) => Number.isFinite(y))
    .sort((a, b) => a - b);
}

function compressYearRanges(yearsSorted) {
  const ranges = [];
  if (!yearsSorted.length) return ranges;

  let start = yearsSorted[0];
  let prev = yearsSorted[0];

  for (let i = 1; i < yearsSorted.length; i++) {
    const y = yearsSorted[i];
    if (y === prev + 1) {
      prev = y;
      continue;
    }
    ranges.push([start, prev]);
    start = y;
    prev = y;
  }
  ranges.push([start, prev]);
  return ranges;
}

function formatYearRanges(years) {
  const ys = normalizeYears(years);
  if (!ys.length) return { count: 0, text: "" };

  const ranges = compressYearRanges(ys);
  const parts = ranges.map(([a, b]) => (a === b ? `${a}` : `${a}\u2013${b}`));
  return { count: ys.length, text: parts.join(", ") };
}

function getMichelinAwards(m) {
  if (!m) return "";

  const bits = [];

  const stars = Math.max(0, Math.min(3, parseInt(m.stars, 10) || 0));
  if (stars) {
    const starIcons = Array.from({ length: stars })
      .map(() => '<i class="fa-solid fa-asterisk" style="color:#BD2333;"></i>')
      .join("");
    bits.push(`<div>${starIcons}</div>`);
  }

  const lines = [];

  const y1 = formatYearRanges(m.years_of_1_star);
  if (y1.count) {
    lines.push(
      `<li><span style="color:#BD2333;">✱</span> 1-star: <strong>${y1.count}</strong> year${y1.count === 1 ? "" : "s"} (${escapeHtml(y1.text)})</li>`
    );
  }

  const y2 = formatYearRanges(m.years_of_2_stars);
  if (y2.count) {
    lines.push(
      `<li><span style="color:#BD2333;">✱✱</span> 2-star: <strong>${y2.count}</strong> year${y2.count === 1 ? "" : "s"} (${escapeHtml(y2.text)})</li>`
    );
  }

  const y3 = formatYearRanges(m.years_of_3_stars);
  if (y3.count) {
    lines.push(
      `<li><span style="color:#BD2333;">✱✱✱</span> 3-star: <strong>${y3.count}</strong> year${y3.count === 1 ? "" : "s"} (${escapeHtml(y3.text)})</li>`
    );
  }

  const yBib = formatYearRanges(m.years_of_bib);
  if (yBib.count) {
    lines.push(
      `<li><span class="val-badge" style="color:#BD2333;">Bib</span> <strong>${yBib.count}</strong> year${yBib.count === 1 ? "" : "s"} (${escapeHtml(yBib.text)})</li>`
    );
  }

  const yGreen = formatYearRanges(m.years_of_green);
  if (yGreen.count) {
    lines.push(
      `<li><span class="val-badge" style="color:#84BD00;">Green</span> <strong>${yGreen.count}</strong> year${yGreen.count === 1 ? "" : "s"} (${escapeHtml(yGreen.text)})</li>`
    );
  }

  if (m.bib) bits.push('<div style="margin-top:4px;"><span class="val-badge" style="color:#BD2333;">Bib</span></div>');
  if (m.green) bits.push('<div style="margin-top:4px;"><span class="val-badge" style="color:#84BD00;">Green</span></div>');

  if (lines.length) {
    bits.push(`
      <div style="margin-top:6px;">
        <div class="muted"><strong>Michelin history</strong></div>
        <ul class="list-unstyled" style="margin:4px 0 0 0;">
          ${lines.join("")}
        </ul>
      </div>
    `);
  }

  return bits.length ? `<div style="margin-top:6px;">${bits.join("")}</div>` : "";
}

function awardsList(a) {
  let out = "";

  if (a && a.michelin) out += getMichelinAwards(a.michelin);

  if (a && Array.isArray(a.other) && a.other.length) {
    const items = a.other
      .map((s) => String(s || "").trim())
      .filter(Boolean)
      .map((s) => `<li>${escapeHtml(s)}</li>`)
      .join("");

    out += `
      <div style="margin-top:6px;">
        <div class="muted"><strong>Awards</strong></div>
        <ul class="list-unstyled" style="margin:4px 0 0 0;">
          ${items}
        </ul>
      </div>
    `;
  }

  return out;
}

/**
 * ============================================================================
 * Filtering (list-driven)
 * - Assumes your list items have data-* attributes (data-name, data-journey, etc.)
 * - Assumes filter controls exist with the IDs below (or they’ll be ignored)
 * ============================================================================
 */

function getListItems(listId) {
  return Array.prototype.slice.call(document.querySelectorAll(`#${listId} .map-card`));
}

function applyFilters(config, markerLayer, markerBySlug) {
  const qEl = document.getElementById("filter-q");
  const journeyEl = document.getElementById("filter-journey");
  const priceEl = document.getElementById("filter-price");
  const cuisineEl = document.getElementById("filter-cuisine");
  const valueEl = document.getElementById("filter-value");

  const q = (qEl ? qEl.value : "").trim().toLowerCase();
  const journey = journeyEl ? journeyEl.value : "";
  const price = priceEl ? priceEl.value : "";
  const cuisine = cuisineEl ? cuisineEl.value : "";
  const valueOnly = !!(valueEl && valueEl.checked);

  const items = getListItems(config.listId);
  const allowed = [];

  items.forEach((el) => {
    const name = (el.getAttribute("data-name") || "").toLowerCase();
    const city = (el.getAttribute("data-city") || "").toLowerCase();
    const neighborhood = (el.getAttribute("data-neighborhood") || "").toLowerCase();
    const cuisines = (el.getAttribute("data-cuisines") || "").toLowerCase();

    const elJourney = el.getAttribute("data-journey") || "0";
    const elPrice = el.getAttribute("data-price") || "";
    const elValue = (el.getAttribute("data-value") || "0") === "1";
    const slug = el.getAttribute("data-slug");

    let ok = true;

    if (journey && elJourney !== journey) ok = false;
    if (price && elPrice !== price) ok = false;
    if (valueOnly && !elValue) ok = false;

    if (cuisine) {
      const keys = cuisines.split(/\s+/).filter(Boolean);
      if (keys.indexOf(cuisine) === -1) ok = false;
    }

    if (q) {
      const hay = `${name} ${city} ${neighborhood} ${cuisines}`;
      if (hay.indexOf(q) === -1) ok = false;
    }

    el.style.display = ok ? "" : "none";
    if (ok && slug) allowed.push(slug);
  });

  markerLayer.clearLayers();
  allowed.forEach((slug) => {
    const m = markerBySlug[slug];
    if (m) m.addTo(markerLayer);
  });
}

function wireFilters(config, markerLayer, markerBySlug) {
  const ids = ["filter-q", "filter-journey", "filter-price", "filter-cuisine", "filter-value"];
  ids.forEach((id) => {
    const el = document.getElementById(id);
    if (!el) return;

    const evt = (id === "filter-q") ? "input" : "change";
    el.addEventListener(evt, () => applyFilters(config, markerLayer, markerBySlug));
  });

  const clearBtn = document.getElementById("filter-clear");
  if (clearBtn) {
    clearBtn.addEventListener("click", () => {
      const q = document.getElementById("filter-q");
      const j = document.getElementById("filter-journey");
      const p = document.getElementById("filter-price");
      const c = document.getElementById("filter-cuisine");
      const v = document.getElementById("filter-value");

      if (q) q.value = "";
      if (j) j.value = "";
      if (p) p.value = "";
      if (c) c.value = "";
      if (v) v.checked = false;

      applyFilters(config, markerLayer, markerBySlug);
    });
  }
}

/**
 * ============================================================================
 * Context builder for templates (generic-ish)
 * ============================================================================
 */

function buildContext(item, slug) {
  const locBits = [item.neighborhood, item.city, item.state, item.country].filter(Boolean);

  const priceHtml = getPriceMeter(item.price_rating);
  const links = buildLinksHtml(item);
  const summary = escapeHtml(item.summary || "");
  const awardsHtml = awardsList(item.awards);

  const safeSlug = escapeHtml(String(slug || ""));

  const summaryCollapseId = `summary-${safeSlug}`;
  const awardsCollapseId = `awards-${safeSlug}`;

  const priceBlock = priceHtml
    ? `<div class="muted" style="margin-top:4px;">${priceHtml}</div>`
    : "";

  const linksBlock = links
    ? `<div style="margin-top:8px;">${links}</div>`
    : "";

  const summaryPanel = summary
    ? `
      <div class="panel panel-default" style="margin-top:8px;">
        <div class="panel-heading" style="padding:6px 10px;">
          <a data-toggle="collapse" href="#${summaryCollapseId}" style="display:block;">
            <strong>Summary</strong>
            <span class="pull-right muted">toggle</span>
          </a>
        </div>
        <div id="${summaryCollapseId}" class="panel-collapse collapse">
          <div class="panel-body" style="padding:8px 10px;">
            ${summary}
          </div>
        </div>
      </div>
    `
    : "";

  const awardsPanel = awardsHtml
    ? `
      <div class="panel panel-default" style="margin-top:8px;">
        <div class="panel-heading" style="padding:6px 10px;">
          <a data-toggle="collapse" href="#${awardsCollapseId}" style="display:block;">
            <strong>Awards</strong>
            <span class="pull-right muted">toggle</span>
          </a>
        </div>
        <div id="${awardsCollapseId}" class="panel-collapse collapse">
          <div class="panel-body" style="padding:8px 10px;">
            ${awardsHtml}
          </div>
        </div>
      </div>
    `
    : "";

  return {
    name: escapeHtml(item.name || ""),
    address: escapeHtml(item.address || ""),
    location: escapeHtml(locBits.join(", ")),
    journey_symbol: getJourneySymbol(item.journey_rating),
    cuisines_html: getCuisinesHtml(item.cuisines),
    price_block: priceBlock,
    value_html: item.value_recognition
      ? `<div style="margin-top:6px;"><span class="val-badge">◈ Exceptional value</span></div>`
      : "",
    awards_panel: awardsPanel,
    summary_panel: summaryPanel,
    links_block: linksBlock,
  };
}

/**
 * ============================================================================
 * Map init (generic engine)
 * ============================================================================
 */

export function initMap(config) {
  const {
    mapId,
    listId,
    zoom = 2,
    center = "",
    popupTemplateId,
    tooltipTemplateId,
  } = config;

  const mapEl = document.getElementById(mapId);
  if (!mapEl) return;

  const dataset =
    (window.__MAP_DATA__ && window.__MAP_DATA__[mapId]) ? window.__MAP_DATA__[mapId] : [];

  const map = L.map(mapId, { scrollWheelZoom: true });

  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    maxZoom: 19,
    attribution: "&copy; OpenStreetMap contributors",
  }).addTo(map);

  const markerLayer = L.layerGroup().addTo(map);
  const markerBySlug = {};

  const popupTpl = getTemplateHtml(popupTemplateId);
  const tooltipTpl = getTemplateHtml(tooltipTemplateId);

  dataset.forEach((d) => {
    const item = d._item || {};
    const ctx = buildContext(item);

    const popupHtml = popupTpl
      ? renderTemplate(popupTpl, ctx)
      : `<div><strong>${ctx.name}</strong></div>`;

    const tooltipHtml = tooltipTpl
      ? renderTemplate(tooltipTpl, ctx)
      : `<strong>${ctx.name}</strong>`;

    const m = L.marker([d.lat, d.lng], { icon: makeJourneyIcon(item.journey_rating) });
    m.bindTooltip(tooltipHtml, { sticky: true });
    m.bindPopup(popupHtml, { maxWidth: 340 });

    m.addTo(markerLayer);
    markerBySlug[d.slug] = m;
  });

  // Filters only if list exists
  const listEl = listId ? document.getElementById(listId) : null;
  if (listEl) {
    wireFilters(config, markerLayer, markerBySlug);
    applyFilters(config, markerLayer, markerBySlug);
  }

  // View logic
  if (center) {
    try {
      const parsed = JSON.parse(center);
      if (Array.isArray(parsed) && parsed.length === 2) map.setView(parsed, zoom);
      else map.setView([20, 0], zoom);
    } catch {
      map.setView([20, 0], zoom);
    }
  } else if (dataset.length) {
    const latlngs = dataset.map((x) => [x.lat, x.lng]);
    map.fitBounds(latlngs, { padding: [30, 30] });
  } else {
    map.setView([20, 0], zoom);
  }

  // List click -> open marker
  if (listEl) {
    listEl.addEventListener("click", (e) => {
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
