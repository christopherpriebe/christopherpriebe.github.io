import "normalize.css";

import { enhanceJourneyPins, initMap } from "./map";
import { initRouteMap } from "./routes";
import { initFilters } from "./filters";
import { initUnits } from "./units";
import { initInstruments } from "./instruments";
import { initAbstracts } from "./abstracts";
import { initEmailLinks } from "./email";
document.addEventListener("DOMContentLoaded", () => {
    initEmailLinks();
    initUnits();
    enhanceJourneyPins();
    const queue = window.__MAP_INIT__ || [];
    queue.forEach((cfg) => initMap(cfg));

    const routeQueue = window.__ROUTE_MAP_INIT__ || [];
    routeQueue.forEach((cfg) => initRouteMap(cfg));

    initFilters();
    initInstruments();
    initAbstracts();
});
