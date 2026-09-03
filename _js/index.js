import "normalize.css";
import "bootstrap/dist/css/bootstrap.min.css";
import "jquery";
import "bootstrap/dist/js/bootstrap.min.js";

import { library, dom } from "@fortawesome/fontawesome-svg-core";
import { fas } from "@fortawesome/free-solid-svg-icons";
import { far } from "@fortawesome/free-regular-svg-icons";
import { fab } from "@fortawesome/free-brands-svg-icons";
library.add(fas, far, fab);
dom.watch();

import { enhanceJourneyPins, initMap } from "./map";
import { initRouteMap } from "./routes";
import { initPublicationsChart } from "./publications";
import { initFilters } from "./filters";
import { initUnits } from "./units";
document.addEventListener("DOMContentLoaded", () => {
    initUnits();
    enhanceJourneyPins();
    const queue = window.__MAP_INIT__ || [];
    queue.forEach((cfg) => initMap(cfg));

    const routeQueue = window.__ROUTE_MAP_INIT__ || [];
    routeQueue.forEach((cfg) => initRouteMap(cfg));

    initPublicationsChart();
    initFilters();
});
