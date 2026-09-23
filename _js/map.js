import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { attachBasemaps } from "./basemaps";
import { buildPinSvg, getJourneyTier, getMarkerMetrics } from "./pins";

// TODO: Refactor this so that there is a generic map engine
//      right now, some features are specific to the F&B domain
//      e.g., filtering at the moment is specific to the F&B domain
//      Also, move the F&B stuff to a separate file

// TODO: Write a way to more easily manage F&B awards, as right now
//      the manual effort to add awards is too much;
//      can do the same things for other parts of F&B such as if it
//      is closed and such.

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
  american: { label: "American", emoji: "🇺🇸" },
  argentine: { label: "Argentine", emoji: "🇦🇷" },
  indian: { label: "Indian", emoji: "🇮🇳" },
  french: { label: "French", emoji: "🇫🇷" },
  italian: { label: "Italian", emoji: "🇮🇹" },
  german: { label: "German", emoji: "🇩🇪" },
  spanish: { label: "Spanish", emoji: "🇪🇸" },
  mexican: { label: "Mexican", emoji: "🇲🇽" },
  japanese: { label: "Japanese", emoji: "🇯🇵" },
  thai: { label: "Thai", emoji: "🇹🇭" },
  chinese: { label: "Chinese", emoji: "🇨🇳" },
  korean: { label: "Korean", emoji: "🇰🇷" },
  vietnamese: { label: "Vietnamese", emoji: "🇻🇳" },
  asian: { label: "Asian", emoji: "🍽️" },
  european: { label: "European", emoji: "🍽️" },
  mediterranean: { label: "Mediterranean", emoji: "🍽️" },
  pizza: { label: "Pizza", emoji: "🍕" },
  barbecue: { label: "BBQ", emoji: "🍖" },
  burgers: { label: "Burgers", emoji: "🍔" },
  hot_dogs: { label: "Hot Dogs", emoji: "🌭" },
  sandwiches: { label: "Sandwiches", emoji: "🥪" },
  steakhouse: { label: "Steakhouse", emoji: "🥩" },
  tacos: { label: "Tacos", emoji: "🌮" },
  sushi: { label: "Sushi", emoji: "🍣" },
  ramen: { label: "Ramen", emoji: "🍜" },
  tonkatsu: { label: "Tonkatsu", emoji: "🍗" },
  yakitori: { label: "Yakitori", emoji: "🍢" },
  seafood: { label: "Seafood", emoji: "🦞" },
  vegetarian: { label: "Vegetarian", emoji: "🥬" },
  vegan: { label: "Vegan", emoji: "🌱" },
  breakfast: { label: "Breakfast", emoji: "🥓" },
  brunch: { label: "Brunch", emoji: "🍳" },
  fusion: { label: "Fusion", emoji: "🍽️" },
  haute: { label: "Haute", emoji: "🍽️" },
  ice_cream: { label: "Ice Cream", emoji: "🍦" },
  gelato: { label: "Gelato", emoji: "🍨" },
  bakery: { label: "Bakery", emoji: "🥖" },
  cafe: { label: "Cafe", emoji: "☕" },
  dive_bar: { label: "Dive Bar", emoji: "🍹" },
  sports_bar: { label: "Sports Bar", emoji: "🏟️" },
  pub: { label: "Pub", emoji: "🍻" },
  cocktail_bar: { label: "Cocktail Bar", emoji: "🍸" },
  beer: { label: "Beer", emoji: "🍺" },
  wine: { label: "Wine", emoji: "🍷" },
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

function getMetaRowHtml(innerHtml, className = "") {
  if (!innerHtml) return "";
  return `<div class="fnb-meta-row${className ? ` ${className}` : ""}">${innerHtml}</div>`;
}

export function getCuisinesHtml(keys) {
  if (!Array.isArray(keys) || !keys.length) return "";
  const parts = keys.map((k) => {
    const d = getCuisineDisplay(k);
    return `${d.emoji} ${escapeHtml(d.label)}`;
  });
  return getMetaRowHtml(parts.join(" · "), "fnb-meta-row--cuisines muted");
}

function getJourneyLabel(journeyRating) {
  const tier = getJourneyTier(journeyRating);
  if (tier === 3) return "Worth the trip";
  if (tier === 2) return "Worth planning around";
  if (tier === 1) return "Worth the visit";
  return "Selected";
}

// Fills in pins already rendered by Liquid — the filter chips, the sidebar
// list, and the tier legend. Each carries its tier as a class.
export function enhanceJourneyPins(root = document) {
  const nodes = (root.matches && root.matches(".journey-pin"))
    ? [root]
    : Array.from(root.querySelectorAll(".journey-pin"));

  nodes.forEach((el) => {
    if (el.getAttribute("data-journey-ready") === "1") return;

    const matched = /journey-pin--t(\d)/.exec(el.className);
    el.innerHTML = buildPinSvg(matched ? matched[1] : 0);
    el.setAttribute("data-journey-ready", "1");
  });
}

function getJourneyPinMarkup(journeyRating, variant = "map") {
  const tier = getJourneyTier(journeyRating);
  const attrs = (variant === "map")
    ? 'aria-hidden="true"'
    : `role="img" aria-label="${escapeHtml(getJourneyLabel(tier))}"`;

  return `
    <span class="journey-pin journey-pin--${variant} journey-pin--t${tier}" data-journey-ready="1" ${attrs}>
      ${buildPinSvg(tier)}
    </span>
  `.trim();
}

function getJourneySymbol(journeyRating) {
  return getJourneyPinMarkup(journeyRating, "inline");
}

function getPriceMeter(priceRating) {
  const n = Math.max(0, Math.min(5, parseInt(priceRating, 10) || 0));
  if (!n) return "";

  const squares = Array.from({ length: 5 }, (_, i) => (
    `<span class="price-meter__square${i < n ? " is-filled" : ""}" aria-hidden="true"></span>`
  )).join("");

  return `
    <span class="price-meter" aria-label="Price ${n} out of 5">
      <span class="price-meter__label">Price</span>
      <span class="price-meter__track">${squares}</span>
    </span>
  `.trim();
}

function getPriceBlockHtml(priceRating) {
  const priceHtml = getPriceMeter(priceRating);
  return priceHtml ? getMetaRowHtml(priceHtml, "fnb-meta-row--price") : "";
}

// The list rows carry cuisines as plain labels; the emoji stay in the popup.
function getCuisineLabels(keys) {
  if (!Array.isArray(keys)) return "";
  return keys.map((k) => getCuisineDisplay(k).label).join(", ");
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
      .map(() => '<span class="michelin-red">✱</span>')
      .join("");
    bits.push(`<div>${starIcons}</div>`);
  }

  const lines = [];

  const y1 = formatYearRanges(m.years_of_1_star);
  if (y1.count) {
    lines.push(
      `<li><span class="michelin-red">✱</span> 1-star: <strong>${y1.count}</strong> year${y1.count === 1 ? "" : "s"} (${escapeHtml(y1.text)})</li>`
    );
  }

  const y2 = formatYearRanges(m.years_of_2_stars);
  if (y2.count) {
    lines.push(
      `<li><span class="michelin-red">✱✱</span> 2-star: <strong>${y2.count}</strong> year${y2.count === 1 ? "" : "s"} (${escapeHtml(y2.text)})</li>`
    );
  }

  const y3 = formatYearRanges(m.years_of_3_stars);
  if (y3.count) {
    lines.push(
      `<li><span class="michelin-red">✱✱✱</span> 3-star: <strong>${y3.count}</strong> year${y3.count === 1 ? "" : "s"} (${escapeHtml(y3.text)})</li>`
    );
  }

  const yBib = formatYearRanges(m.years_of_bib);
  if (yBib.count) {
    lines.push(
      `<li><span class="michelin-red">Bib</span> <strong>${yBib.count}</strong> year${yBib.count === 1 ? "" : "s"} (${escapeHtml(yBib.text)})</li>`
    );
  }

  const yGreen = formatYearRanges(m.years_of_green);
  if (yGreen.count) {
    lines.push(
      `<li><span class="michelin-green">Green</span> <strong>${yGreen.count}</strong> year${yGreen.count === 1 ? "" : "s"} (${escapeHtml(yGreen.text)})</li>`
    );
  }

  if (m.bib) bits.push('<div><span class="michelin-red">Bib Gourmand</span></div>');
  if (m.green) bits.push('<div><span class="michelin-green">Green Star</span></div>');

  if (lines.length) {
    bits.push(`
      <div>
        <div class="muted">Michelin history</div>
        <ul>
          ${lines.join("")}
        </ul>
      </div>
    `);
  }

  return bits.join("");
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
      <div>
        <div class="muted">Other awards</div>
        <ul>
          ${items}
        </ul>
      </div>
    `;
  }

  return out;
}

function getListItems(listId) {
  return Array.from(document.querySelectorAll(`#${listId} .map-card`));
}

function parseMultiFilterValue(value) {
  return String(value || "")
    .split(/\s+/)
    .map((part) => part.trim())
    .filter(Boolean);
}

function getToggleFilterDefaultValue(inputId) {
  const input = document.getElementById(inputId);
  if (!input) return "";
  return input.getAttribute("data-filter-default") || input.defaultValue || "";
}

function syncToggleFilterGroup(inputId) {
  const input = document.getElementById(inputId);
  const group = document.querySelector(`[data-filter-group="${inputId}"]`);
  if (!input || !group) return;

  const isMulti = group.getAttribute("data-filter-multi") === "true";
  const fillThrough = group.getAttribute("data-filter-fill-through") === "true";
  const selected = isMulti ? new Set(parseMultiFilterValue(input.value)) : null;
  const value = input.value || "";
  const numericValue = parseInt(value, 10);
  group.querySelectorAll("[data-filter-value]").forEach((button) => {
    const buttonValue = button.getAttribute("data-filter-value") || "";
    const buttonNumericValue = parseInt(buttonValue, 10);
    const active = isMulti
      ? selected.has(buttonValue)
      : fillThrough && Number.isFinite(numericValue) && Number.isFinite(buttonNumericValue)
        ? buttonNumericValue <= numericValue
        : buttonValue === value;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-pressed", active ? "true" : "false");
  });
}

function wireToggleFilter(inputId, onChange) {
  const input = document.getElementById(inputId);
  const group = document.querySelector(`[data-filter-group="${inputId}"]`);
  if (!input || !group) return;

  const isMulti = group.getAttribute("data-filter-multi") === "true";
  syncToggleFilterGroup(inputId);

  group.addEventListener("click", (event) => {
    const button = event.target.closest("[data-filter-value]");
    if (!button || !group.contains(button)) return;

    const clickedValue = button.getAttribute("data-filter-value") || "";
    let nextValue = clickedValue;

    if (isMulti) {
      const selected = new Set(parseMultiFilterValue(input.value));
      if (selected.has(clickedValue)) selected.delete(clickedValue);
      else if (clickedValue) selected.add(clickedValue);
      nextValue = Array.from(selected).sort().join(" ");
    } else {
      const currentValue = input.value || "";
      nextValue = (clickedValue && clickedValue === currentValue) ? "" : clickedValue;
    }

    input.value = nextValue;
    syncToggleFilterGroup(inputId);
    onChange();
  });
}

function applyFilters(config, markerLayer, markerBySlug) {
  const qEl = document.getElementById("filter-q");
  const journeyEl = document.getElementById("filter-journey");
  const priceEl = document.getElementById("filter-price");
  const priceModeEl = document.getElementById("filter-price-mode");
  const cuisineEl = document.getElementById("filter-cuisine");
  const valueEl = document.getElementById("filter-value");

  const q = (qEl ? qEl.value : "").trim().toLowerCase();
  const journey = journeyEl ? journeyEl.value : "";
  const journeySelections = new Set(parseMultiFilterValue(journey));
  const price = priceEl ? priceEl.value : "";
  const priceValue = parseInt(price, 10);
  const priceMode = priceModeEl ? priceModeEl.value : "";
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

    if (journeySelections.size && !journeySelections.has(elJourney)) ok = false;
    if (price && Number.isFinite(priceValue)) {
      const elPriceValue = parseInt(elPrice, 10);
      if (!Number.isFinite(elPriceValue)) ok = false;
      else if (priceMode === "eq" && elPriceValue !== priceValue) ok = false;
      else if (priceMode === "gte" && elPriceValue < priceValue) ok = false;
      else if (!priceMode && elPriceValue > priceValue) ok = false;
    }
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

  // A group heading only stays while something under it survives the filters.
  const listEl = document.getElementById(config.listId);
  if (listEl) {
    listEl.querySelectorAll("[data-map-group]").forEach((group) => {
      const visible = Array.from(group.querySelectorAll(".map-card"))
        .some((card) => card.style.display !== "none");
      group.style.display = visible ? "" : "none";
    });

    const empty = listEl.querySelector("[data-map-empty]");
    if (empty) empty.style.display = allowed.length ? "none" : "block";
  }

  markerLayer.clearLayers();
  allowed.forEach((slug) => {
    const m = markerBySlug[slug];
    if (m) m.addTo(markerLayer);
  });

  // Keep the filter panel's "N of M places" in step with the filters.
  const countEl = document.querySelector("[data-map-count]");
  if (countEl) countEl.textContent = allowed.length;
}

function wireFilters(config, markerLayer, markerBySlug) {
  const ids = ["filter-q", "filter-cuisine", "filter-value"];
  ids.forEach((id) => {
    const el = document.getElementById(id);
    if (!el) return;

    const evt = (id === "filter-q") ? "input" : "change";
    el.addEventListener(evt, () => applyFilters(config, markerLayer, markerBySlug));
  });

  wireToggleFilter("filter-journey", () => applyFilters(config, markerLayer, markerBySlug));
  wireToggleFilter("filter-price", () => applyFilters(config, markerLayer, markerBySlug));
  wireToggleFilter("filter-price-mode", () => applyFilters(config, markerLayer, markerBySlug));

  const clearBtn = document.getElementById("filter-clear");
  if (clearBtn) {
    clearBtn.addEventListener("click", () => {
      const q = document.getElementById("filter-q");
      const j = document.getElementById("filter-journey");
      const p = document.getElementById("filter-price");
      const pm = document.getElementById("filter-price-mode");
      const c = document.getElementById("filter-cuisine");
      const v = document.getElementById("filter-value");

      if (q) q.value = "";
      if (j) j.value = "";
      if (p) p.value = "";
      if (pm) pm.value = getToggleFilterDefaultValue("filter-price-mode");
      if (c) c.value = "";
      if (v) v.checked = false;
      syncToggleFilterGroup("filter-journey");
      syncToggleFilterGroup("filter-price");
      syncToggleFilterGroup("filter-price-mode");

      applyFilters(config, markerLayer, markerBySlug);
    });
  }
}

function hydrateList(config, dataset) {
  if (!config.listId) return;

  const itemBySlug = new Map();
  const itemByName = new Map();

  dataset.forEach((entry) => {
    const item = entry._item || {};
    itemBySlug.set(String(entry.slug || ""), item);
    itemByName.set(String(item.name || "").trim().toLowerCase(), item);
  });

  getListItems(config.listId).forEach((el) => {
    const slug = String(el.getAttribute("data-slug") || "");
    const name = String(el.getAttribute("data-name") || "").trim().toLowerCase();
    const item = itemBySlug.get(slug) || itemByName.get(name);
    const metaEl = el.querySelector("[data-map-card-meta]");

    if (!item || !metaEl) return;
    metaEl.textContent = getCuisineLabels(item.cuisines);
  });

  // Liquid can only title-case the cuisine keys; the table knows "BBQ".
  document.querySelectorAll("#filter-cuisine option[value]").forEach((option) => {
    if (option.value) option.textContent = getCuisineDisplay(option.value).label;
  });
}

// Fields for the popup and tooltip templates. The popup is only a label; the
// full record opens beneath the place's row in the list.
function buildContext(item) {
  const locBits = [item.neighborhood, item.city, item.state, item.country].filter(Boolean);

  return {
    name: escapeHtml(item.name || ""),
    address: escapeHtml(item.address || ""),
    location: escapeHtml(locBits.join(", ")),
    journey_symbol: getJourneySymbol(item.journey_rating),
  };
}

// The details panel under a selected row: where it is, what it serves, what
// it costs, what it has won, what I thought, and where to go next.
function buildDetailsHtml(item) {
  const locBits = [item.neighborhood, item.city, item.state, item.country].filter(Boolean);
  const summary = escapeHtml(item.summary || "");
  const awardsHtml = awardsList(item.awards);
  const links = buildLinksHtml(item);

  return [
    `<div class="row-details__line">${escapeHtml(getJourneyLabel(item.journey_rating))} &middot; ${escapeHtml(locBits.join(", "))}</div>`,
    item.address ? `<div class="row-details__line">${escapeHtml(item.address)}</div>` : "",
    getCuisinesHtml(item.cuisines),
    getPriceBlockHtml(item.price_rating),
    item.value_recognition
      ? '<div class="fnb-meta-row"><span class="val-badge">◈</span> Exceptional value</div>'
      : "",
    summary ? `<p class="row-details__summary">${summary}</p>` : "",
    awardsHtml ? `<div class="row-details__awards">${awardsHtml}</div>` : "",
    links ? `<div class="row-details__links">${links}</div>` : "",
  ].filter(Boolean).join("");
}

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

  attachBasemaps(map, mapEl.parentNode);

  const markerLayer = L.layerGroup().addTo(map);
  const markerBySlug = {};

  const popupTpl = getTemplateHtml(popupTemplateId);
  const tooltipTpl = getTemplateHtml(tooltipTemplateId);

  dataset.forEach((d) => {
    const item = d._item || {};
    const ctx = buildContext(item);
    const tier = getJourneyTier(item.journey_rating);
    const markerMetrics = getMarkerMetrics(tier);

    const popupHtml = popupTpl
      ? renderTemplate(popupTpl, ctx)
      : `<div><strong>${ctx.name}</strong></div>`;

    const tooltipHtml = tooltipTpl
      ? renderTemplate(tooltipTpl, ctx)
      : `<strong>${ctx.name}</strong>`;

    const m = L.marker([d.lat, d.lng], {
      icon: L.divIcon({
        className: "journey-marker-icon",
        html: getJourneyPinMarkup(tier, "map"),
        iconSize: markerMetrics.iconSize,
        iconAnchor: markerMetrics.iconAnchor,
        popupAnchor: markerMetrics.popupAnchor,
      }),
      alt: `${item.name || "Establishment"} (${getJourneyLabel(tier)})`,
      riseOnHover: true,
      title: item.name || "",
      zIndexOffset: markerMetrics.zIndexOffset,
    });
    m.bindTooltip(tooltipHtml, { sticky: true });
    m.bindPopup(popupHtml, { maxWidth: 340 });

    m.addTo(markerLayer);
    markerBySlug[d.slug] = m;
  });

  const listEl = listId ? document.getElementById(listId) : null;
  if (listEl) {
    hydrateList(config, dataset);
    wireFilters(config, markerLayer, markerBySlug);
    applyFilters(config, markerLayer, markerBySlug);
  }

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

  if (!listEl) return;

  const itemBySlug = new Map(dataset.map((d) => [String(d.slug), d._item || {}]));
  const cards = new Map(getListItems(listId).map((card) => [card.getAttribute("data-slug"), card]));
  let openSlug = null;

  function setOpen(card, open) {
    const toggle = card.querySelector(".map-card__toggle");
    const details = card.querySelector("[data-map-card-details]");
    card.classList.toggle("is-open", open);
    if (toggle) toggle.setAttribute("aria-expanded", String(open));
    if (!details) return;
    if (open && !details.hasAttribute("data-ready")) {
      details.innerHTML = buildDetailsHtml(itemBySlug.get(card.getAttribute("data-slug")) || {});
      details.setAttribute("data-ready", "");
    }
    details.hidden = !open;
  }

  // Scrolls the list, not the page, so the row sits just under its sticky
  // city heading.
  function revealInList(card) {
    const group = card.closest("[data-map-group]");
    const heading = group ? group.querySelector(".rows__group-name") : null;
    const top = card.getBoundingClientRect().top - listEl.getBoundingClientRect().top
      + listEl.scrollTop - (heading ? heading.offsetHeight : 0);
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    listEl.scrollTo({ top, behavior: reduceMotion ? "auto" : "smooth" });
  }

  // One place is open at a time. From the list, selecting the open row closes
  // it; from the map, a marker always opens its row.
  function selectPlace(slug, fromMap) {
    const card = cards.get(slug);
    const marker = markerBySlug[slug];
    if (!card) return;

    if (!fromMap && openSlug === slug) {
      setOpen(card, false);
      openSlug = null;
      if (marker) marker.closePopup();
      return;
    }

    if (openSlug && openSlug !== slug && cards.has(openSlug)) setOpen(cards.get(openSlug), false);
    setOpen(card, true);
    openSlug = slug;

    if (fromMap) {
      revealInList(card);
    } else if (marker) {
      map.setView(marker.getLatLng(), Math.max(map.getZoom(), 14));
      marker.openPopup();
    }
  }

  Object.entries(markerBySlug).forEach(([slug, marker]) => {
    marker.on("click", () => selectPlace(slug, true));
  });

  listEl.addEventListener("click", (e) => {
    const toggle = e.target.closest(".map-card__toggle");
    if (!toggle || !listEl.contains(toggle)) return;
    selectPlace(toggle.closest(".map-card").getAttribute("data-slug"), false);
  });
}
