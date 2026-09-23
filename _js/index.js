import "normalize.css";

import { enhanceJourneyPins, initMap } from "./map";
import { initRouteMap } from "./routes";
import { initFilters } from "./filters";
import { initUnits } from "./units";
import { initInstruments } from "./instruments";
import { initAbstracts } from "./abstracts";
import { initEmailLinks } from "./email";

// Each step runs on its own so that one that throws (bad data, a missing
// element) is logged and costs only its own feature, not every one after it.
function run(name, step) {
    try {
        step();
    } catch (error) {
        console.error(`${name} failed:`, error);
    }
}

document.addEventListener("DOMContentLoaded", () => {
    run("initEmailLinks", initEmailLinks);
    run("initUnits", initUnits);
    run("enhanceJourneyPins", enhanceJourneyPins);

    const queue = window.__MAP_INIT__ || [];
    queue.forEach((cfg) => run(`initMap(${cfg.mapId})`, () => initMap(cfg)));

    const routeQueue = window.__ROUTE_MAP_INIT__ || [];
    routeQueue.forEach((cfg) => run(`initRouteMap(${cfg.mapId})`, () => initRouteMap(cfg)));

    run("initFilters", initFilters);
    run("initInstruments", initInstruments);
    run("initAbstracts", initAbstracts);
});
