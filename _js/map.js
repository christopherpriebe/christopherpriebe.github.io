import L from "leaflet";
import "leaflet/dist/leaflet.css";

function getErrorText(fieldName) {
    return `<p style="color:red;">ERROR: Invalid ${escapeHtml(fieldName)}</p>`;
}

export const CUISINES = {
    italian:   { label: "Italian", emoji: "🇮🇹" },
    pizza:     { label: "Pizza", emoji: "🍕" },
    barbecue:       { label: "BBQ", emoji: "🍖" },
    mexican:   { label: "Mexican", emoji: "🇲🇽" },
    tacos:   { label: "Tacos", emoji: "🌮" },
    japanese:  { label: "Japanese", emoji: "🇯🇵" },
    sushi:     { label: "Sushi", emoji: "🍣" },
    thai:      { label: "Thai", emoji: "🇹🇭" },
    chinese:   { label: "Chinese", emoji: "🇨🇳" },
    korean:    { label: "Korean", emoji: "🇰🇷" },
    vietnamese:{ label: "Vietnamese", emoji: "🇻🇳" },
    indian:    { label: "Indian", emoji: "🇮🇳" },
    french:    { label: "French", emoji: "🇫🇷" },
    seafood:   { label: "Seafood", emoji: "🦞" },
    bakery:    { label: "Bakery", emoji: "🥖" },
    cafe:      { label: "Cafe", emoji: "☕" },
};

export function getCuisineDisplay(key) {
    const k = String(key || "").trim();
    const entry = CUISINES[k];
    if (!entry) return { label: k || k.charAt(0).toUpperCase() + k.slice(1), emoji: "🍽️" };
    return entry;
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
        popupAnchor: [0, -10]
    });
}

function escapeHtml(s) {
    return String(s || "").replace(/[&<>"']/g, (m) => ({
        "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"
    }[m]));
}

function getJourneySymbol(journeyRating) {
    const n = parseInt(journeyRating, 10) || 0;
    if (n === 3) return '<i class="fa-solid fa-circle jr-marker jr-3"></i>';
    if (n === 2) return '<i class="fa-regular fa-circle-dot jr-marker jr-2"></i>';
    if (n === 1) return '<i class="fa-regular fa-circle jr-marker jr-1"></i>';
    return "";
}

function getPriceMeter(priceRating) {
    const n = Math.max(0, Math.min(5, parseInt(priceRating, 10) || 0));
    if (!n) return "";
    let out = "Price: ";
    for (let i = 1; i <= 5; i++) out += (i <= n ? '<i class="fa-solid fa-square" style="margin-right:2px;"></i>' : '<i class="fa-regular fa-square" style="margin-right:2px;"></i>');
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
    const parts = ranges.map(([a, b]) => (a === b ? `${a}` : `${a}\u2013${b}`)); // en dash
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
    if (y1.count) lines.push(`<li><span style="color:#BD2333;">✱</span> 1-star: <strong>${y1.count}</strong> year${y1.count === 1 ? "" : "s"} (${escapeHtml(y1.text)})</li>`);

    const y2 = formatYearRanges(m.years_of_2_stars);
    if (y2.count) lines.push(`<li><span style="color:#BD2333;">✱✱</span> 2-star: <strong>${y2.count}</strong> year${y2.count === 1 ? "" : "s"} (${escapeHtml(y2.text)})</li>`);

    const y3 = formatYearRanges(m.years_of_3_stars);
    if (y3.count) lines.push(`<li><span style="color:#BD2333;">✱✱✱</span> 3-star: <strong>${y3.count}</strong> year${y3.count === 1 ? "" : "s"} (${escapeHtml(y3.text)})</li>`);

    const yBib = formatYearRanges(m.years_of_bib);
    if (yBib.count) lines.push(`<li><span class="val-badge" style="color:#BD2333;">Bib</span> <strong>${yBib.count}</strong> year${yBib.count === 1 ? "" : "s"} (${escapeHtml(yBib.text)})</li>`);

    const yGreen = formatYearRanges(m.years_of_green);
    if (yGreen.count) lines.push(`<li><span class="val-badge" style="color:#84BD00;">Green</span> <strong>${yGreen.count}</strong> year${yGreen.count === 1 ? "" : "s"} (${escapeHtml(yGreen.text)})</li>`);

    // Also show simple current badges if present (optional — you may omit these if redundant)
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

    if (!bits.length) return "";
    return `<div style="margin-top:6px;">${bits.join("")}</div>`;
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

function buildFoodPopup(item) {
    const title = `<strong>${escapeHtml(item.name || getErrorText("name"))}</strong> ${getJourneySymbol(item.journey_rating)}`;
    const locBits = [];
    if (item.neighborhood) locBits.push(item.neighborhood);
    if (item.city) locBits.push(item.city);
    if (item.state) locBits.push(item.state);
    if (item.country) locBits.push(item.country);
    const location = locBits.length ? `<div class="muted">${escapeHtml(locBits.join(", "))}</div>` : "";
    const addr = item.address ? `<div class="muted">${escapeHtml(item.address)}</div>` : "";
    const value = item.value_recognition ? `<div style="margin-top:6px;"><span class="val-badge">◈ Exceptional value</span></div>` : "";
    const tldr = item.tldr ? `<div style="margin-top:6px;">${escapeHtml(item.tldr)}</div>` : "";
    const links = [];
    if (item.map_url) links.push(`<a href="${escapeHtml(item.map_url)}" target="_blank" rel="noopener">Map</a>`);
    if (item.website) links.push(`<a href="${escapeHtml(item.website)}" target="_blank" rel="noopener">Website</a>`);
    if (item.reservation_url) links.push(`<a href="${escapeHtml(item.reservation_url)}" target="_blank" rel="noopener">Reservations</a>`);
    const linksHtml = links.length ? `<div style="margin-top:8px;">${links.join(" · ")}</div>` : "";
    return `<div>
        ${title}
        ${location}
        ${addr}
        ${getCuisinesHtml(item.cuisines)}
        ${getPriceMeter(item.price_rating)}
        ${value}
        ${awardsList(item.awards)}
        ${tldr}
        ${linksHtml}
        </div>`;
}

function buildGenericPopup(item) {
    const title = `<strong>${escapeHtml(item.name || getErrorText("name"))}</strong>`;
    return `<div>${title}</div>`;
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

    const markerLayer = L.layerGroup().addTo(map);
    const markerBySlug = {};

    dataset.forEach((d) => {
    const item = d._item || {};
    const popupHtml = (popupType === "f_and_b_establishment") ? buildFoodPopup(item) : buildGenericPopup(item);

    const m = L.marker([d.lat, d.lng], { icon: makeJourneyIcon(item.journey_rating) });
        m.bindTooltip(`<strong>${escapeHtml(item.name || "")}</strong>`, { sticky: true });
        m.bindPopup(buildFoodPopup(item), { maxWidth: 340 });
        m.addTo(markerLayer);
        markerBySlug[d.slug] = m;
    });

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
