// Publications timeline. One dot per paper, placed on a shared year axis and
// read straight out of the rendered bibliography, so the chart cannot disagree
// with the listing below it. Colours live in _sass/_publications.sass; this
// only assigns the type class.

const TYPE_SELECTORS = {
  conference: "#conference-papers",
  journal: "#journal-papers",
  preprint: "#preprints",
};

const TYPE_LABELS = {
  conference: "Conference",
  journal: "Journal",
  preprint: "Preprint",
};

// Timeline geometry, in step with .dots in the stylesheet. ROW_STEP is a
// minimum: rows are spaced by measured label height so a title that wraps to
// six lines on a phone still clears its neighbours.
const ROW_TOP = 44;
const ROW_STEP = 68;
const ROW_GAP = 14;
const AXIS_GAP = 28;

// Horizontal insets, leaving room for a label beside the outermost dots. The
// right inset only needs to keep the newest paper off the edge — labels hang
// to the left — so it stays small and the timeline uses the column width.
const PAD_LEFT = 4;
const PAD_RIGHT = 10;

// Below this the label sits to the right of its dot rather than the left.
const FLIP_SIDE_AT = 45;

// An axis shorter than this reads as a stub rather than a timeline.
const MIN_AXIS_YEARS = 5;

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun",
                "jul", "aug", "sep", "oct", "nov", "dec"];

const MONTH_LABELS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                      "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// BibTeX months arrive as macros (dec), names (December) or numbers (12).
function monthIndex(raw) {
  const value = String(raw || "").trim().toLowerCase();
  if (!value) return null;

  const numeric = Number(value);
  if (Number.isInteger(numeric) && numeric >= 1 && numeric <= 12) return numeric - 1;

  const index = MONTHS.indexOf(value.slice(0, 3));
  return index === -1 ? null : index;
}

// Fractional year used for horizontal placement, so December sits nearly a
// full year right of January. An entry with no month lands mid-year rather
// than being pinned to a January it does not claim.
function timelinePosition(year, month) {
  return month === null ? year + 0.5 : year + (month + 0.5) / 12;
}

function shortTitle(entry, title) {
  if (entry) return entry;
  // Most titles lead with the system name before a colon, which reads better
  // in a narrow label than the full subtitle.
  return title.split(":")[0].trim() || title;
}

function collectPublications() {
  const publications = [];

  Object.keys(TYPE_SELECTORS).forEach((type) => {
    const container = document.querySelector(TYPE_SELECTORS[type]);
    if (!container) return;

    container.querySelectorAll(".bib-entry").forEach((entry) => {
      const year = parseInt(entry.getAttribute("data-pub-year"), 10);
      if (!Number.isFinite(year)) return;

      const month = monthIndex(entry.getAttribute("data-pub-month"));
      const title = entry.getAttribute("data-pub-title") || "";

      publications.push({
        year,
        month,
        type,
        title,
        venue: entry.getAttribute("data-pub-venue") || TYPE_LABELS[type],
        short: shortTitle(entry.getAttribute("data-pub-short"), title),
        at: timelinePosition(year, month),
      });
    });
  });

  // Left to right down the page, so the leader lines never cross.
  return publications.sort((a, b) => a.at - b.at);
}

function dateLabel(publication) {
  return publication.month === null
    ? String(publication.year)
    : `${MONTH_LABELS[publication.month]} ${publication.year}`;
}

function buildTooltip() {
  const tooltip = document.getElementById("pubs-tooltip");
  if (!tooltip) return { attach: () => {} };

  function move(event) {
    // Keep the tooltip inside the viewport when hovering near an edge.
    const width = tooltip.offsetWidth || 220;
    const left = Math.min(event.clientX + 14, window.innerWidth - width - 12);
    tooltip.style.left = `${Math.max(12, left)}px`;
    tooltip.style.top = `${event.clientY + 16}px`;
  }

  return {
    attach(element, publication) {
      element.addEventListener("mouseenter", (event) => {
        tooltip.innerHTML =
          `<strong>${publication.title}</strong><br><em>${publication.venue} · ${dateLabel(publication)}</em>`;
        tooltip.style.display = "block";
        move(event);
      });
      element.addEventListener("mousemove", move);
      element.addEventListener("mouseleave", () => {
        tooltip.style.display = "none";
      });
    },
  };
}

export function initPublicationsChart() {
  const plot = document.getElementById("pubs-dots");
  const axis = document.getElementById("pubs-axis");
  if (!plot || !axis) return;

  const publications = collectPublications();
  if (!publications.length) {
    const container = document.getElementById("pubs-chart-container");
    if (container) container.style.display = "none";
    return;
  }

  const positions = publications.map((publication) => publication.at);
  const axisEnd = Math.max(...positions);
  const earliest = Math.floor(Math.min(...positions));
  // Pad the axis back so a couple of recent papers still read as a timeline.
  const axisStart = Math.min(earliest, Math.ceil(axisEnd) - MIN_AXIS_YEARS);
  const span = Math.max(1, axisEnd - axisStart);

  const xFor = (at) =>
    PAD_LEFT + ((at - axisStart) / span) * (100 - PAD_LEFT - PAD_RIGHT);

  const tooltip = buildTooltip();

  const rows = publications.map((publication) => {
    const x = xFor(publication.at);
    const side = x < FLIP_SIDE_AT ? "r" : "l";

    const dot = document.createElement("div");
    dot.className = `dots__dot dots__dot--${publication.type}`;
    dot.style.left = `${x}%`;
    dot.textContent = (publication.venue === TYPE_LABELS[publication.type]
      ? publication.short
      : publication.venue).charAt(0).toUpperCase();
    tooltip.attach(dot, publication);

    const label = document.createElement("div");
    label.className = `dots__label dots__label--${side}`;
    label.style.left = `${x}%`;
    label.innerHTML = `<b>${publication.venue} &middot; ${dateLabel(publication)}</b>${publication.short}`;
    tooltip.attach(label, publication);

    plot.appendChild(dot);
    plot.appendChild(label);

    return { dot, label };
  });

  // Append first, then measure, then write — one layout pass rather than one
  // per row. Labels are centred on their row, so consecutive rows must be at
  // least half of each label apart.
  const heights = rows.map((row) => row.label.offsetHeight);

  let top = Math.max(ROW_TOP, heights[0] / 2 + 8);
  rows.forEach((row, index) => {
    if (index > 0) {
      const needed = (heights[index - 1] + heights[index]) / 2 + ROW_GAP;
      top += Math.max(ROW_STEP, needed);
    }
    row.dot.style.top = `${top}px`;
    row.label.style.top = `${top}px`;
  });

  axis.style.marginTop = `${top + heights[heights.length - 1] / 2 + AXIS_GAP}px`;

  // Ticks mark January of each year; a dot's offset from its tick is its month.
  for (let year = axisStart; year <= Math.floor(axisEnd); year += 1) {
    const tick = document.createElement("div");
    tick.className = "dots__tick";
    tick.style.left = `${xFor(year)}%`;
    tick.innerHTML = `<span>${year}</span>`;
    axis.appendChild(tick);
  }

  const legend = document.getElementById("pubs-legend");
  if (!legend) return;

  const present = new Set(publications.map((publication) => publication.type));
  Object.keys(TYPE_LABELS).forEach((type) => {
    if (!present.has(type)) return;
    const item = document.createElement("div");
    item.className = "legend-item";
    item.innerHTML = `<span class="legend-dot dots__dot--${type}"></span> ${TYPE_LABELS[type]}`;
    legend.appendChild(item);
  });
}
