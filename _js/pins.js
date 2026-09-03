// Four-tier journey pins for the F&B map.
//
// ---------------------------------------------------------------------------
// REPLACING THE PINS
// ---------------------------------------------------------------------------
// Every pin lives in the PINS table below — one entry per tier, and nothing
// about pin appearance exists anywhere else. To swap in a new set, replace the
// entries; no other file needs to change.
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
//   journey-pin__halo        dashed outer ring (tier 3)
//   journey-pin__star        the gold star (tier 3)
//
// Per-tier overrides (sizes, tier 2's accent fill, tier 3's gold) are grouped
// under .journey-pin--t0 … --t3 in that same stylesheet.
//
// Adding a tier 4 means adding an entry here, a --t4 block in the stylesheet,
// a filter chip in _includes/f_and_b/filters.liquid, and a legend row in
// _layouts/f_and_b_establishments.liquid.
// ---------------------------------------------------------------------------

const TIER_3_STAR =
  "M21 11.2 L21.88 13.79 L24.61 13.83 L22.43 15.46 L23.23 18.07 " +
  "L21 16.5 L18.77 18.07 L19.57 15.46 L17.39 13.83 L20.12 13.79 Z";

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

  // Worth the visit.
  1: {
    width: 22,
    height: 30,
    anchor: [11, 30],
    shape:
      '<path class="journey-pin__body" d="M11 1c5 0 9 4 9 9 0 7-9 19-9 19S2 17 2 10c0-5 4-9 9-9z"/>'
      + '<circle class="journey-pin__eye" cx="11" cy="10" r="3.6"/>'
      + '<circle class="journey-pin__pupil" cx="11" cy="10" r="1.5"/>',
  },

  // Worth planning around — the aura is what reads at map scale.
  2: {
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

  // Worth the trip — the only pin that gets gold.
  3: {
    width: 42,
    height: 52,
    anchor: [21, 52],
    shape:
      '<circle class="journey-pin__halo" cx="21" cy="17" r="17"/>'
      + '<circle class="journey-pin__aura" cx="21" cy="17" r="14"/>'
      + '<circle class="journey-pin__aura-ring" cx="21" cy="17" r="13.5"/>'
      + '<path class="journey-pin__body" d="M21 4c6 0 10 4 10 11 0 9-10 28-10 28S11 24 11 15c0-7 4-11 10-11z"/>'
      + '<circle class="journey-pin__eye" cx="21" cy="15" r="5.5"/>'
      + `<path class="journey-pin__star" d="${TIER_3_STAR}"/>`,
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
