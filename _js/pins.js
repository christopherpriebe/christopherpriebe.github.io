// Four-tier journey pins for the F&B map.
//
// ---------------------------------------------------------------------------
// REPLACING THE PINS
// ---------------------------------------------------------------------------
// Every pin's geometry lives in the PINS table below — one entry per tier. To
// swap in a new set, replace the entries. Colours live in
// _sass/_f_and_b_establishments.sass, which also repeats each tier's pixel size
// as --pin-width / --pin-height for the CSS-sized wrapper; resizing a pin means
// editing the table and that stylesheet together.
//
// Each entry needs:
//
//   width, height  The SVG viewBox is "0 0 width height". Also the marker's
//                  pixel size on the map at scale 1, so keep them honest.
//   anchor         [x, y] in viewBox units: the point that sits on the map
//                  coordinate. A teardrop anchors at its tip ([w/2, height]);
//                  a dot anchors at its centre.
//   shape          SVG child elements as a string — no <svg> wrapper, no
//                  width/height/viewBox. Use the class names below instead of
//                  fill/stroke attributes so the pin picks up the site palette;
//                  anything else you hand it renders as authored.
//
// Palette classes, all coloured in _sass/_f_and_b_establishments.sass:
//
//   journey-pin__body        the pin silhouette
//   journey-pin__eye         the light centre disc
//   journey-pin__pupil       the dot inside the eye
//   journey-pin__aura        soft halo behind the pin
//   journey-pin__aura-ring   thin outline on that halo
//
// Per-tier overrides (sizes, tier 0/1 grey, tier 3's accent fill) are grouped
// under .journey-pin--t0 … --t3 in that same stylesheet.
//
// Adding a tier 4 means adding an entry here, a --t4 block in the stylesheet,
// a filter chip in _includes/f_and_b/filters.liquid, and a legend row in
// _layouts/f_and_b_establishments.liquid.
// ---------------------------------------------------------------------------

export const PINS = {
  // Selected — a dot, not a pin: worth knowing about, not a destination.
  0: {
    width: 16,
    height: 16,
    anchor: [8, 8],
    shape:
      '<circle class="journey-pin__eye" cx="8" cy="8" r="6"/>'
      + '<circle class="journey-pin__pupil" cx="8" cy="8" r="2.2"/>',
  },

  // Worth the visit — tier 0's dot drawn as a hollow teardrop. Tiers 0 and 1
  // are local distinctions, so both stay grey; colour starts at tier 2.
  1: {
    width: 20,
    height: 27,
    anchor: [10, 27],
    shape:
      '<path class="journey-pin__body" d="M10 1.5c4.4 0 8 3.5 8 8 0 6.2-8 16.5-8 16.5S2 15.7 2 9.5c0-4.5 3.6-8 8-8z"/>'
      + '<circle class="journey-pin__pupil" cx="10" cy="9.5" r="2.4"/>',
  },

  // Worth planning around — the first solid pin.
  2: {
    width: 22,
    height: 30,
    anchor: [11, 30],
    shape:
      '<path class="journey-pin__body" d="M11 1c5 0 9 4 9 9 0 7-9 19-9 19S2 17 2 10c0-5 4-9 9-9z"/>'
      + '<circle class="journey-pin__eye" cx="11" cy="10" r="3.6"/>'
      + '<circle class="journey-pin__pupil" cx="11" cy="10" r="1.5"/>',
  },

  // Worth the trip — accent body and aura; the aura is what reads at map scale.
  3: {
    width: 32,
    height: 40,
    anchor: [16, 40],
    shape:
      '<circle class="journey-pin__aura" cx="16" cy="13" r="13"/>'
      + '<circle class="journey-pin__aura-ring" cx="16" cy="13" r="12.5"/>'
      + '<path class="journey-pin__body" d="M16 3c5 0 9 4 9 10 0 8-9 24-9 24S7 21 7 13c0-6 4-10 9-10z"/>'
      + '<circle class="journey-pin__eye" cx="16" cy="13" r="4.6"/>'
      + '<circle class="journey-pin__pupil" cx="16" cy="13" r="2"/>',
  },
};

const LOWEST_TIER = Math.min(...Object.keys(PINS).map(Number));
const HIGHEST_TIER = Math.max(...Object.keys(PINS).map(Number));

export function getJourneyTier(journeyRating) {
  const tier = parseInt(journeyRating, 10) || 0;
  return Math.max(LOWEST_TIER, Math.min(HIGHEST_TIER, tier));
}

export function buildPinSvg(journeyRating) {
  const pin = PINS[getJourneyTier(journeyRating)];

  return `<svg class="journey-pin__svg" viewBox="0 0 ${pin.width} ${pin.height}"`
    + ' aria-hidden="true" focusable="false">'
    + pin.shape
    + "</svg>";
}

// Leaflet icon metrics, derived from the table so a resized pin stays anchored.
export function getMarkerMetrics(journeyRating) {
  const tier = getJourneyTier(journeyRating);
  const pin = PINS[tier];
  const [anchorX, anchorY] = pin.anchor;

  return {
    iconSize: [pin.width, pin.height],
    iconAnchor: [anchorX, anchorY],
    // Clear the top of the shape, so the popup never covers the pin. The top
    // edge sits at -anchorY from the anchor, whatever the silhouette.
    popupAnchor: [0, -anchorY - 2],
    zIndexOffset: tier * 100,
  };
}
