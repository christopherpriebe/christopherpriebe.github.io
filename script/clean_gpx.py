#!/usr/bin/env python3
"""Clean a recorded GPX track so it draws well on the routes map.

A watch recording (one fix a second) has three problems that a planned
course file does not:

1. Outliers. Tunnels, overpasses and tall buildings make the receiver jump
   tens of metres and then snap back.
2. Jitter. Every fix wanders a few metres either side of the true path, which
   draws as a fuzzy line and inflates the distance by a few percent.
3. Density. Thousands of points on straight roads cost bytes and add nothing.

Each has its own stage:

1. A constant-velocity Kalman filter runs forward over the fixes. A fix whose
   innovation is improbable under the filter's own uncertainty (a
   chi-squared gate) is treated as missing rather than trusted. While fixes
   are being rejected the prediction's uncertainty grows, so a genuine
   change in position is accepted again after a few seconds.
2. A Rauch-Tung-Striebel pass runs backward over the filter output, so every
   point is estimated from the fixes both before and after it. This removes
   jitter without the lag of a one-sided filter.
3. Ramer-Douglas-Peucker drops every point that lies within a tolerance of
   the line between the points kept around it.

Tracks without timestamps (planned courses) skip the first two stages and are
only simplified.

Usage:
    script/clean_gpx.py assets/gpx/route.gpx --in-place
    script/clean_gpx.py raw.gpx -o assets/gpx/route.gpx --tolerance 2
"""

import argparse
import math
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from datetime import datetime
from typing import List, Optional, Sequence, Tuple

GPX_NS = "http://www.topografix.com/GPX/1/1"
NS = {"gpx": GPX_NS}
EARTH_RADIUS_M = 6371008.8

# 99.9th percentile of chi-squared with two degrees of freedom.
GATE_2D = 13.82


@dataclass
class Point:
    lat: float
    lon: float
    ele: Optional[float]
    time: Optional[datetime]


@dataclass
class Track:
    name: Optional[str]
    points: List[Point]


# ---------------------------------------------------------------------------
# GPX input and output
# ---------------------------------------------------------------------------


def parse_time(text: str) -> datetime:
    return datetime.fromisoformat(text.strip().replace("Z", "+00:00"))


def read_gpx(path: str) -> Track:
    root = ET.parse(path).getroot()
    name_el = root.find("gpx:trk/gpx:name", NS)
    if name_el is None:
        name_el = root.find("gpx:metadata/gpx:name", NS)

    points = []
    # Segments are joined: the map draws one line per route anyway.
    elements = root.findall(".//gpx:trkpt", NS) or root.findall(".//gpx:rtept", NS)
    for el in elements:
        ele = el.find("gpx:ele", NS)
        time = el.find("gpx:time", NS)
        points.append(Point(
            lat=float(el.get("lat")),
            lon=float(el.get("lon")),
            ele=float(ele.text) if ele is not None and ele.text else None,
            time=parse_time(time.text) if time is not None and time.text else None,
        ))
    if len(points) < 2:
        raise ValueError(f"{path}: needs at least two track points, found {len(points)}")
    return Track(name=name_el.text if name_el is not None else None, points=points)


def write_gpx(track: Track, path: str, keep_time: bool) -> None:
    ET.register_namespace("", GPX_NS)
    root = ET.Element(f"{{{GPX_NS}}}gpx", {"version": "1.1", "creator": "clean_gpx.py"})
    trk = ET.SubElement(root, f"{{{GPX_NS}}}trk")
    if track.name:
        ET.SubElement(trk, f"{{{GPX_NS}}}name").text = track.name
    seg = ET.SubElement(trk, f"{{{GPX_NS}}}trkseg")

    for p in track.points:
        # Six decimals is about 0.1 m, well under GPS accuracy.
        pt = ET.SubElement(seg, f"{{{GPX_NS}}}trkpt", {"lat": f"{p.lat:.6f}", "lon": f"{p.lon:.6f}"})
        if p.ele is not None:
            ET.SubElement(pt, f"{{{GPX_NS}}}ele").text = f"{p.ele:.1f}"
        if keep_time and p.time is not None:
            ET.SubElement(pt, f"{{{GPX_NS}}}time").text = p.time.strftime("%Y-%m-%dT%H:%M:%SZ")

    tree = ET.ElementTree(root)
    ET.indent(tree, space=" ")
    tree.write(path, encoding="UTF-8", xml_declaration=True)
    with open(path, "a", encoding="utf-8") as f:
        f.write("\n")


# ---------------------------------------------------------------------------
# Geometry
# ---------------------------------------------------------------------------


class LocalProjection:
    """Equirectangular projection to metres around a reference point.

    Accurate to well under a metre over the few tens of kilometres a race
    covers, and trivially invertible.
    """

    def __init__(self, lat0: float, lon0: float):
        self.lat0 = lat0
        self.lon0 = lon0
        self.k = math.cos(math.radians(lat0))

    def forward(self, lat: float, lon: float) -> Tuple[float, float]:
        x = math.radians(lon - self.lon0) * self.k * EARTH_RADIUS_M
        y = math.radians(lat - self.lat0) * EARTH_RADIUS_M
        return x, y

    def inverse(self, x: float, y: float) -> Tuple[float, float]:
        lat = self.lat0 + math.degrees(y / EARTH_RADIUS_M)
        lon = self.lon0 + math.degrees(x / (EARTH_RADIUS_M * self.k))
        return lat, lon


def haversine_m(a: Point, b: Point) -> float:
    p1, p2 = math.radians(a.lat), math.radians(b.lat)
    dp, dl = p2 - p1, math.radians(b.lon - a.lon)
    h = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 2 * EARTH_RADIUS_M * math.asin(math.sqrt(h))


def track_length_m(points: Sequence[Point]) -> float:
    return sum(haversine_m(points[i], points[i + 1]) for i in range(len(points) - 1))


# ---------------------------------------------------------------------------
# Kalman filter and RTS smoother
# ---------------------------------------------------------------------------
#
# Each axis is modelled independently as [position, velocity] with white-noise
# acceleration. The axes share dt, process noise and measurement noise, so
# they share one 2x2 covariance, and a single set of gains serves all of them.
# Matrices are ((a, b), (c, d)) tuples; numpy would be overkill for 2x2.


def _transition(dt: float):
    return ((1.0, dt), (0.0, 1.0))


def _process_noise(dt: float, q: float):
    return ((q * dt ** 3 / 3, q * dt ** 2 / 2), (q * dt ** 2 / 2, q * dt))


def _mat_mul(a, b):
    return (
        (a[0][0] * b[0][0] + a[0][1] * b[1][0], a[0][0] * b[0][1] + a[0][1] * b[1][1]),
        (a[1][0] * b[0][0] + a[1][1] * b[1][0], a[1][0] * b[0][1] + a[1][1] * b[1][1]),
    )


def _mat_t(a):
    return ((a[0][0], a[1][0]), (a[0][1], a[1][1]))


def _mat_add(a, b):
    return ((a[0][0] + b[0][0], a[0][1] + b[0][1]), (a[1][0] + b[1][0], a[1][1] + b[1][1]))


def _mat_inv(a):
    det = a[0][0] * a[1][1] - a[0][1] * a[1][0]
    return ((a[1][1] / det, -a[0][1] / det), (-a[1][0] / det, a[0][0] / det))


def _mat_vec(a, v):
    return (a[0][0] * v[0] + a[0][1] * v[1], a[1][0] * v[0] + a[1][1] * v[1])


def rts_smooth(
    times: Sequence[float],
    series: Sequence[Sequence[float]],
    meas_sigma: float,
    accel_q: float,
    gate: Optional[float],
) -> Tuple[List[List[float]], List[bool]]:
    """Smooth one or more position series sampled at `times`.

    Returns the smoothed series and, per sample, whether the measurement was
    accepted. With `gate` set, a measurement whose normalised innovation
    squared (summed over the series) exceeds it is skipped.
    """
    n = len(times)
    dims = len(series)
    r = meas_sigma ** 2

    x_pred, p_pred, x_filt, p_filt, accepted = [], [], [], [], []

    # Start at the first fix, at rest, with velocity barely constrained.
    x = [(series[d][0], 0.0) for d in range(dims)]
    p = ((r, 0.0), (0.0, 25.0))

    for k in range(n):
        if k > 0:
            dt = max(times[k] - times[k - 1], 0.0)
            f = _transition(dt)
            x = [_mat_vec(f, xd) for xd in x]
            p = _mat_add(_mat_mul(_mat_mul(f, p), _mat_t(f)), _process_noise(dt, accel_q))
        x_pred.append(x)
        p_pred.append(p)

        s = p[0][0] + r
        innovations = [series[d][k] - x[d][0] for d in range(dims)]
        nis = sum(i * i for i in innovations) / s
        ok = k == 0 or gate is None or nis <= gate
        if ok:
            gain = (p[0][0] / s, p[1][0] / s)
            x = [(xd[0] + gain[0] * i, xd[1] + gain[1] * i) for xd, i in zip(x, innovations)]
            p = (
                ((1 - gain[0]) * p[0][0], (1 - gain[0]) * p[0][1]),
                (p[1][0] - gain[1] * p[0][0], p[1][1] - gain[1] * p[0][1]),
            )
        x_filt.append(x)
        p_filt.append(p)
        accepted.append(ok)

    out = [[0.0] * n for _ in range(dims)]
    for d in range(dims):
        out[d][-1] = x_filt[-1][d][0]
    x_next = x_filt[-1]
    for k in range(n - 2, -1, -1):
        f = _transition(max(times[k + 1] - times[k], 0.0))
        c = _mat_mul(_mat_mul(p_filt[k], _mat_t(f)), _mat_inv(p_pred[k + 1]))
        x_next = [
            tuple(a + b for a, b in zip(
                x_filt[k][d],
                _mat_vec(c, (x_next[d][0] - x_pred[k + 1][d][0], x_next[d][1] - x_pred[k + 1][d][1])),
            ))
            for d in range(dims)
        ]
        for d in range(dims):
            out[d][k] = x_next[d][0]
    return out, accepted


# ---------------------------------------------------------------------------
# Simplification
# ---------------------------------------------------------------------------


def _segment_distance(px, py, ax, ay, bx, by) -> float:
    dx, dy = bx - ax, by - ay
    length_sq = dx * dx + dy * dy
    if length_sq == 0:
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / length_sq))
    return math.hypot(px - (ax + t * dx), py - (ay + t * dy))


def douglas_peucker(xs: Sequence[float], ys: Sequence[float], tolerance: float) -> List[int]:
    """Return the indices kept by Ramer-Douglas-Peucker, in order."""
    keep = [False] * len(xs)
    keep[0] = keep[-1] = True
    stack = [(0, len(xs) - 1)]  # Iterative: 1 Hz tracks are too deep to recurse.
    while stack:
        start, end = stack.pop()
        worst, worst_i = 0.0, -1
        for i in range(start + 1, end):
            d = _segment_distance(xs[i], ys[i], xs[start], ys[start], xs[end], ys[end])
            if d > worst:
                worst, worst_i = d, i
        if worst > tolerance:
            keep[worst_i] = True
            stack.append((start, worst_i))
            stack.append((worst_i, end))
    return [i for i, k in enumerate(keep) if k]


# ---------------------------------------------------------------------------
# Pipeline
# ---------------------------------------------------------------------------


@dataclass
class Report:
    points_in: int
    points_out: int
    rejected: int
    length_in_m: float
    length_smoothed_m: float
    length_out_m: float
    smoothed: bool


def clean(track: Track, gps_sigma: float, accel_q: float, tolerance: float) -> Tuple[Track, Report]:
    pts = track.points
    proj = LocalProjection(pts[0].lat, pts[0].lon)
    xy = [proj.forward(p.lat, p.lon) for p in pts]
    xs = [v[0] for v in xy]
    ys = [v[1] for v in xy]

    timed = all(p.time is not None for p in pts)
    rejected = 0
    smoothed_pts = pts
    if timed:
        t0 = pts[0].time
        times = [(p.time - t0).total_seconds() for p in pts]
        (xs, ys), accepted = rts_smooth(times, [xs, ys], gps_sigma, accel_q, GATE_2D)
        rejected = accepted.count(False)

        eles = [p.ele for p in pts]
        if all(e is not None for e in eles):
            # Barometric altitude is far steadier than GPS position, so it
            # gets a tighter measurement noise and no gate.
            (eles,), _ = rts_smooth(times, [eles], 1.0, 0.01, None)

        smoothed_pts = []
        for i, p in enumerate(pts):
            lat, lon = proj.inverse(xs[i], ys[i])
            smoothed_pts.append(Point(lat, lon, eles[i], p.time))

    kept = douglas_peucker(xs, ys, tolerance)
    out_pts = [smoothed_pts[i] for i in kept]

    report = Report(
        points_in=len(pts),
        points_out=len(out_pts),
        rejected=rejected,
        length_in_m=track_length_m(pts),
        length_smoothed_m=track_length_m(smoothed_pts),
        length_out_m=track_length_m(out_pts),
        smoothed=timed,
    )
    return Track(name=track.name, points=out_pts), report


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("input", help="GPX file to clean")
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("-o", "--output", help="where to write the cleaned GPX")
    target.add_argument("--in-place", action="store_true", help="overwrite the input file")
    parser.add_argument("--gps-noise", type=float, default=4.0, metavar="M",
                        help="standard deviation of a GPS fix, in metres (default: 4)")
    parser.add_argument("--accel", type=float, default=0.5, metavar="Q",
                        help="acceleration noise, in m^2/s^3; higher follows sharp turns "
                             "more closely but smooths less (default: 0.5)")
    parser.add_argument("--tolerance", type=float, default=2.0, metavar="M",
                        help="simplification tolerance, in metres (default: 2)")
    parser.add_argument("--keep-time", action="store_true",
                        help="keep timestamps (the map does not use them)")
    args = parser.parse_args(argv)

    track = read_gpx(args.input)
    cleaned, report = clean(track, args.gps_noise, args.accel, args.tolerance)
    write_gpx(cleaned, args.input if args.in_place else args.output, args.keep_time)

    print(f"points    {report.points_in} -> {report.points_out}")
    print(f"distance  {report.length_in_m / 1000:.2f} km -> {report.length_out_m / 1000:.2f} km")
    if report.smoothed:
        print(f"rejected  {report.rejected} outlier fixes")
    else:
        print("no timestamps: simplified only, not smoothed", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
