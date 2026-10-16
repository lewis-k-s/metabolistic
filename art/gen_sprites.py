#!/usr/bin/env python3
"""Draft sprites for the vent stage, drawn with code.

Run:  uv run --with numpy art/gen_sprites.py

Output:
  assets/sprites/*.png     one native-size PNG for each sprite (no scaling)
  docs/art/contact_sheet.png   all sprites, scaled up, with names
  docs/art/vent_scene.png      a top-down mock of the Run screen
  assets/sprites/anim/*.png    animation sheets (idle and move) and manifest.json
  docs/art/anim/*              previews of the animations, with a GIF

The sprites are pixel art. Every shape is made from a signed distance field,
shaded as a lit ball with 4 tones, then outlined. All colours come from the
RAMPS table below, so the whole set shares one palette. See
docs/art-direction.md for the rules.
"""

from __future__ import annotations

import json
import shutil
import struct
import subprocess
import tempfile
import zlib
from dataclasses import dataclass, replace
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
SPRITE_DIR = ROOT / "assets" / "sprites"
PREVIEW_DIR = ROOT / "docs" / "art"
ANIM_DIR = SPRITE_DIR / "anim"


# --------------------------------------------------------------------------
# Palette. Each ramp has 5 tones: outline, shadow, base, light, highlight.
# --------------------------------------------------------------------------


def hx(code: str) -> tuple[int, int, int]:
    code = code.lstrip("#")
    return tuple(int(code[i : i + 2], 16) for i in (0, 2, 4))  # type: ignore[return-value]


def ramp(*codes: str) -> list[tuple[int, int, int]]:
    return [hx(c) for c in codes]


RAMPS = {
    # Cells. One hue for each role, so the player can read a cell at a glance.
    "player": ramp("#17352f", "#3c8a6e", "#78d49a", "#b4f0b8", "#f0ffe0"),  # mint
    "prey": ramp("#1b1f4a", "#3a4aa0", "#6f86e0", "#a9bcff", "#e6edff"),  # periwinkle
    "predator": ramp("#3a0f1c", "#8a1f33", "#d1394a", "#f2715f", "#ffc2a0"),  # crimson
    "symbiont": ramp("#4a2e0c", "#b0701c", "#f0b030", "#ffd866", "#fff6c0"),  # gold
    "gate": ramp("#0f3a3e", "#1f7a73", "#34b8a0", "#7ae6c0", "#d0fff0"),  # teal
    "proteo": ramp("#3a1030", "#8a2a6e", "#c8509c", "#f08cc8", "#ffe0f2"),  # rose: the future mitochondrion
    "pigment": ramp("#7a2a0a", "#d0541a", "#f08a28", "#ffc060", "#ffe8a0"),  # carotenoid
    "virus": ramp("#2b1a4a", "#5b3d9a", "#8f6fd8", "#bda2f5", "#efe6ff"),  # lilac
    # Inside a cell.
    "nucleoid": ramp("#2c1a40", "#6a3f8f", "#a56bc4", "#d6a2ea", "#f6dcff"),
    "bead": ramp("#5a4a1a", "#c8a838", "#f0dc78", "#fff4b0", "#ffffff"),  # lipid bead
    "granule": ramp("#10261f", "#2a5c48", "#3c8a6e", "#78d49a", "#b4f0b8"),
    # Things you can take in.
    "carbon": ramp("#3a2210", "#7a4a22", "#b87a3a", "#e0a860", "#fbe0a0"),
    "mineral": ramp("#0c3550", "#1d7aa8", "#3ec0e0", "#92ecff", "#eaffff"),
    "metal": ramp("#3d3010", "#8a7020", "#d4b030", "#f4e068", "#fffbd0"),  # Fe-S
    # World.
    "rock": ramp("#070a12", "#101626", "#1b2438", "#2b3650", "#44506c"),
    "vent": ramp("#5a1408", "#b02c0c", "#ee6a14", "#ffb030", "#fff0a0"),
    "head": ramp("#5a2030", "#c06a7c", "#ffb0a0", "#ffd0c0", "#fff0e8"),  # lipid head
    "tail": ramp("#2a0f1c", "#4a1a34", "#7a3050", "#a04a60", "#c06a7c"),
    "protein": ramp("#1a1a4a", "#3a3a8a", "#5a62c8", "#8c92f0", "#c8ccff"),
    # Membrane proteins. Flagellum = protein (blue), adhesin = magenta, toxin = acid green.
    "adhesin": ramp("#3a1038", "#8a2a78", "#d050b8", "#f58ae0", "#ffd0f4"),
    "toxin": ramp("#1c3a0a", "#3f7a14", "#78c02a", "#b4ee54", "#eaffa8"),
    "lytic": ramp("#10141c", "#3a4660", "#7a8aa8", "#b8c8e0", "#f0f8ff"),  # steel
    "fusion": ramp("#3a3224", "#8a7a5a", "#c8b890", "#ecdcb8", "#fffaee"),  # ivory
    "receptor": ramp("#0e3050", "#2670b0", "#4aa4e8", "#92d0ff", "#e4f6ff"),  # sky
    "immune": ramp("#2a2630", "#6c6676", "#b8b2c0", "#e8e4ee", "#ffffff"),  # pearl: the colour is in the antigen
    # Metabolism view: internal pools. Carbon, Minerals and Metals keep their resource
    # ramps above, and Lipids use "head" and "tail". Gold-white stays for the gene reward.
    "atp": ramp("#2c3200", "#6c7e08", "#b8cc18", "#e2f058", "#faffb0"),  # lime: energy
    "reducing": ramp("#062a6a", "#0e5ee0", "#2a90ff", "#7cc4ff", "#e0f4ff"),  # electric blue: electrons
    "pyruvate": ramp("#0a3440", "#1a6a78", "#40a8b0", "#8adcd8", "#e0fffa"),  # muted aqua
    "amino": ramp("#1c3018", "#3c6a30", "#64a050", "#a0d088", "#e0f8c8"),  # sage
    "nucleotide": ramp("#2e1038", "#6a2e8a", "#a458d0", "#d490f0", "#f4d8ff"),  # orchid
    "waste": ramp("#1a1612", "#403a30", "#6a6252", "#948a74", "#c0b8a0"),  # murky olive-grey
    "o2": ramp("#0a3a34", "#1a7a64", "#2ec09a", "#7ae6c4", "#d8fff0"),  # sea green: oxygen
    "ros": ramp("#3a0a3a", "#9a1a8a", "#e03ac8", "#ff80e8", "#ffe0fa"),  # hot magenta: a radical (never perforin red)
}

# Deep-water backdrop, dark to light.
WATER = ramp("#04060d", "#070c1a", "#0a1226", "#0e1a33", "#13243f")

BAYER4 = (
    np.array(
        [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], dtype=float
    )
    + 0.5
) / 16.0

LIGHT = np.array([-0.45, -0.55, 0.70])
LIGHT = LIGHT / np.linalg.norm(LIGHT)


@dataclass(frozen=True)
class Pose:
    """How a sprite looks in one frame.

    t:       phase of the loop, 0 <= t < 1. Every wave uses whole turns of 2*pi*t,
             so the loop is seamless.
    heading: direction of travel in radians (screen angle, +y down). None = at rest.
    stretch: length change along the heading (0.1 = 10 % longer). The width
             shrinks so the area stays about the same.
    taper:   how much the back end narrows (0..1). The front stays round.
    lag:     offset of the organelles in design pixels. They trail the body.
    scale:   uniform size. Used by the spawn animation.
    shift:   move of the body along the heading, in px (+ = forwards).
    cut:     a flat wall in front of the cell, `cut` px from its centre along the
             heading. Everything beyond it is cut off. This squashes a cell flat
             against a wall.
    """

    t: float = 0.0
    heading: float | None = None
    stretch: float = 0.0
    taper: float = 0.0
    lag: tuple[float, float] = (0.0, 0.0)
    scale: float = 1.0
    shift: float = 0.0
    cut: float | None = None


REST = Pose()


# --------------------------------------------------------------------------
# Canvas
# --------------------------------------------------------------------------


class Canvas:
    def __init__(self, w: int, h: int, origin: tuple[float, float] = (0, 0)):
        """origin: the pixel position of design point (0, 0). Sprites are drawn
        in design coordinates, so animation frames can add room around them."""
        self.w, self.h = w, h
        self.ox, self.oy = origin
        self.px = np.zeros((h, w, 4), dtype=np.uint8)
        yy, xx = np.mgrid[0:h, 0:w]
        self.X = xx + 0.5 - self.ox  # pixel centres, in design coordinates
        self.Y = yy + 0.5 - self.oy
        self._fwd = None

    def set_pose(self, pose: Pose, cx: float, cy: float, radius: float) -> None:
        """Deform everything drawn after this call.

        X and Y become the rest-shape coordinates of each pixel, so every shape
        that is drawn from X and Y (discs, lines, glow) deforms with the body.
        `spark` and `dot` map their points forward in the same way.
        Order, from the rest shape: stretch and taper, shift, scale, wall cut.
        """
        ang = pose.heading if pose.heading is not None else 0.0
        d0, d1 = np.cos(ang), np.sin(ang)
        s, a, sh, sc, cut = pose.stretch, pose.taper, pose.shift, pose.scale, pose.cut
        k = (1 + s) ** -0.5

        def width(u_rest):
            return 1 - a * np.clip(-u_rest / radius, 0, 1)

        def inverse(X, Y):
            dx, dy = X - cx, Y - cy
            beyond = None if cut is None else (dx * d0 + dy * d1) > cut
            dx, dy = dx / sc, dy / sc
            u, v = dx * d0 + dy * d1 - sh, -dx * d1 + dy * d0
            ur = u / (1 + s)
            vr = v / (width(ur) * k)
            if beyond is not None:
                ur = np.where(beyond, 1e4, ur)  # far outside the shape
            return cx + ur * d0 - vr * d1, cy + ur * d1 + vr * d0

        def forward(x, y):
            dx, dy = x - cx, y - cy
            ur, vr = dx * d0 + dy * d1, -dx * d1 + dy * d0
            u, v = ur * (1 + s) + sh, vr * width(ur) * k
            return cx + sc * (u * d0 - v * d1), cy + sc * (u * d1 + v * d0)

        self.X, self.Y = inverse(self.X, self.Y)
        self._fwd = forward

    # -- masks ------------------------------------------------------------
    @staticmethod
    def dilate(mask: np.ndarray) -> np.ndarray:
        out = mask.copy()
        out[1:] |= mask[:-1]
        out[:-1] |= mask[1:]
        out[:, 1:] |= mask[:, :-1]
        out[:, :-1] |= mask[:, 1:]
        return out

    @property
    def filled(self) -> np.ndarray:
        return self.px[..., 3] > 0

    # -- painting ---------------------------------------------------------
    def paint(self, mask: np.ndarray, color) -> None:
        self.px[mask] = (*color, 255)

    def outline(self, color) -> None:
        """Add a 1 px outline around everything drawn so far."""
        body = self.filled
        self.paint(self.dilate(body) & ~body, color)

    def blit(self, other: "Canvas", x: int, y: int) -> None:
        """Copy `other` onto this canvas. Pixels are on or off, no blending."""
        for oy in range(other.h):
            for ox in range(other.w):
                tx, ty = x + ox, y + oy
                if 0 <= tx < self.w and 0 <= ty < self.h and other.px[oy, ox, 3]:
                    self.px[ty, tx] = other.px[oy, ox]

    def lit_body(self, sdf, tones, band=None, band_tones=None, width=None) -> np.ndarray:
        """Fill a shape (sdf < 0) as a lit ball. Returns the mask.

        tones: ramp. The ball uses tones[1..3]. `band` is the thickness of the
        membrane rim in pixels. The rim uses band_tones, one step lighter.
        """
        mask = sdf < 0
        depth = np.where(mask, -sdf, 0.0)
        w = width if width is not None else max(depth.max(), 1.0)
        t = 1.0 - np.clip(depth / w, 0, 1)
        h = np.sqrt(np.clip(1 - t * t, 0, 1))
        gy, gx = np.gradient(h)
        normal = np.stack([-gx * 2.2, -gy * 2.2, np.ones_like(h)], axis=-1)
        normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
        lam = normal @ LIGHT
        # Four bands: shadow, shadow, base, light.
        idx = np.digitize(lam, [0.55, 0.72, 0.86])  # 0..3
        palette = np.array(tones[1:5])  # shadow, base, light, highlight -> use 0..2
        body = palette[np.minimum(idx, 2) if band is None else np.minimum(idx, 2)]
        out = np.zeros((self.h, self.w, 3), dtype=np.uint8)
        out[...] = body
        if band is not None:
            rim = np.array((band_tones or tones)[1:5])
            rim_idx = np.minimum(idx + 1, 3)
            rim_col = rim[rim_idx]
            in_band = mask & (depth < band)
            out[in_band] = rim_col[in_band]
        self.px[mask, :3] = out[mask]
        self.px[mask, 3] = 255
        return mask

    def disc(self, cx, cy, r, tones, outline=True) -> None:
        """A small ball: used for organelles and granules."""
        d = np.hypot(self.X - cx, self.Y - cy)
        mask = d < r
        if outline:
            self.paint(self.dilate(mask) & ~mask & ~self.filled, tones[0])
        self.paint(mask, tones[2])
        shadow = mask & ((self.X - cx) * 0.55 + (self.Y - cy) * 0.65 > 0.25 * r)
        self.paint(shadow, tones[1])
        if r >= 2.0:
            hi = mask & (np.hypot(self.X - (cx - r * 0.35), self.Y - (cy - r * 0.4)) < max(0.9, r * 0.32))
            self.paint(hi, tones[4])

    def line(self, x0, y0, x1, y1, color, thick=0.6) -> None:
        px, py = self.X - x0, self.Y - y0
        dx, dy = x1 - x0, y1 - y0
        t = np.clip((px * dx + py * dy) / (dx * dx + dy * dy + 1e-9), 0, 1)
        d = np.hypot(px - t * dx, py - t * dy)
        self.paint(d < thick, color)

    def poly(self, points) -> np.ndarray:
        """Mask of a convex polygon (points in order)."""
        inside = np.ones((self.h, self.w), dtype=bool)
        n = len(points)
        for i in range(n):
            ax, ay = points[i]
            bx, by = points[(i + 1) % n]
            cross = (bx - ax) * (self.Y - ay) - (by - ay) * (self.X - ax)
            inside &= cross >= 0
        return inside

    def halo(self, cx, cy, r_in, r_out, tones, strength=1.0) -> None:
        """A dithered glow behind the body. Pixels are on or off."""
        d = np.hypot(self.X - cx, self.Y - cy)
        a = np.clip(1 - (d - r_in) / (r_out - r_in), 0, 1) * strength
        bayer = np.tile(BAYER4, (self.h // 4 + 1, self.w // 4 + 1))[: self.h, : self.w]
        on = (a > bayer) & (d >= r_in * 0.6) & ~self.filled
        level = np.clip((a * (len(tones) - 1)).astype(int), 0, len(tones) - 1)
        for i, col in enumerate(tones):
            self.paint(on & (level == i), col)

    def dot(self, x: float, y: float, color) -> None:
        """One pixel at a point in design coordinates (it follows the pose)."""
        if self._fwd is not None:
            x, y = self._fwd(x, y)
        ix, iy = int(np.floor(x + self.ox + 1e-6)), int(np.floor(y + self.oy + 1e-6))  # 1e-6: a point on an edge stays put
        if 0 <= ix < self.w and 0 <= iy < self.h:
            self.px[iy, ix] = (*color, 255)

    def plot(self, x: float, y: float, color) -> None:
        """One pixel at a point in design coordinates, without the pose."""
        ix, iy = int(np.floor(x + self.ox + 1e-6)), int(np.floor(y + self.oy + 1e-6))  # 1e-6: a point on an edge stays put
        if 0 <= ix < self.w and 0 <= iy < self.h:
            self.px[iy, ix] = (*color, 255)

    def spark(self, points, color) -> None:
        """One pixel for each (x, y) point. Points are pixel indices of the rest shape."""
        for x, y in points:
            self.dot(x + 0.5, y + 0.5, color)


def blob_sdf(c: Canvas, cx, cy, radius, harmonics=(), sx=1.0, sy=1.0, spikes=0, spike_amp=0.0,
             spike_angle=0.0):
    """Distance to a wobbly disc. Positive outside, negative inside."""
    dx, dy = (c.X - cx) / sx, (c.Y - cy) / sy
    theta = np.arctan2(dy, dx)
    r = radius * (1 + sum(a * np.sin(k * theta + p) for k, a, p in harmonics))
    if spikes:
        # Peaks sit either side of `spike_angle`, so a mouth there has a valley.
        r = r * (0.78 + spike_amp * np.abs(np.cos(spikes * (theta - spike_angle + np.pi / spikes) / 2)) ** 6)
    return np.hypot(dx, dy) - r


def capsule_sdf(c: Canvas, x0, y0, x1, y1, radius):
    px, py = c.X - x0, c.Y - y0
    dx, dy = x1 - x0, y1 - y0
    t = np.clip((px * dx + py * dy) / (dx * dx + dy * dy), 0, 1)
    return np.hypot(px - t * dx, py - t * dy) - radius


# --------------------------------------------------------------------------
# Sprites
# --------------------------------------------------------------------------


def _canvas(size, anim: bool, pad: int, origin=None) -> Canvas:
    """A canvas for one sprite. Animation frames get extra room for the deform."""
    if not anim:
        return Canvas(*size)
    if origin is not None:
        return Canvas(*size, origin=origin)
    return Canvas(size[0] + 2 * pad, size[1] + 2 * pad, origin=(pad, pad))


def player_cell(pose: Pose = REST, anim: bool = False) -> Canvas:
    """The player. A mint blob with a visible membrane rim and a few organelles."""
    c = _canvas((48, 48), anim, 8)
    ph = 2 * np.pi * pose.t
    c.set_pose(pose, 24, 24.5, 20)
    lx, ly = pose.lag

    def org(x, y, amp, phase, k=1.0):
        """An organelle: it drifts a little and lags behind when the cell moves."""
        return x + amp * np.sin(ph + phase) + lx * k, y + amp * np.cos(ph + phase * 1.3) + ly * k

    sdf = blob_sdf(
        c, 24, 24.5, 20 * (1 + 0.012 * np.sin(ph)), [(2, 0.055, 0.6 + ph), (3, 0.045, 2.1 - ph)]
    )
    c.lit_body(sdf, RAMPS["player"], band=2.6, width=15)
    # Nucleoid: a loose, irregular clump.
    nx, ny = org(21.5, 23, 0.9, 0.0, 1.0)
    n = blob_sdf(c, nx, ny, 5.2, [(2, 0.18, 0.3 + ph), (3, 0.14, 1.7 - 2 * ph)])
    inner = (n < 0) & (sdf < -4)
    c.paint(c.dilate(inner) & ~inner & (sdf < -3), RAMPS["nucleoid"][0])
    c.lit_body(np.where(inner, n, 1), RAMPS["nucleoid"], width=5)
    c.dot(nx - 2.0, ny - 2.5, RAMPS["nucleoid"][4])
    c.dot(nx - 1.0, ny - 2.5, RAMPS["nucleoid"][4])
    # A storage bead, a few ribosome dots.
    bx, by = org(31.5, 28.5, 0.7, 1.3, 0.8)
    c.disc(bx, by, 3.4, RAMPS["bead"])
    for (x, y, r, a, p, k) in [
        (15.5, 31, 1.6, 0.6, 2.0, 0.5), (28.5, 16.5, 1.5, 0.7, 3.1, 0.6),
        (21, 34.5, 1.3, 0.6, 4.2, 0.5), (33.5, 21, 1.2, 0.5, 5.0, 0.4),
    ]:
        gx, gy = org(x, y, a, p, k)
        c.disc(gx, gy, r, RAMPS["granule"])
    # Specular highlight on the membrane.
    c.spark([(12, 13), (13, 12), (14, 11), (12, 14), (11, 15)], RAMPS["player"][4])
    c.outline(RAMPS["player"][0])
    return c


# Prey: the rod, its axis and its flagellum.
PREY_CENTRE = (13.75, 11.0)
PREY_REST_ANGLE = float(np.arctan2(-6, 10.5))  # points at the flagellum


def prey_bacillus(pose: Pose = REST, anim: bool = False) -> Canvas:
    """Prey. A small rod with a flagellum. Blue, so it reads as cold and harmless.

    The rod swims away from its flagellum. When it moves, it turns to face the
    heading and the flagellum beats faster.
    """
    moving = pose.heading is not None
    axis_angle = pose.heading + np.pi if moving else PREY_REST_ANGLE
    ph = 2 * np.pi * pose.t
    if not moving:
        axis_angle += 0.07 * np.sin(ph)
    if anim:  # room to turn the rod in any direction, centred on the rod
        c = Canvas(48, 48, origin=(24 - PREY_CENTRE[0], 24 - PREY_CENTRE[1]))
    else:
        c = Canvas(28, 22)
    cx, cy = PREY_CENTRE
    c.set_pose(replace(pose, heading=axis_angle + np.pi), cx, cy, 11)
    a = np.array([np.cos(axis_angle), np.sin(axis_angle)])  # towards the flagellum
    n = np.array([-a[1], a[0]])
    lag = np.array(pose.lag)
    half, rad = 6.05, 5.2 * (1 + 0.03 * np.sin(ph))
    p0, p1 = np.array([cx, cy]) - half * a, np.array([cx, cy]) + half * a
    sdf = capsule_sdf(c, p0[0], p0[1], p1[0], p1[1], rad)
    c.lit_body(sdf, RAMPS["prey"], band=1.8, width=5)
    ctr = np.array([cx, cy])
    nuc = ctr + 2.0 * a + lag
    gra = ctr - 3.3 * a + lag * 0.7
    c.disc(nuc[0], nuc[1], 1.5, RAMPS["nucleoid"], outline=False)
    c.disc(gra[0], gra[1], 1.1, RAMPS["granule"], outline=False)
    # Highlight: on the rim that faces the light, whatever the rod's angle.
    to_light = np.array([-0.6, -0.8])
    side = to_light - (to_light @ a) * a
    side = side / (np.linalg.norm(side) + 1e-9)
    hp = ctr + (to_light @ a) * 3.0 * a + side * 3.3
    for k in (-1, 0, 1):
        c.dot(*(hp + k * a), RAMPS["prey"][4])
    # Flagellum: a wave that runs away from the body.
    beats, amp = (2, 2.0) if moving else (1, 1.3)
    start = ctr + 9.4 * a
    last = start
    for i in range(1, 9):
        s = i * 0.8
        grow = min(1.0, s / 2.5)
        pt = start + a * s + n * amp * grow * np.sin(1.3 * s - beats * ph)
        c.line(last[0], last[1], pt[0], pt[1], RAMPS["prey"][3], 0.55)
        last = pt
    c.outline(RAMPS["prey"][0])
    return c


def predator_cell(pose: Pose = REST, anim: bool = False) -> Canvas:
    """Predator. A spiny crimson body with a toothed mouth that faces the heading.

    At rest the mouth faces right and chatters. When it moves, the mouth opens
    and points where the cell goes.
    """
    moving = pose.heading is not None
    ang = pose.heading if moving else 0.0
    ph = 2 * np.pi * pose.t
    c = _canvas((48, 48), anim, 8)
    c.set_pose(pose, 24, 24, 17)
    lx, ly = pose.lag
    sdf = blob_sdf(
        c, 24, 24, 17 * (1 + 0.015 * np.sin(ph)), [(2, 0.04, 1.0 + ph)],
        spikes=7, spike_amp=0.42 + 0.04 * np.sin(ph + 1.0), spike_angle=ang,
    )
    c.lit_body(sdf, RAMPS["predator"], band=2.4, width=11)
    # Dark nucleoid and granules.
    c.disc(19 + lx, 24 + ly, 4.6, RAMPS["nucleoid"])
    for (x, y, k) in [(15, 17, 0.6), (15, 31, 0.6), (24, 31.5, 0.5)]:
        c.disc(x + lx * k, y + ly * k, 1.4 if x < 20 else 1.2, RAMPS["vent"], outline=False)
    # Mouth: a dark notch with teeth. It opens and closes.
    opening = (0.9 + 0.1 * np.sin(ph)) if moving else (0.65 + 0.22 * np.sin(ph))
    width = 0.35 + 0.25 * opening
    ca, sa = np.cos(ang), np.sin(ang)
    u = (c.X - 24) * ca + (c.Y - 24) * sa
    v = -(c.X - 24) * sa + (c.Y - 24) * ca
    c.paint((sdf < 0) & (u > 3) & (np.abs(v) < (u - 2.0) * width), hx("#240510"))
    teeth = hx("#f6efe0")
    for tu in (5.0, 7.5, 10.0):
        for sign in (-1, 1):
            tv = sign * (tu - 2.0) * width * 0.96
            px_, py_ = 24 + tu * ca - tv * sa, 24 + tu * sa + tv * ca
            for k in (0.0, 1.0):  # a tooth is 2 px long, along the edge of the notch
                c.dot(px_ + k * (-sa) * -sign * 1.0, py_ + k * ca * -sign * 1.0, teeth)
    c.spark([(13, 12), (14, 11), (15, 11)], RAMPS["predator"][4])
    c.outline(RAMPS["predator"][0])
    return c


def symbiont_cell(pose: Pose = REST, anim: bool = False) -> Canvas:
    """Symbiont. Small, gold, with a warm glow that says 'friend'. The glow pulses."""
    c = _canvas((32, 32), anim, 8)
    ph = 2 * np.pi * pose.t
    c.set_pose(pose, 16, 16, 9.5)
    lx, ly = pose.lag
    sdf = blob_sdf(
        c, 16, 16, 9.5 * (1 + 0.03 * np.sin(ph)), [(2, 0.06, 0.2 + ph), (3, 0.05, 1.4 - ph)]
    )
    c.lit_body(sdf, RAMPS["symbiont"], band=2.0, width=8)
    c.disc(16 + 0.6 * np.sin(ph) + lx, 16.5 + 0.5 * np.cos(ph) + ly, 3.0, RAMPS["bead"])
    c.spark([(10, 10), (11, 9), (12, 9)], RAMPS["symbiont"][4])
    c.outline(RAMPS["symbiont"][0])
    c.halo(
        16, 16, 11, 15.5,
        [RAMPS["symbiont"][1], RAMPS["symbiont"][2], RAMPS["symbiont"][3]],
        0.55 + 0.15 * np.sin(ph),
    )
    return c


def gate_cell(pose: Pose = REST, anim: bool = False) -> Canvas:
    """Gate cell of the vent stage: a big pigmented phototroph.

    The orange rim is carotenoid pigment (the key innovation of this stage).
    The curved stripes inside are thylakoid membranes (the gene you win).
    It does not move. Its idle shows energy flowing in the stripes, and the
    halo pulses, so the player sees that this cell is the goal.
    """
    c = Canvas(72, 72)
    ph = 2 * np.pi * pose.t
    sdf = blob_sdf(
        c, 36, 36, 25 * (1 + 0.008 * np.sin(ph)),
        [(2, 0.03, 0.5 + ph), (3, 0.03, 2.4 - ph), (5, 0.015, 0.9)],
    )
    mask = c.lit_body(sdf, RAMPS["gate"], band=3.2, band_tones=RAMPS["pigment"], width=20)
    # Thylakoid stacks: concentric arcs around a far-away centre. They shift
    # by one period over the loop, so the stripes seem to flow.
    d = np.hypot(c.X - 70, c.Y - 36)
    inside = mask & (sdf < -6) & (c.X < 40) & (d > 18)
    stripes = inside & (((d + 4.0 * pose.t) % 4.0) < 1.15)
    c.paint(stripes, RAMPS["gate"][1])
    stripes2 = stripes & (c.Y < 36)
    c.paint(np.roll(stripes2, 1, axis=0) & inside & ~stripes, RAMPS["gate"][3])
    # Nucleoid and beads.
    c.disc(44 + 0.8 * np.sin(ph), 39 + 0.6 * np.cos(ph), 5.5, RAMPS["nucleoid"])
    c.disc(47 + 0.5 * np.sin(ph + 2), 28 + 0.5 * np.cos(ph + 2), 2.2, RAMPS["bead"])
    c.disc(40, 48, 1.6, RAMPS["granule"], outline=False)
    c.spark([(20, 20), (21, 19), (22, 18), (23, 18), (19, 21)], RAMPS["pigment"][4])
    c.outline(RAMPS["pigment"][0])
    pulse = 0.5 + 0.5 * np.sin(ph)
    c.halo(
        36, 36, 27, 33.5 + 2.0 * pulse,
        [RAMPS["vent"][1], RAMPS["vent"][2], RAMPS["vent"][3]], 0.5 + 0.18 * pulse,
    )
    return c


def gate_cell_proteobacterium(pose: Pose = REST, anim: bool = False) -> Canvas:
    """Gate cell of the shallow-water stage: an alpha-proteobacterium.

    The gene you win is respiration: this cell is the future mitochondrion. It is
    rose, so it does not look like the teal phototroph gate. The folds inside are
    cristae (the respiratory membrane). It does not move. Its idle shows the
    folds drifting and the halo pulsing, like the other gate cell.
    """
    c = Canvas(72, 72)
    ph = 2 * np.pi * pose.t
    sdf = blob_sdf(
        c, 36, 36, 25 * (1 + 0.008 * np.sin(ph)),
        [(2, 0.05, 0.5 + ph), (3, 0.025, 2.0 - ph), (4, 0.012, 0.3)],
    )
    mask = c.lit_body(sdf, RAMPS["proteo"], band=3.0, band_tones=RAMPS["proteo"], width=20)
    # Cristae: wavy folds across the inner body. They drift by one period.
    inside = mask & (sdf < -6)
    wave = 2.2 * np.sin(c.Y / 3.2 + ph)
    folds = inside & (((c.X + wave + 6.0 * pose.t) % 6.0) < 1.3) & (c.X < 46)
    c.paint(folds, RAMPS["proteo"][1])
    c.paint(np.roll(folds, 1, axis=1) & inside & ~folds, RAMPS["proteo"][3])
    # Nucleoid and a bead.
    c.disc(47 + 0.8 * np.sin(ph), 40 + 0.6 * np.cos(ph), 5.0, RAMPS["nucleoid"])
    c.disc(42 + 0.5 * np.sin(ph + 2), 26 + 0.5 * np.cos(ph + 2), 2.0, RAMPS["bead"])
    c.spark([(20, 20), (21, 19), (22, 18), (23, 18), (19, 21)], RAMPS["proteo"][4])
    c.outline(RAMPS["proteo"][0])
    pulse = 0.5 + 0.5 * np.sin(ph)
    c.halo(
        36, 36, 27, 33.5 + 2.0 * pulse,
        [RAMPS["proteo"][1], RAMPS["proteo"][2], RAMPS["proteo"][3]], 0.5 + 0.18 * pulse,
    )
    return c


def virus_phage(pose: Pose = REST, anim: bool = False) -> Canvas:
    """Virus, seen from above: a hexagon head and a tail with splayed legs.

    It lies flat and points to the right, like a T4 phage on a slide. At idle
    the legs twitch, as if the virus feels for a surface.
    """
    c = _canvas((44, 28), anim, 4)
    ph = 2 * np.pi * pose.t
    c.set_pose(pose, 22, 14, 20)
    hc = np.array([12.0, 14.0])
    R = 9.5
    pts = [hc + R * np.array([np.cos(a), np.sin(a)]) for a in np.radians(np.arange(6) * 60)]
    to_light = np.array([-0.6, -0.8])
    for i in range(6):
        tri = c.poly([tuple(hc), tuple(pts[i]), tuple(pts[(i + 1) % 6])])
        mid = (pts[i] + pts[(i + 1) % 6]) / 2 - hc
        lam = float(mid @ to_light) / np.linalg.norm(mid)
        tone = 3 if lam > 0.55 else 2 if lam > -0.2 else 1
        c.paint(tri, RAMPS["virus"][tone])
    for p in pts:
        c.line(hc[0], hc[1], p[0], p[1], RAMPS["virus"][0], 0.4)
    c.paint(np.hypot(c.X - hc[0], c.Y - hc[1]) < 2.4, RAMPS["virus"][2])
    # Collar, tail (lit from the top), base plate.
    c.paint((c.X > 21) & (c.X < 24) & (c.Y > 11) & (c.Y < 17), RAMPS["virus"][3])
    c.paint((c.X >= 24) & (c.X < 33) & (c.Y > 12.5) & (c.Y < 13.5), RAMPS["virus"][3])
    c.paint((c.X >= 24) & (c.X < 33) & (c.Y > 13.5) & (c.Y < 15.5), RAMPS["virus"][2])
    c.paint((c.X >= 24) & (c.X < 33) & (c.Y > 15.5) & (c.Y < 16.5), RAMPS["virus"][1])
    c.paint((c.X > 32) & (c.X < 35) & (c.Y > 9.5) & (c.Y < 18.5), RAMPS["virus"][2])
    c.paint((c.X > 32) & (c.X < 33.5) & (c.Y > 9.5) & (c.Y < 18.5), RAMPS["virus"][3])
    c.outline(RAMPS["virus"][0])
    # Legs go over the outline. Each one twitches on its own phase.
    for i, (y0, x1, y1) in enumerate([(10.5, 40, 4.5), (12, 42, 9), (17, 42, 19), (18.5, 40, 23.5)]):
        twitch = np.sin(ph + i * 1.7)
        c.line(34, y0, x1 - 1.0 * twitch, y1 + 1.6 * twitch * (1 if y1 < 14 else -1), RAMPS["virus"][1], 0.55)
    c.spark([(7, 9), (8, 8), (9, 8), (7, 10)], RAMPS["virus"][4])
    return c


def food_carbon() -> Canvas:
    """Organic carbon: a warm flake. The most common pickup."""
    c = Canvas(12, 12)
    sdf = blob_sdf(c, 6, 6, 3.6, [(3, 0.22, 0.4), (2, 0.15, 1.1)])
    c.lit_body(sdf, RAMPS["carbon"], width=3.6)
    c.spark([(4, 4), (5, 4)], RAMPS["carbon"][4])
    c.outline(RAMPS["carbon"][0])
    return c


def pickup_mineral() -> Canvas:
    """Mineral shard (the Minerals channel). A cyan crystal."""
    c = Canvas(12, 18)
    pts = [(6, 1), (10, 5), (9.5, 13), (6, 17), (2.5, 13), (2, 5)]
    m = c.poly(pts)
    c.paint(m, RAMPS["mineral"][2])
    c.paint(m & (c.X < 6), RAMPS["mineral"][3])
    c.paint(m & (c.X >= 6), RAMPS["mineral"][1])
    c.paint(m & (c.Y < 5.5) & (c.X >= 4.5) & (c.X <= 7.5), RAMPS["mineral"][4])
    c.paint(m & (c.X >= 5.5) & (c.X < 6.5), RAMPS["mineral"][2])
    c.paint(m & (c.Y > 13.5) & (c.X >= 6), RAMPS["mineral"][0])
    c.outline(RAMPS["mineral"][0])
    c.spark([(4, 5), (4, 6)], RAMPS["mineral"][4])
    return c


def pickup_metal() -> Canvas:
    """Metals (Fe-S): a golden pyrite cube. The cofactor."""
    c = Canvas(16, 16)
    T, UR, LR, B, LL, UL, C = (8, 1.5), (14, 4.7), (14, 11), (8, 14.4), (2, 11), (2, 4.7), (8, 8)
    c.paint(c.poly([T, UR, C, UL]), RAMPS["metal"][3])
    c.paint(c.poly([UL, C, B, LL]), RAMPS["metal"][2])
    c.paint(c.poly([C, UR, LR, B]), RAMPS["metal"][1])
    c.outline(RAMPS["metal"][0])
    c.spark([(8, 3), (7, 4), (9, 4), (8, 5)], RAMPS["metal"][4])
    c.spark([(4, 7)], RAMPS["metal"][4])
    return c


def _lipid_row(c: Canvas, xs, y_head, y_core, sign) -> None:
    for x in xs:
        # Two tails for each head, a little wavy.
        for off in (-1.3, 1.3):
            for i in range(int(abs(y_core - y_head)) + 1):
                y = y_head + sign * (2.5 + i)
                if (y - y_head) * sign > abs(y_core - y_head) + 0.5:
                    break
                wob = 0.5 if (i // 2) % 2 else -0.5
                c.spark([(int(x + off + wob), int(y))], RAMPS["tail"][3])
        c.disc(x, y_head, 2.9, RAMPS["head"])


def wall_membrane(width: int = 16, pore: bool = False) -> Canvas:
    """A piece of world membrane: a lipid bilayer. Tiles sideways.

    Walls block movement. The pore piece has a transporter, which is how a cell
    with the matching transporter feeds through the wall.
    """
    h = 26
    c = Canvas(width, h)
    c.paint(np.ones((h, width), dtype=bool), RAMPS["tail"][0])
    xs = [4 + 8 * i for i in range(width // 8)]
    _lipid_row(c, xs, 3.2, 12, +1)
    _lipid_row(c, xs, h - 3.2, h - 12, -1)
    if pore:
        cx = width / 2
        barrel = (np.abs(c.X - cx) < 6.2)
        c.paint(barrel, RAMPS["protein"][1])
        c.paint(barrel & (c.X < cx), RAMPS["protein"][2])
        c.paint(barrel & (c.X < cx - 3.5), RAMPS["protein"][3])
        c.paint((np.abs(c.X - cx) < 1.8), RAMPS["rock"][0])
        c.paint((np.abs(c.X - cx) < 1.8) & (c.Y % 4 < 1), RAMPS["protein"][4])
        # Edge lines for the barrel.
        edge = (np.abs(np.abs(c.X - cx) - 6.2) < 0.7)
        c.paint(edge, RAMPS["protein"][0])
    return c


def vent_side() -> Canvas:
    """A black smoker seen from the side: a rock tower, a hot mouth, a plume.

    The game is top-down, so this is not for the Run screen. It is for menu
    and loading art, cutaways, and the codex.
    """
    W, H = 72, 120
    c = Canvas(W, H)
    rng = np.random.RandomState(7)
    top, base = 46, H - 1
    cx = 36.0
    rows = np.arange(H)
    t = np.clip((rows - top) / (base - top), 0, 1)
    half = 10 + 15 * t**2.0 + 1.6 * np.sin(rows * 0.45) * t  # flares at the foot
    half += np.cumsum(rng.randn(H) * 0.25) * 0.5 * t
    lean = -2 * (1 - t)
    for y in range(top, H):
        lo, hi = cx + lean[y] - half[y], cx + lean[y] + half[y]
        xs = np.arange(W) + 0.5
        row = (xs > lo) & (xs < hi)
        u = (xs - (cx + lean[y])) / half[y]
        streak = np.sin(xs * 1.7 + rows[y] * 0.08) * 0.05 + rng.randn(W) * 0.025
        shade = np.clip(0.52 - 0.42 * u + streak, 0, 0.999)
        tone = np.digitize(shade, [0.18, 0.40, 0.62, 0.82])  # 0..4
        for x in np.where(row)[0]:
            c.px[y, x] = (*RAMPS["rock"][min(tone[x], 4)], 255)
    body = c.filled
    # Mineral crust: bands and speckles.
    for _ in range(90):
        x, y = rng.randint(0, W), rng.randint(top + 4, H)
        if body[y, x]:
            tone = RAMPS["metal"][1] if rng.rand() < 0.45 else RAMPS["mineral"][1]
            c.spark([(x, y)], tone)
            if rng.rand() < 0.5 and x + 1 < W and body[y, x + 1]:
                c.spark([(x + 1, y)], tone)
    # Heat on the upper rim: the mouth of the vent lights the rock near it.
    glow = np.hypot(c.X - cx, (c.Y - top) * 1.3)
    hot = body & (glow < 17)
    lvl = np.clip(((17 - glow) / 17 * 4).astype(int), 0, 3)
    bayer = np.tile(BAYER4, (H // 4 + 1, W // 4 + 1))[:H, :W]
    for i in range(4):
        sel = hot & (lvl == i) & (((17 - glow) / 17) > bayer * 0.9)
        c.paint(sel, RAMPS["vent"][i])
    c.outline(RAMPS["rock"][0])
    # The mouth: a bright ellipse on top.
    y_mouth = top - 1
    mouth = ((c.X - (cx + lean[top])) / (half[top] - 1.5)) ** 2 + ((c.Y - y_mouth) / 3.2) ** 2 < 1
    c.paint(mouth, RAMPS["vent"][3])
    c.paint(mouth & (c.Y > y_mouth), RAMPS["vent"][2])
    c.paint(mouth & (np.abs(c.X - cx) < 5) & (np.abs(c.Y - y_mouth) < 1.2), RAMPS["vent"][4])
    # Plume: dark, dithered, wider as it rises.
    for y in range(0, top - 2):
        k = (top - 2 - y) / (top - 2)  # 0 at the mouth, 1 at the top
        width = 5 + 12 * k
        centre = cx + lean[top] + np.sin(y * 0.22) * 3 * k
        dens = (1 - k) ** 0.9
        for x in range(W):
            if abs(x + 0.5 - centre) < width:
                edge = 1 - abs(x + 0.5 - centre) / width
                if dens * (0.4 + edge) > BAYER4[y % 4, x % 4] * 1.15:
                    c.px[y, x] = (*RAMPS["rock"][2 if edge > 0.5 else 1], 255)
    c.halo(cx + lean[top], y_mouth, 6, 20, [RAMPS["vent"][0], RAMPS["vent"][1], RAMPS["vent"][2]], 0.55)
    return c


def vent_top() -> Canvas:
    """A black smoker seen from above: a mound with a glowing crater.

    The mound is lit from the upper left like every other sprite. Terraces and
    radial ridges give it height. Heat spreads from the crater and fades into
    the rock. The vent is the main source of the Heat and Minerals channels.
    """
    S = 128
    c = Canvas(S, S)
    rng = np.random.RandomState(11)
    cx = cy = S / 2
    sdf = blob_sdf(c, cx, cy, 39, [(2, 0.03, 0.4), (3, 0.035, 1.9), (4, 0.02, 0.2)])
    mask = c.lit_body(sdf, RAMPS["rock"], width=39)
    dx, dy = c.X - cx, c.Y - cy
    r = np.hypot(dx, dy)
    theta = np.arctan2(dy, dx)
    wob = 1 + 0.05 * np.sin(3 * theta + 0.8) + 0.04 * np.sin(5 * theta + 2.1)
    rr = r / wob
    facing = (np.cos(theta) * -0.6 + np.sin(theta) * -0.8) > 0  # towards the light
    # Terraces: a lit edge on the upper left, a dark edge on the lower right.
    for ring in (21, 29, 36):
        edge = mask & (np.abs(rr - ring) < 0.85)
        c.paint(edge & facing, RAMPS["rock"][4])
        c.paint(edge & ~facing, RAMPS["rock"][0])
        step = mask & (rr > ring) & (rr < ring + 2.2)
        c.paint(step & facing, RAMPS["rock"][3])
    # Radial ridges, as if the rock flowed out of the crater.
    ridge = np.sin(11 * theta + 2.0 * np.sin(3 * theta) + rr * 0.05) > 0.9
    c.paint(mask & ridge & (rr > 17) & ~facing, RAMPS["rock"][0])
    c.paint(mask & ridge & (rr > 17) & facing, RAMPS["rock"][4])
    # Mineral crust: specks of sulfide (gold) and of minerals (cyan).
    for _ in range(140):
        a, d = rng.uniform(0, 2 * np.pi), rng.uniform(17, 37)
        x, y = int(cx + d * np.cos(a)), int(cy + d * np.sin(a))
        if mask[y, x]:
            tone = RAMPS["metal"][1] if rng.rand() < 0.5 else RAMPS["mineral"][1]
            if rng.rand() < 0.25:
                tone = RAMPS["metal"][2] if tone is RAMPS["metal"][1] else RAMPS["mineral"][2]
            c.spark([(x, y)], tone)
            if rng.rand() < 0.4 and mask[y, x + 1]:
                c.spark([(x + 1, y)], tone)
    # Heat: dithered, strongest at the crater, fading out over the rock.
    bayer = np.tile(BAYER4, (S // 4 + 1, S // 4 + 1))[:S, :S]
    heat = np.clip(1 - (rr - 13) / 15, 0, 1)
    level = np.minimum((heat * 3.0).astype(int), 2)
    for i in range(3):
        c.paint(mask & (rr > 13) & (level == i) & (heat > bayer), RAMPS["vent"][i])
    c.outline(RAMPS["rock"][0])
    # Crater: a molten pool with a hot rim.
    crater = blob_sdf(c, cx, cy, 13, [(3, 0.05, 0.7), (2, 0.04, 2.0)])
    rim = (crater < 3.2) & (crater >= 0)
    c.paint(rim, RAMPS["vent"][0])
    c.paint(rim & (heat > bayer), RAMPS["vent"][1])
    inside = crater < 0
    t = np.clip(r / 13, 0, 1)
    lava = np.clip(((1 - t) * 3.4 + (bayer - 0.5) * 0.9).astype(int), 0, 3)
    for i, tone in enumerate([2, 2, 3, 4]):
        c.paint(inside & (lava == i), RAMPS["vent"][tone])
    c.paint(inside & (crater > -1.4), RAMPS["vent"][1])
    # Glow on the water around the mound.
    c.halo(cx, cy, 39, 62, [RAMPS["vent"][0], RAMPS["vent"][1], RAMPS["vent"][2]], 0.5)
    return c


def vent_plume() -> Canvas:
    """Smoke over the vent, seen from above. Draw it above the creatures.

    It is dithered, so the crater and the cells below still show through.
    """
    S = 128
    c = Canvas(S, S)
    rng = np.random.RandomState(5)
    cx = cy = S / 2
    dens = np.zeros((S, S))
    for _ in range(16):
        a, d = rng.uniform(0, 2 * np.pi), rng.uniform(2, 36)
        px_, py_ = cx + d * np.cos(a) + 6, cy + d * np.sin(a) - 4  # it drifts up and right
        rad = rng.uniform(8, 16) * (0.7 + d / 70)
        dens += np.clip(1 - np.hypot(c.X - px_, c.Y - py_) / rad, 0, 1)
    fade = np.clip(1 - (np.hypot(c.X - cx - 6, c.Y - cy + 4) - 14) / 46, 0, 1)
    near = np.clip((np.hypot(c.X - cx, c.Y - cy) - 12) / 16, 0, 1)  # keep the crater visible
    a = np.clip(dens * 0.55, 0, 1) * fade * near
    bayer = np.tile(BAYER4, (S // 4 + 1, S // 4 + 1))[:S, :S]
    on = a > bayer
    c.paint(on, RAMPS["rock"][2])
    c.paint(on & (a > 0.55), RAMPS["rock"][1])
    # Lit side of each puff.
    shifted = np.roll(np.roll(on, 1, axis=0), 1, axis=1)
    c.paint(on & ~shifted & (a < 0.55), RAMPS["rock"][3])
    return c


# --------------------------------------------------------------------------
# Metabolism view: pool icons, enzymes, pathway plates, valves
# --------------------------------------------------------------------------


def shade_mask(c: Canvas, mask: np.ndarray, tones) -> None:
    """Fill a mask with 3 tones, lit from the upper left, plus a highlight."""
    ys, xs = np.nonzero(mask)
    if len(xs) == 0:
        return
    cx, cy = xs.mean() + 0.5 - c.ox, ys.mean() + 0.5 - c.oy
    r = max(np.sqrt(len(xs) / np.pi), 1.0)
    s = ((c.X - cx) * 0.6 + (c.Y - cy) * 0.8) / r
    c.paint(mask, tones[1])
    c.paint(mask & (s < 0.15), tones[2])
    c.paint(mask & (s < -0.35), tones[3])
    c.paint(mask & (s < -0.8), tones[4])


def ellipse_sdf(c: Canvas, cx, cy, rx, ry):
    return (np.hypot((c.X - cx) / rx, (c.Y - cy) / ry) - 1.0) * min(rx, ry)


def _rows_mask(c: Canvas, rows: dict[int, tuple[int, int]]) -> np.ndarray:
    m = np.zeros((c.h, c.w), dtype=bool)
    for y, (x0, x1) in rows.items():
        m[y, x0 : x1 + 1] = True
    return m


MOL_NAMES = {  # Currency variant -> file name
    "ATP": "atp", "ReducingPower": "reducing_power", "Pyruvate": "pyruvate",
    "CarbonSkeletons": "carbon_skeletons", "AminoAcids": "amino_acids",
    "Nucleotides": "nucleotides", "Lipids": "lipids", "Metals": "metals",
    "Waste": "waste", "Minerals": "minerals", "Pigments": "pigments", "O2": "o2", "ROS": "ros",
}


def drop_mask(c: Canvas, cx, cy, r) -> np.ndarray:
    tip = c.poly([(cx, cy - r * 2.0), (cx + r * 0.9, cy - r * 0.5), (cx - r * 0.9, cy - r * 0.5)])
    return (np.hypot(c.X - cx, c.Y - cy) < r) | tip


def mol_icon(kind: str) -> Canvas:
    """A 10x10 icon for an internal pool. The silhouette differs for every pool."""
    c = Canvas(10, 10)
    if kind == "atp":  # a coin with a ring
        R = RAMPS["atp"]
        c.lit_body(blob_sdf(c, 5, 5, 3.7), R, width=3.7)
        d = np.hypot(c.X - 5, c.Y - 5)
        c.paint((d >= 1.6) & (d < 2.6) & c.filled, R[1])
        c.paint(d < 1.6, R[3])
        c.spark([(3, 3), (4, 3)], R[4])
    elif kind == "reducing_power":  # a bolt: electrons
        R = RAMPS["reducing"]
        shade_mask(c, _rows_mask(c, {1: (5, 8), 2: (4, 7), 3: (3, 6), 4: (2, 7), 5: (4, 8), 6: (4, 7), 7: (3, 6), 8: (3, 5)}), R)
    elif kind == "pyruvate":  # a small tilted pill
        R = RAMPS["pyruvate"]
        c.lit_body(capsule_sdf(c, 3.2, 6.8, 6.8, 3.2, 2.3), R, width=2.3)
        c.spark([(3, 5), (4, 4)], R[4])
    elif kind == "carbon_skeletons":  # the flake of food_carbon
        R = RAMPS["carbon"]
        c.lit_body(blob_sdf(c, 5, 5, 3.3, [(3, 0.22, 0.4), (2, 0.15, 1.1)]), R, width=3.3)
        c.spark([(3, 3), (4, 3)], R[4])
    elif kind == "amino_acids":  # a block with a side group
        R = RAMPS["amino"]
        box = _rows_mask(c, {y: (2, 6) for y in range(4, 9)})
        c.line(6, 4.5, 7.5, 2.5, R[2], 0.6)
        ball = np.hypot(c.X - 7.5, c.Y - 2.6) < 1.7
        shade_mask(c, box | ball, R)
        c.paint(ball, R[3])
        c.paint(ball & (c.X > 7.5), R[2])
        c.spark([(7, 2)], R[4])
    elif kind == "nucleotides":  # a base ring
        R = RAMPS["nucleotide"]
        hexa = c.poly([(5, 1.5), (8.4, 3.2), (8.4, 6.8), (5, 8.5), (1.6, 6.8), (1.6, 3.2)])
        shade_mask(c, hexa, R)
        c.paint(np.hypot(c.X - 5, c.Y - 5) < 1.4, R[0])
    elif kind == "lipids":  # a head with two tails
        for x in (3.5, 6.5):
            for y in range(5, 9):
                c.plot(x + (0.0 if (y // 2) % 2 else 1.0) - 0.5, y, RAMPS["tail"][3])
        c.disc(5, 3.4, 2.5, RAMPS["head"])
    elif kind == "metals":  # the Fe-S cube
        R = RAMPS["metal"]
        T, UR, LR, B, LL, UL, C = (5, 1.6), (8.4, 3.6), (8.4, 6.8), (5, 8.6), (1.6, 6.8), (1.6, 3.6), (5, 5.1)
        c.paint(c.poly([T, UR, C, UL]), R[3])
        c.paint(c.poly([UL, C, B, LL]), R[2])
        c.paint(c.poly([C, UR, LR, B]), R[1])
        c.spark([(4, 2), (5, 3)], R[4])
    elif kind == "waste":  # a lumpy blob with dark flecks
        R = RAMPS["waste"]
        c.lit_body(blob_sdf(c, 5, 5, 3.4, [(3, 0.25, 0.2), (5, 0.12, 1.0)]), R, width=3.4)
        c.spark([(6, 5), (4, 6), (6, 3)], R[0])
        c.spark([(3, 3)], R[4])
    elif kind == "minerals":  # the cyan shard of pickup_mineral
        R = RAMPS["mineral"]
        m = c.poly([(5, 1.5), (7.8, 3.5), (7.4, 6.8), (5, 8.5), (2.6, 6.8), (2.2, 3.5)])
        c.paint(m, R[2])
        c.paint(m & (c.X < 5), R[3])
        c.paint(m & (c.X >= 5), R[1])
        c.paint(m & (c.Y < 4) & (c.X >= 3.5) & (c.X <= 5.5), R[4])
    elif kind == "pigments":  # a drop of carotenoid
        R = RAMPS["pigment"]
        shade_mask(c, drop_mask(c, 5, 6, 2.9), R)
    elif kind == "o2":  # a dumbbell: two atoms
        R = RAMPS["o2"]
        c.disc(3.4, 6.4, 2.6, R)
        c.disc(6.6, 3.6, 2.6, R)
    elif kind == "ros":  # a spiky radical
        R = RAMPS["ros"]
        th = np.arctan2(c.Y - 5, c.X - 5)
        rr = 1.9 + 2.3 * np.abs(np.cos(2 * th)) ** 4 + 1.0 * np.abs(np.sin(2 * th)) ** 6
        shade_mask(c, np.hypot(c.X - 5, c.Y - 5) < rr, R)
    else:
        raise ValueError(kind)
    c.outline({"pigments": RAMPS["pigment"], "o2": RAMPS["o2"], "ros": RAMPS["ros"], "lipids": RAMPS["tail"], "carbon_skeletons": RAMPS["carbon"], "metals": RAMPS["metal"],
               "minerals": RAMPS["mineral"], "reducing_power": RAMPS["reducing"],
               "amino_acids": RAMPS["amino"], "nucleotides": RAMPS["nucleotide"]}.get(kind, RAMPS.get(kind, RAMPS["waste"]))[0])
    return c


def enzyme(i: int = 0) -> Canvas:
    """A protein body with a notched active site. Bobs by one pixel. Blue, so the
    screen can tint it with a pathway accent."""
    R = RAMPS["protein"]
    c = Canvas(16, 16)
    ph = 2 * np.pi * i / 8
    dy = round(1.2 * np.sin(ph))
    body = blob_sdf(c, 8, 8.6 + dy, 5.2, [(3, 0.06, 0.5)])
    notch = np.hypot(c.X - 11.2, c.Y - (5.4 + dy)) - (2.3 + 0.5 * np.cos(ph))
    sdf = np.maximum(body, -notch)
    c.lit_body(sdf, R, width=4.5)
    c.spark([(4, 7 + dy), (5, 6 + dy), (4, 8 + dy)], R[4])
    rim = c.dilate(notch < 0) & (notch >= 0) & (body < 0)
    c.paint(rim & c.filled, R[0])  # the dark mouth of the active site
    c.outline(R[0])
    return c


def ribosome(i: int = 0) -> Canvas:
    """Two subunits with an mRNA thread between them and a growing chain."""
    L = RAMPS["nucleotide"]
    S = [L[0], L[2], L[3], L[4], L[4]]  # the small subunit: one step lighter
    c = Canvas(24, 24)
    # The chain leaves the large subunit down and right, then runs off the sheet.
    # It starts under the subunit, so a new bead seems to grow out of it.
    step = (2.6, 1.8)
    for k in range(-1, 6):
        c.disc(13.5 + (k + i / 8) * step[0], 18.0 + (k + i / 8) * step[1], 1.5, RAMPS["amino"])
    c.lit_body(ellipse_sdf(c, 10.5, 17.5, 9.0, 4.6), L, width=4.5)
    c.outline(L[0])
    c.lit_body(ellipse_sdf(c, 10.5 + round(np.sin(2 * np.pi * i / 8)), 7.0, 6.5, 3.6), S, width=3.6)
    c.outline(L[0])
    # The thread runs left through the gap between the subunits.
    for x in range(24):
        if (x + i) % 4 < 3:
            c.plot(x, 12, RAMPS["granule"][3])
    return c


def _fold(v: np.ndarray, lo: int, hi: int) -> np.ndarray:
    """Collapse the stretchable middle of a nine-slice to one pixel, so a pattern
    does not change when the slice stretches."""
    return np.where(v < lo, v, np.where(v >= hi, v - (hi - lo - 1), lo))


PLATE = 24
PLATE_BORDER = 8


def pathway_plate(ghost: bool = False) -> Canvas:
    """A nine-slice frame, 24x24 with an 8 px border. The middle of every edge is
    the same along its length, so it stretches. The ghost is the same shape drawn
    with a dither, for a pathway that the cell does not have yet."""
    R = RAMPS["lytic"]
    c = Canvas(PLATE, PLATE)
    yy, xx = np.mgrid[0:PLATE, 0:PLATE]
    q = np.stack([np.abs(xx + 0.5 - PLATE / 2) - (PLATE / 2 - 4), np.abs(yy + 0.5 - PLATE / 2) - (PLATE / 2 - 4)])
    dist = np.hypot(np.maximum(q[0], 0), np.maximum(q[1], 0)) + np.minimum(np.maximum(q[0], q[1]), 0) - 4
    depth = np.floor(-dist).astype(int)  # 0 = outermost ring
    inside = dist < 0
    dl, dr, dt, db = xx + 0.5, PLATE - xx - 0.5, yy + 0.5, PLATE - yy - 0.5
    nearest = np.stack([dl, dt, dr, db]).argmin(axis=0)
    lit = nearest < 2  # left and top sides catch the light
    fx = _fold(xx, PLATE_BORDER, PLATE - PLATE_BORDER)
    fy = _fold(yy, PLATE_BORDER, PLATE - PLATE_BORDER)
    if not ghost:
        c.paint(inside, RAMPS["rock"][1])
        c.paint(inside & (depth >= 1) & (depth < 3), R[1])
        c.paint(inside & (depth == 1) & lit, R[3])
        c.paint(inside & (depth == 2) & lit, R[2])
        c.paint(inside & (depth == 1) & ~lit, R[2])
        c.paint(inside & (depth == 2) & ~lit, R[1])
        c.paint(inside & (depth == 0), R[0])
    else:
        level = np.select([depth == 0, depth == 1, depth == 2, depth == 3], [0.95, 0.7, 0.45, 0.25], 0.0)
        bayer = BAYER4[fy % 4, fx % 4]
        on = inside & (level > bayer)
        c.paint(on & (depth == 0), R[1])
        c.paint(on & (depth >= 1) & lit, R[2])
        c.paint(on & (depth >= 1) & ~lit, R[1])
    return c


PATHWAY_ICONS = {  # BlockKind with a block_spec -> file suffix
    "Chemosynthesis": "chemosynthesis", "AminoAcidBiosynthesis": "amino_acid_biosynthesis",
    "LipidMetabolism": "lipid_metabolism", "ProteinSynthesis": "protein_synthesis",
    "Fermentation": "fermentation", "VesicleExport": "vesicle_export", "FatStorage": "fat_storage",
    "LightCapture": "light_capture", "Respiration": "respiration", "Catalase": "catalase",
    "Cuticle": "cuticle", "NucleotideCofactorSynthesis": "nucleotide_cofactor_synthesis",
    "SecondaryMetabolites": "secondary_metabolites",
}


def pathway_icon(kind: str) -> Canvas:
    c = Canvas(16, 16)
    if kind == "chemosynthesis":  # vent flame over a mineral crystal
        V, M = RAMPS["vent"], RAMPS["mineral"]
        shade_mask(c, c.poly([(8, 1.5), (10.8, 5.5), (10.2, 8.8), (5.8, 8.8), (5.2, 5.5)]), V)
        crystal = c.poly([(8, 8.2), (11.2, 10.2), (10.6, 13.6), (8, 14.6), (5.4, 13.6), (4.8, 10.2)])
        c.paint(crystal, M[2])
        c.paint(crystal & (c.X < 8), M[3])
        c.paint(crystal & (c.X >= 8), M[1])
        c.paint(crystal & (c.Y < 11) & (c.X > 6) & (c.X < 9), M[4])
        c.outline(M[0])
    elif kind == "amino_acid_biosynthesis":  # a block with a side group
        A = RAMPS["amino"]
        c.line(9.5, 6.5, 12, 3.5, A[2], 0.7)
        shade_mask(c, _rows_mask(c, {y: (3, 10) for y in range(6, 14)}), A)
        c.disc(12.2, 3.6, 2.2, A)
        c.outline(A[0])
    elif kind == "lipid_metabolism":  # a phospholipid
        for x in (6.0, 10.0):
            for y in range(8, 15):
                c.plot(x + (0.0 if (y // 2) % 2 else 1.0) - 0.5, y, RAMPS["tail"][3])
        c.disc(8, 5, 3.4, RAMPS["head"])
        c.outline(RAMPS["tail"][0])
    elif kind == "protein_synthesis":  # a ribosome with a chain
        L = RAMPS["nucleotide"]
        S = [L[0], L[2], L[3], L[4], L[4]]
        for k in range(3):
            c.disc(11 + k * 2.0, 12 + k * 1.6, 1.1, RAMPS["amino"], outline=False)
        c.lit_body(ellipse_sdf(c, 7.5, 10.5, 6.2, 3.6), L, width=3.4)
        c.lit_body(ellipse_sdf(c, 7.5, 5.5, 4.4, 2.8), S, width=2.8)
        c.outline(L[0])
    elif kind == "fermentation":  # a flask with bubbles
        G, P = RAMPS["lytic"], RAMPS["pyruvate"]
        body = c.poly([(6.5, 6), (9.5, 6), (13, 13.5), (3, 13.5)]) | _rows_mask(c, {y: (6, 9) for y in range(2, 7)})
        shade_mask(c, body, G)
        liquid = body & (c.Y > 9.5) & (c.Y < 13.5) & (c.X > 4) & (c.X < 12)
        shade_mask(c, liquid, P)
        c.paint(c.poly([(5.5, 2), (10.5, 2), (10.5, 3), (5.5, 3)]), G[3])
        c.spark([(7, 11), (9, 12)], P[4])
        c.spark([(8, 8), (9, 6)], G[4])
        c.outline(G[0])
    elif kind == "vesicle_export":  # a vesicle buds off the membrane
        H, W = RAMPS["head"], RAMPS["waste"]
        c.paint(_rows_mask(c, {y: (1, 3) for y in range(1, 15)}), H[2])
        c.paint(_rows_mask(c, {y: (1, 1) for y in range(1, 15)}), H[3])
        c.paint(_rows_mask(c, {y: (3, 3) for y in range(1, 15)}), H[1])
        c.lit_body(blob_sdf(c, 8.6, 8, 3.8), W, band=1.0, width=3.8)
        c.spark([(6, 6), (7, 6)], W[4])
        c.spark([(12, 6), (13, 7), (14, 8), (13, 9), (12, 10)], RAMPS["lytic"][4])
        c.outline(RAMPS["tail"][0])
    elif kind == "fat_storage":  # lipid beads
        B = RAMPS["bead"]
        c.disc(6.5, 9.5, 4.5, B)
        c.disc(11.8, 5.2, 3.0, B)
        c.disc(12, 12, 2.6, B)
        c.outline(B[0])
    elif kind == "light_capture":  # a sun
        P = RAMPS["pigment"]
        for a in range(8):
            ang = a * np.pi / 4
            c.line(8 + 4.2 * np.cos(ang), 8 + 4.2 * np.sin(ang), 8 + 6.6 * np.cos(ang), 8 + 6.6 * np.sin(ang), P[3], 0.55)
        c.lit_body(blob_sdf(c, 8, 8, 3.8), P, width=3.8)
        c.outline(P[0])
    elif kind == "respiration":  # O2 in, ATP out
        c.disc(4.2, 10.8, 2.6, RAMPS["o2"])
        c.disc(7.4, 7.6, 2.6, RAMPS["o2"])
        c.lit_body(blob_sdf(c, 11.4, 4.6, 3.3), RAMPS["atp"], width=3.3)
        c.paint((np.hypot(c.X - 11.4, c.Y - 4.6) < 1.3) & c.filled, RAMPS["atp"][3])
        c.outline(RAMPS["o2"][0])
    elif kind == "catalase":  # a radical turns into oxygen
        th = np.arctan2(c.Y - 8, c.X - 4)
        rr = 1.6 + 2.0 * np.abs(np.cos(2 * th)) ** 4 + 0.8 * np.abs(np.sin(2 * th)) ** 6
        shade_mask(c, np.hypot(c.X - 4, c.Y - 8) < rr, RAMPS["ros"])
        c.disc(12, 8, 3.0, RAMPS["o2"])
        c.paint(_rows_mask(c, {6: (7, 7), 7: (8, 8), 8: (9, 9), 9: (8, 8), 10: (7, 7)}), RAMPS["protein"][3])
        c.outline(RAMPS["protein"][0])
    elif kind == "cuticle":  # a tough layered wall
        F = RAMPS["fusion"]
        for k, (y, off) in enumerate([(2, 0), (6, 3), (10, 0)]):
            for x0 in (-off, 6 - off, 12 - off):
                x0 = max(x0, 0) if x0 < 0 else x0
                shade_mask(c, _rows_mask(c, {yy: (max(x0 + 1, 1), min(x0 + 5, 14)) for yy in range(y, y + 3)}), F)
        c.outline(F[0])
    elif kind == "nucleotide_cofactor_synthesis":  # a base ring and a Fe-S cube
        hexa = c.poly([(6, 2), (10.4, 4.2), (10.4, 8.6), (6, 10.8), (1.6, 8.6), (1.6, 4.2)])
        shade_mask(c, hexa, RAMPS["nucleotide"])
        c.paint(np.hypot(c.X - 6, c.Y - 6.4) < 1.5, RAMPS["nucleotide"][0])
        M = RAMPS["metal"]
        c.paint(c.poly([(12, 8.5), (14.4, 9.8), (12, 11), (9.6, 9.8)]), M[3])
        c.paint(c.poly([(9.6, 9.8), (12, 11), (12, 14.4), (9.6, 13)]), M[2])
        c.paint(c.poly([(12, 11), (14.4, 9.8), (14.4, 13), (12, 14.4)]), M[1])
        c.outline(RAMPS["nucleotide"][0])
    elif kind == "secondary_metabolites":  # two drops: a pigment and a toxin
        shade_mask(c, drop_mask(c, 5.2, 10, 3.0), RAMPS["pigment"])
        shade_mask(c, drop_mask(c, 11.2, 8, 2.6), RAMPS["toxin"])
        c.outline(RAMPS["toxin"][0])
    else:
        raise ValueError(kind)
    return c


def slot_socket() -> Canvas:
    """An empty socket on the bilayer: a collar around a dark pit. A protein sits
    in it when the cell has one."""
    w = 24
    c = wall_membrane(w)
    P = RAMPS["protein"]
    cx, cy = w / 2, c.h / 2
    dx, dy = c.X - cx, c.Y - cy
    d = np.hypot(dx / 8.2, dy / 9.4)
    c.paint(d < 1.0, P[2])
    c.paint((d < 1.0) & (dx + dy < -4), P[3])
    c.paint((d < 1.0) & (dx + dy > 4), P[1])
    c.paint((d >= 0.94) & (d < 1.0), P[0])
    pit = d < 0.62
    c.paint(pit, RAMPS["rock"][0])
    c.paint(pit & ((c.X + c.Y).astype(int) % 2 == 0) & (dy > 0), RAMPS["rock"][1])
    c.paint((d >= 0.62) & (d < 0.7), P[0])
    return c


def badge_limit() -> Canvas:
    """A cap on a pathway: a bar above a downward arrow, in a steel disc."""
    R = RAMPS["lytic"]
    c = Canvas(12, 12)
    c.lit_body(blob_sdf(c, 6, 6, 5.0), R, width=5.0)
    c.paint(_rows_mask(c, {3: (3, 8), 4: (3, 8)}), RAMPS["rock"][0])
    c.paint(_rows_mask(c, {6: (5, 6), 7: (5, 6), 8: (4, 7), 9: (5, 6)}), RAMPS["rock"][0])
    c.outline(R[0])
    return c


def valve(state: str) -> Canvas:
    """A pipe with a gate: shut, a third open, open (the gate is lifted out)."""
    R = RAMPS["lytic"]
    c = Canvas(12, 12)
    c.paint(_rows_mask(c, {y: (0, 11) for y in range(6, 11)}), R[2])
    c.paint(_rows_mask(c, {6: (0, 11)}), R[3])
    c.paint(_rows_mask(c, {10: (0, 11)}), R[1])
    c.paint(_rows_mask(c, {y: (0, 11) for y in (7, 8, 9)}), RAMPS["rock"][0])
    bottom = {"off": 9, "low": 8, "high": 4}[state]
    gate = _rows_mask(c, {y: (4, 7) for y in range(3, bottom + 1)})
    c.paint(gate, R[3])
    c.paint(gate & (c.X < 5), R[4])
    c.paint(gate & (c.X >= 7), R[2])
    if state == "high":
        c.paint(_rows_mask(c, {y: (5, 6) for y in range(7, 10)}), RAMPS["player"][2])  # flow
    c.paint(_rows_mask(c, {1: (3, 8), 2: (3, 8)}), R[2])
    c.paint(_rows_mask(c, {1: (3, 8)}), R[3])
    c.outline(R[0])
    return c


METABOLISM_STATIC = {
    **{f"mol_{n}": (lambda n=n: mol_icon(n)) for n in MOL_NAMES.values()},
    **{f"pathway_icon_{n}": (lambda n=n: pathway_icon(n)) for n in PATHWAY_ICONS.values()},
    "enzyme": lambda: enzyme(0),
    "ribosome": lambda: ribosome(0),
    "pathway_plate": lambda: pathway_plate(False),
    "pathway_plate_ghost": lambda: pathway_plate(True),
    "slot_socket": slot_socket,
    "badge_limit": badge_limit,
    "valve_off": lambda: valve("off"),
    "valve_low": lambda: valve("low"),
    "valve_high": lambda: valve("high"),
}

METABOLISM_ANIMS = {
    "enzyme": dict(make=enzyme, size=16, clip="idle", fps=8),
    "ribosome": dict(make=ribosome, size=24, clip="loop", fps=8),
}


def write_metabolism_anims() -> None:
    path = ANIM_DIR / "manifest.json"
    manifest = json.loads(path.read_text())
    for name, spec in METABOLISM_ANIMS.items():
        frames = [spec["make"](i) for i in range(FRAMES)]
        write_png(ANIM_DIR / f"{name}_{spec['clip']}.png", np.concatenate([f.px for f in frames], axis=1))
        manifest[name] = {
            "frame_size": [spec["size"], spec["size"]],
            "anchor_px": [spec["size"] // 2, spec["size"] // 2],
            spec["clip"]: {"file": f"{name}_{spec['clip']}.png", "columns": FRAMES, "rows": 1,
                           "fps": spec["fps"], "loop": True},
        }
    path.write_text(json.dumps(manifest, indent=2) + "\n")



SPRITES = {
    "player_cell": player_cell,
    "prey_bacillus": prey_bacillus,
    "predator_cell": predator_cell,
    "symbiont_cell": symbiont_cell,
    "gate_cell_phototroph": gate_cell,
    "gate_cell_proteobacterium": gate_cell_proteobacterium,
    "virus_phage": virus_phage,
    "food_carbon": food_carbon,
    "pickup_mineral": pickup_mineral,
    "pickup_metal_fes": pickup_metal,
    "wall_membrane": lambda: wall_membrane(16),
    "wall_membrane_pore": lambda: wall_membrane(32, pore=True),
    "vent_chimney_top": vent_top,
    "vent_plume_top": vent_plume,
    "vent_chimney_side": vent_side,
    "overlay_flagellum": lambda: overlay_flagellum(0, 0, "idle"),
    "overlay_adhesin": lambda: overlay_adhesin(0, 0, "idle"),
    "overlay_toxin": lambda: overlay_toxin(0, 0, "idle"),
    "toxin_droplet": lambda: toxin_droplet(0),
    "gene_orb": lambda: gene_orb(0),
    **METABOLISM_STATIC,
}


# --------------------------------------------------------------------------
# Animation
# --------------------------------------------------------------------------

FRAMES = 8  # frames in every sheet row (death has more)
DEATH_FRAMES = 10
DIRECTIONS = ["E", "SE", "S", "SW", "W", "NW", "N", "NE"]  # screen angles k * 45 deg, +y down
FLASH = hx("#fff4e0")

# How each kind of animation plays. Directions: the sheet has one row for each.
KINDS = {
    "idle": dict(fps=6, loop=True),
    "move": dict(fps=10, loop=True, directions="direction of travel"),
    "contact": dict(fps=14, loop=False, directions="direction from the cell to the wall"),
    "hit": dict(fps=14, loop=False, directions="direction the blow travels"),
    "spawn": dict(fps=12, loop=False),
    "death": dict(fps=10, loop=False),
}


def idle_pose(i: int) -> Pose:
    """Frame i of the idle loop: membrane waves, drifting organelles, a slow breath."""
    t = i / FRAMES
    return Pose(t=t, stretch=0.02 * np.sin(2 * np.pi * t))


def move_pose(i: int, direction: int) -> Pose:
    """Frame i of the move loop, for one of the 8 directions.

    The cell pushes and relaxes: it stretches along its heading, the back end
    tapers, and the organelles lag behind. Light stays fixed at the upper left,
    so every direction is shaded again from scratch.
    """
    t = i / FRAMES
    ph = 2 * np.pi * t
    heading = direction * np.pi / 4
    push = 1.3 + 1.2 * np.sin(ph - 0.6)
    return Pose(
        t=t,
        heading=heading,
        stretch=0.07 + 0.09 * np.sin(ph),
        taper=0.10 + 0.07 * np.sin(ph - 0.9),
        lag=(-np.cos(heading) * push, -np.sin(heading) * push),
    )


def _squash_pose(i: int, heading: float, radius: float, seq) -> Pose:
    """A cell pressed against a flat plane at distance `radius` in front of it.

    seq[i] = (stretch, penetration). The body is squashed along the heading and
    bulges to the sides. Its front would pass the plane by `penetration` px, and
    the plane cuts it flat. A negative penetration means the cell has left the
    plane (the rebound). The organelles keep moving into the wall, so they lag
    forward while the body is squashed.
    """
    s, p = seq[i]
    shift = radius + p - radius * (1 + s)  # puts the front at radius + p
    push = -s * 8.0
    return Pose(
        t=0.0, heading=heading, stretch=s, shift=shift,
        cut=radius if p >= 0 else None,
        lag=(np.cos(heading) * push, np.sin(heading) * push),
    )


CONTACT_SEQ = [(-0.03, 0.0), (-0.10, 1.0), (-0.19, 2.5), (-0.26, 3.6),
               (-0.17, 2.0), (-0.05, 0.3), (0.07, -1.0), (0.02, -0.4)]
HIT_SEQ = [(-0.14, 0.8), (-0.28, 3.0), (-0.22, 2.0), (-0.05, 0.0),
           (0.12, -1.5), (0.16, -3.0), (0.06, -2.0), (0.00, -0.6)]
HIT_FLASH = [2, 2, 1, 1, 0, 0, 0, 0]  # 2 = white silhouette, 1 = checker, 0 = none
HIT_BURST = [2, 4, 5, 3, 0, 0, 0, 0]  # length of the impact rays in px
SPAWN_SCALE = [0.18, 0.45, 0.78, 1.06, 1.12, 0.97, 1.03, 1.0]
SPAWN_FLASH = [1, 1, 0, 0, 0, 0, 0, 0]
SPAWN_RING = [(0.5, 1.0), (0.8, 0.9), (1.1, 0.75), (1.4, 0.55), (1.7, 0.35), None, None, None]


def contact_pose(i: int, direction: int, radius: float) -> Pose:
    """Frame i of the squash against a wall that lies in `direction`."""
    return _squash_pose(i, direction * np.pi / 4, radius, CONTACT_SEQ)


def hit_pose(i: int, direction: int, radius: float) -> Pose:
    """Frame i of a hit. `direction` is where the blow travels.

    The cell squashes on the side that faces the attacker, then is flung away
    from it and stretches along the line of the blow.
    """
    toward_attacker = direction * np.pi / 4 + np.pi
    return _squash_pose(i, toward_attacker, radius, HIT_SEQ)


def spawn_pose(i: int) -> Pose:
    return Pose(t=i / FRAMES, scale=SPAWN_SCALE[i])


# -- pixel effects, drawn on a finished frame --------------------------------


def fx_flash(c: Canvas, level: int) -> None:
    """Turn the inside of the body white. The outline stays, so the shape reads."""
    if level == 0:
        return
    body = c.filled
    inner = body & ~c.dilate(~body)  # not on the edge of the body
    if level == 1:
        inner &= (np.add.outer(np.arange(c.h), np.arange(c.w)) % 2) == 0
    c.px[inner, :3] = FLASH


def fx_burst(c: Canvas, x: float, y: float, angle: float, size: int) -> None:
    """Impact rays that start at (x, y) and point away along `angle`."""
    for off in (0, -50, 50, -100, 100):
        a = angle + np.radians(off)
        reach = size if off == 0 else max(size - 2, 1) if abs(off) == 50 else max(size - 3, 0)
        for k in range(1, reach + 1):
            tone = FLASH if k < reach else RAMPS["vent"][3]
            c.plot(x + np.cos(a) * k, y + np.sin(a) * k, tone)


def fx_ring(c: Canvas, cx: float, cy: float, radius: float, strength: float, color) -> None:
    """A thin dithered ring that grows from the cell: the pop of a new cell."""
    d = np.hypot(c.X - cx, c.Y - cy)
    bayer = np.tile(BAYER4, (c.h // 4 + 1, c.w // 4 + 1))[: c.h, : c.w]
    on = (np.abs(d - radius) < 0.8) & (strength > bayer) & ~c.filled
    c.paint(on, color)


def _remap_to_rock(rgb: np.ndarray) -> np.ndarray:
    """Dead cells turn to grey-blue stone. Keep the light and dark of each pixel."""
    lum = (rgb[..., 0] * 0.3 + rgb[..., 1] * 0.6 + rgb[..., 2] * 0.1) / 255.0
    idx = np.digitize(lum, [0.14, 0.32, 0.52, 0.72])
    return np.array(RAMPS["rock"], dtype=np.uint8)[idx]


def _scale_px(px: np.ndarray, cx: float, cy: float, k: float) -> np.ndarray:
    """Nearest-neighbour scale of a frame about (cx, cy) in pixels."""
    h, w, _ = px.shape
    yy, xx = np.mgrid[0:h, 0:w]
    sx = np.floor(cx + (xx + 0.5 - cx) / k).astype(int)
    sy = np.floor(cy + (yy + 0.5 - cy) / k).astype(int)
    ok = (sx >= 0) & (sx < w) & (sy >= 0) & (sy < h)
    out = np.zeros_like(px)
    out[ok] = px[sy[ok], sx[ok]]
    return out


def death_frames(base: Canvas, centre, radius: float, ramp_name: str) -> list[Canvas]:
    """The death of a cell, made from its idle frame.

    0-1: the body swells and flashes, then cracks.
    2-9: it ruptures into shards that fly out, turn to stone, and fade in
         dither. A few bright specks of cytoplasm fly out with them.
    """
    n = DEATH_FRAMES
    rng = np.random.RandomState(21)
    src = base.px
    h, w, _ = src.shape
    cx, cy = centre[0] + base.ox, centre[1] + base.oy  # in pixels
    ys, xs = np.nonzero(src[..., 3] > 0)
    # Shards: the nearest of 9 random seed points in the body.
    pick = rng.choice(len(xs), 9, replace=False)
    seeds = np.stack([xs[pick] + 0.5, ys[pick] + 0.5], axis=1)
    d2 = (xs[:, None] + 0.5 - seeds[None, :, 0]) ** 2 + (ys[:, None] + 0.5 - seeds[None, :, 1]) ** 2
    shard = d2.argmin(axis=1)
    dirs = []
    for k in range(len(seeds)):
        v = np.array([xs[shard == k].mean() + 0.5 - cx, ys[shard == k].mean() + 0.5 - cy])
        if np.linalg.norm(v) < 3:
            a = rng.uniform(0, 2 * np.pi)
            v = np.array([np.cos(a), np.sin(a)])
        dirs.append(v / np.linalg.norm(v))
    dirs = np.array(dirs)
    speed = rng.uniform(0.8, 1.2, len(seeds))
    reach = radius * 0.5
    bayer = np.tile(BAYER4, (h // 4 + 1, w // 4 + 1))[:h, :w]
    ramp_cols = RAMPS[ramp_name]
    parts = rng.uniform(0, 2 * np.pi, 10), rng.uniform(0.7, 1.5, 10)

    out = []
    for i in range(n):
        c = Canvas(base.w, base.h, origin=(base.ox, base.oy))
        if i < 2:
            px = _scale_px(src, cx, cy, [1.06, 1.10][i])
            c.px[:] = px
            fx_flash(c, [2, 1][i])
            if i == 1:  # cracks: dark lines from the centre
                cracks = Canvas(base.w, base.h, origin=(base.ox, base.oy))
                for a in np.linspace(0, 2 * np.pi, 6, endpoint=False) + 0.4:
                    x, y = cx, cy
                    for _ in range(int(radius * 0.8)):
                        a += rng.uniform(-0.35, 0.35)
                        x, y = x + np.cos(a), y + np.sin(a)
                        cracks.px[int(y), int(x)] = (*ramp_cols[0], 255)
                body = c.px[..., 3] > 0
                keep = body & (cracks.px[..., 3] > 0)
                c.px[keep] = cracks.px[keep]
        else:
            q = (i - 1) / (n - 2)
            ease = 1 - (1 - q) ** 2
            moved = np.zeros_like(src)
            for k in range(len(seeds)):
                sel = shard == k
                off = np.round(dirs[k] * speed[k] * reach * ease).astype(int)
                tx, ty = xs[sel] + off[0], ys[sel] + off[1]
                ok = (tx >= 0) & (tx < w) & (ty >= 0) & (ty < h)
                moved[ty[ok], tx[ok]] = src[ys[sel][ok], xs[sel][ok]]
            body = moved[..., 3] > 0
            stone = i >= 4
            mixed = i in (2, 3)
            if stone or mixed:
                rock = _remap_to_rock(moved[..., :3])
                use = body & (np.ones_like(body) if stone else (bayer < 0.5 * (i - 1)))
                moved[use, :3] = rock[use]
            fade = [0, 0, 0, 0, 0, 0.2, 0.45, 0.7, 0.9, 1.0][i]
            moved[(1 - fade) <= bayer, 3] = 0
            c.px[:] = moved
            # Specks of cytoplasm.
            for a, sp in zip(*parts):
                px_ = cx + np.cos(a) * sp * radius * 0.9 * ease
                py_ = cy + np.sin(a) * sp * radius * 0.9 * ease
                if q < 0.85:
                    tone = ramp_cols[3] if q < 0.55 else ramp_cols[2]
                    x, y = int(px_), int(py_)
                    if 0 <= x < w and 0 <= y < h:
                        c.px[y, x] = (*tone, 255)
        out.append(c)
    return out


# name -> what to draw. centre and radius are in design coordinates.
ANIMS = {
    "player_cell": dict(
        make=player_cell, centre=(24, 24.5), radius=20, ramp="player",
        kinds=("idle", "move", "contact", "hit", "spawn", "death")),
    "prey_bacillus": dict(
        make=prey_bacillus, centre=PREY_CENTRE, radius=11.25, ramp="prey",
        kinds=("idle", "move", "contact", "hit", "spawn", "death")),
    "predator_cell": dict(
        make=predator_cell, centre=(24, 24), radius=19, ramp="predator",
        kinds=("idle", "move", "contact", "hit", "spawn", "death")),
    "symbiont_cell": dict(
        make=symbiont_cell, centre=(16, 16), radius=9.5, ramp="symbiont",
        kinds=("idle", "move", "contact", "hit", "spawn", "death")),
    "gate_cell_phototroph": dict(
        make=gate_cell, centre=(36, 36), radius=25, ramp="gate", kinds=("idle",)),
    "gate_cell_proteobacterium": dict(
        make=gate_cell_proteobacterium, centre=(36, 36), radius=25, ramp="proteo", kinds=("idle",)),
    "virus_phage": dict(
        make=virus_phage, centre=(22, 14), radius=20, ramp="virus",
        kinds=("idle", "spawn", "death")),
}


def build_animations() -> dict:
    """Draw every frame. Returns {name: {kind: [Canvas] or [[Canvas] for each direction]}}."""
    frames: dict = {}
    dirs = range(len(DIRECTIONS))
    for name, spec in ANIMS.items():
        make, kinds, R = spec["make"], spec["kinds"], spec["radius"]
        cx, cy = spec["centre"]
        e: dict = {"idle": [make(idle_pose(i), anim=True) for i in range(FRAMES)]}
        if "move" in kinds:
            e["move"] = [[make(move_pose(i, d), anim=True) for i in range(FRAMES)] for d in dirs]
        if "contact" in kinds:
            e["contact"] = [[make(contact_pose(i, d, R), anim=True) for i in range(FRAMES)] for d in dirs]
        if "hit" in kinds:
            grid = []
            for d in dirs:
                row = []
                for i in range(FRAMES):
                    pose = hit_pose(i, d, R)
                    c = make(pose, anim=True)
                    fx_flash(c, HIT_FLASH[i])
                    if HIT_BURST[i]:
                        h = pose.heading
                        fx_burst(c, cx + np.cos(h) * R, cy + np.sin(h) * R, h, HIT_BURST[i])
                    row.append(c)
                grid.append(row)
            e["hit"] = grid
        if "spawn" in kinds:
            row = []
            for i in range(FRAMES):
                c = make(spawn_pose(i), anim=True)
                fx_flash(c, SPAWN_FLASH[i])
                if SPAWN_RING[i]:
                    k, strength = SPAWN_RING[i]
                    fx_ring(c, cx, cy, R * k, strength, RAMPS[spec["ramp"]][3])
                row.append(c)
            e["spawn"] = row
        if "death" in kinds:
            e["death"] = death_frames(e["idle"][0], spec["centre"], R, spec["ramp"])
        frames[name] = e
    return frames


def _grid(entry_kind) -> np.ndarray:
    """One kind of animation as a sheet: a row of frames, or a row for each direction."""
    if isinstance(entry_kind[0], list):
        return np.concatenate([np.concatenate([f.px for f in row], axis=1) for row in entry_kind], axis=0)
    return np.concatenate([f.px for f in entry_kind], axis=1)


def write_animations(frames: dict) -> None:
    """Write one sheet for each animation, and a manifest that describes them.

    A sheet with directions has one row for each direction (DIRECTIONS order).
    Every sheet starts at the top-left. `anchor_px` is the pixel in a frame where
    the cell centre is. `contact_radius` is the distance from the anchor to the
    wall plane in the contact frames.
    """
    manifest = {}
    for name, entry in frames.items():
        spec = ANIMS[name]
        fw, fh = entry["idle"][0].w, entry["idle"][0].h
        c0 = entry["idle"][0]
        info = {
            "frame_size": [fw, fh],
            "anchor_px": [spec["centre"][0] + c0.ox, spec["centre"][1] + c0.oy],
            "radius": spec["radius"],
        }
        for kind, data in entry.items():
            write_png(ANIM_DIR / f"{name}_{kind}.png", _grid(data))
            rows = len(data) if isinstance(data[0], list) else 1
            cols = len(data[0]) if isinstance(data[0], list) else len(data)
            meta = dict(KINDS[kind])
            sheet = {"file": f"{name}_{kind}.png", "columns": cols, "rows": rows,
                     "fps": meta["fps"], "loop": meta["loop"]}
            if "directions" in meta:
                sheet["directions"] = DIRECTIONS
                sheet["direction_meaning"] = meta["directions"]
            info[kind] = sheet
        manifest[name] = info
    (ANIM_DIR / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def on_water(px: np.ndarray, scale: int, tone: int = 2) -> np.ndarray:
    """Scale a sheet up and put it on the water colour, so it is easy to see."""
    big = upscale(px, scale)
    out = np.zeros_like(big)
    out[..., :3] = WATER[tone]
    out[..., 3] = 255
    return np.where(big[..., 3:4] > 0, big, out)


def wall_layer(c: Canvas, spec: dict, direction: int) -> np.ndarray:
    """A background with a wall on the side of `direction`, for the contact previews."""
    cx, cy = spec["centre"][0] + c.ox, spec["centre"][1] + c.oy
    ang = direction * np.pi / 4
    yy, xx = np.mgrid[0 : c.h, 0 : c.w]
    u = (xx + 0.5 - cx) * np.cos(ang) + (yy + 0.5 - cy) * np.sin(ang)
    layer = np.zeros((c.h, c.w, 4), dtype=np.uint8)
    wall = u > spec["radius"]
    layer[wall] = (*RAMPS["rock"][2], 255)
    layer[wall & (u < spec["radius"] + 1.0)] = (*RAMPS["rock"][4], 255)
    return layer


def _over(bg: np.ndarray, fg: np.ndarray) -> np.ndarray:
    return np.where(fg[..., 3:4] > 0, fg, bg)


def write_anim_previews(frames: dict) -> None:
    prev = PREVIEW_DIR / "anim"
    cell = max(f.w for e in frames.values() for f in e["idle"])

    def strips(kind: str, names) -> np.ndarray:
        rows = []
        for name in names:
            data = frames[name][kind]
            row = np.zeros((cell, cell * len(data), 4), dtype=np.uint8)
            for i, f in enumerate(data):
                ox, oy = i * cell + (cell - f.w) // 2, (cell - f.h) // 2
                row[oy : oy + f.h, ox : ox + f.w] = f.px
            rows.append(row)
        return np.concatenate(rows, axis=0)

    write_png(prev / "idle.png", on_water(strips("idle", frames), 2))
    for kind in ("spawn", "death"):
        names = [n for n in frames if kind in frames[n]]
        write_png(prev / f"{kind}.png", on_water(strips(kind, names), 2))
    for name, entry in frames.items():
        for kind in ("move", "contact", "hit"):
            if kind not in entry:
                continue
            rows = []
            for d, row in enumerate(entry[kind]):
                if kind == "contact":
                    bg = wall_layer(row[0], ANIMS[name], d)
                    rows.append(np.concatenate([_over(bg, f.px) for f in row], axis=1))
                else:
                    rows.append(np.concatenate([f.px for f in row], axis=1))
            write_png(prev / f"{kind}_{name}.png", on_water(np.concatenate(rows, axis=0), 2))
    # GIFs.
    if shutil.which("magick") is None:
        print("magick not found: skipped the GIFs")
        return
    names = list(frames)
    cell, scale = 76, 3
    base_w = len(names) * cell

    def put(board, col, row, f, bg=None):
        px = f.px if bg is None else _over(bg, f.px)
        ox, oy = col * cell + (cell - f.w) // 2, row * cell + (cell - f.h) // 2
        board[oy : oy + f.h, ox : ox + f.w] = px

    def gif(path: Path, count: int, rows: int, fill, delay: int):
        with tempfile.TemporaryDirectory() as tmp:
            for i in range(count):
                board = np.zeros((rows * cell, base_w, 4), dtype=np.uint8)
                for col, name in enumerate(names):
                    fill(board, col, name, i)
                write_png(Path(tmp) / f"f{i:02d}.png", on_water(board, scale, 1))
            subprocess.run(
                ["magick", "-delay", str(delay), "-loop", "0", f"{tmp}/f*.png", "-layers", "optimize", str(path)],
                check=True,
            )

    def loops(board, col, name, i):
        e = frames[name]
        put(board, col, 0, e["idle"][i % FRAMES])
        if "move" in e:
            put(board, col, 1, e["move"][(i // 2) % 8][i % FRAMES])

    gif(prev / "preview.gif", 16, 2, loops, 9)

    def oneshots(board, col, name, i):
        # Rows: contact (with a wall), hit, spawn, death. Each plays once in 12
        # ticks and holds on its last frame. Contact and hit turn every 12 ticks.
        e = frames[name]
        t = i % 12
        turn = [0, 3, 6][(i // 12) % 3]
        if "contact" in e:
            f = e["contact"][turn][min(t, FRAMES - 1)]
            put(board, col, 0, f, wall_layer(f, ANIMS[name], turn))
        if "hit" in e:
            put(board, col, 1, e["hit"][(turn + 5) % 8][min(t, FRAMES - 1)])
        if "spawn" in e:
            put(board, col, 2, e["spawn"][min(t, FRAMES - 1)])
        if "death" in e:
            put(board, col, 3, e["death"][min(t, DEATH_FRAMES - 1)])

    gif(prev / "oneshots.gif", 36, 4, oneshots, 8)


# --------------------------------------------------------------------------
# Membrane protein overlays
# --------------------------------------------------------------------------
#
# A membrane protein sticks out of the cell membrane. It is a separate sprite,
# so the game can put it on any cell, at any slot. Each overlay has a frame of
# 56x56 px with the mount point at the centre (OVERLAY_C, OVERLAY_C), and 8
# baked directions: the direction in which the protein points, away from the
# cell. Place the mount point on the membrane, at about 0.95 x the cell radius
# from the cell centre.

OVERLAY_SIZE = 56
OVERLAY_C = 28
MOUNT_RADIUS = 0.95  # of the cell radius


def _overlay_canvas(d: int):
    """A canvas centred on the mount point, and the local axes for direction d.

    u runs away from the cell, v runs across. xy(u, v) gives design coordinates.
    """
    c = Canvas(OVERLAY_SIZE, OVERLAY_SIZE, origin=(OVERLAY_C, OVERLAY_C))
    a = d * np.pi / 4
    ca, sa = np.cos(a), np.sin(a)
    u = c.X * ca + c.Y * sa
    v = -c.X * sa + c.Y * ca

    def xy(u_, v_):
        return u_ * ca - v_ * sa, u_ * sa + v_ * ca

    return c, u, v, xy, a


def _socket(c: Canvas, x: float, y: float, tones, spin: float | None = None) -> None:
    """The base of a protein: a ring in the membrane, with a dark pore."""
    c.disc(x, y, 3.3, tones)
    c.paint(np.hypot(c.X - x, c.Y - y) < 1.3, tones[0])
    if spin is not None:  # six spokes on the ring: they show the motor turning
        for k in range(6):
            ang = spin + k * np.pi / 3
            c.plot(x + 2.3 * np.cos(ang), y + 2.3 * np.sin(ang), tones[4])


def _strand(c: Canvas, pts, tones, thick=1.2) -> None:
    """A thin protein fibre: a dark body with a light core."""
    for width, tone in ((thick, tones[2]), (thick * 0.6, tones[3])):
        for (x0, y0), (x1, y1) in zip(pts[:-1], pts[1:]):
            c.line(x0, y0, x1, y1, tone, width)


def _ellipse(c: Canvas, x, y, angle, rx, ry) -> np.ndarray:
    dx, dy = c.X - x, c.Y - y
    p = dx * np.cos(angle) + dy * np.sin(angle)
    q = -dx * np.sin(angle) + dy * np.cos(angle)
    return (p / rx) ** 2 + (q / ry) ** 2 < 1


def overlay_flagellum(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Flagellum: a motor in the membrane and a long helical filament.

    idle: a slow, small wave. use (a burst of thrust): the motor spins fast,
    the wave is large and quick, and specks stream off the tip. Point it away
    from the way you want to go.
    """
    use = mode == "use"
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["protein"]
    bx, by = xy(1.5, 0)
    _socket(c, bx, by, tones, spin=(2 if use else 1) * ph)
    amp, k, m, length = (3.8, 0.55, 2, 24) if use else (2.2, 0.42, 1, 25)
    n = 26
    pts = []
    for j in range(n + 1):
        s = 2 + (length - 2) * j / n
        env = min(1.0, (s - 2) / 7.0)  # the wave grows along the filament
        pts.append(xy(s, amp * env * np.sin(k * s - m * ph)))
    _strand(c, pts, tones, 1.3)
    # Bright beads run along the filament. They show the wave moving.
    spacing = 4 if use else 8
    for j in range(n + 1):
        if (j - i) % spacing == 0 and j > 2:
            c.plot(*pts[j], tones[4])
    c.plot(*pts[-1], tones[4])
    if use:  # specks stream off the tip
        for j, vs in enumerate((-2.2, 1.6, -0.6, 2.4)):
            sx, sy = xy(length + 1 + (3 * j + 2 * i) % 4, vs)  # period divides 8: seamless
            c.plot(sx, sy, tones[1])
    return c


USE_REACH = [0.0, 0.35, 0.75, 1.0, 1.0, 0.9, 0.85, 0.85]  # adhesin: how far the stalks reach
USE_PAD = [0.0, 0.0, 0.0, 0.3, 0.8, 1.0, 0.8, 0.6]  # adhesin: how flat the tips are
USE_GLOW = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.5, 0.0]  # adhesin: glow at the tips


def overlay_adhesin(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Adhesin: three sticky stalks with a knob on each.

    idle: the stalks sway. use (grab): they reach out, fan wide, and the knobs
    flatten into pads that glow. hold (stuck to something): taut stalks and
    pulsing pads. Mode `use` plays once. Mode `hold` loops while stuck.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["adhesin"]
    if mode == "use":
        reach = USE_REACH[i]
        length, spread, sway, bend = 12 + 7 * reach, 0.40 + 0.22 * reach, 0.04, 0.06
        pad, glow = USE_PAD[i], USE_GLOW[i]
    elif mode == "hold":
        length, spread, sway, bend = 18.5 + 0.6 * np.sin(ph), 0.62, 0.0, 0.0
        pad, glow = 0.75 + 0.2 * np.sin(ph), 0.0
    else:
        length, spread, sway, bend, pad, glow = 12.0, 0.40, 0.10, 0.10, 0.0, 0.0
    _socket(c, *xy(0, 0), tones)
    tips = []
    for idx, kk in enumerate((-1, 0, 1)):
        ln = length * (1.0 if kk == 0 else 0.85)
        ang = kk * spread + sway * np.sin(ph + idx * 2.1)
        pts = []
        for j in range(11):
            s = 2 + (ln - 2) * j / 10
            curve = bend * ln * np.sin(np.pi * j / 10) * np.sin(ph + idx * 1.3)
            pts.append(xy(s * np.cos(ang), s * np.sin(ang) + curve))
        _strand(c, pts, tones, 1.0)
        if mode == "hold":  # a bright pulse runs out along each taut stalk
            c.plot(*pts[(i + idx * 3) % 8 + 2], tones[4])
        tips.append((pts[-1], a + ang))
    for (x, y), screen_ang in tips:
        if pad < 0.05:  # a round knob
            c.disc(x, y, 1.9, tones)
        else:  # a flat pad, wider across the stalk
            mask = _ellipse(c, x, y, screen_ang, 1.4, 1.6 + 2.3 * pad)
            c.paint(c.dilate(mask) & ~mask & ~c.filled, tones[0])
            c.paint(mask, tones[3])
            c.paint(mask & (np.abs(-(c.X - x) * np.sin(screen_ang) + (c.Y - y) * np.cos(screen_ang)) < 0.7), tones[4])
    if glow > 0:
        for (x, y), _ in tips:
            c.halo(x, y, 3.0, 6.5, [tones[1], tones[2], tones[3]], 0.75 * glow)
    return c


NEEDLE = [19, 20, 23, 26, 24, 22, 20, 19]  # toxin: length of the needle in each use frame
RESERVOIR = [1.0, 1.0, 0.82, 0.62, 0.62, 0.78, 0.92, 1.0]  # toxin: size of the sac


def overlay_toxin(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Toxin: a sac on the membrane with an injector needle.

    idle: the sac breathes and a drop swells on the needle tip. use (fire): the
    sac squeezes, the needle thrusts out, and a spray of droplets leaves the
    tip. The game then spawns a `toxin_droplet` projectile.
    """
    use = mode == "use"
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["toxin"]
    sac = RESERVOIR[i] if use else 1 + 0.07 * np.sin(ph)
    sdf = (np.hypot((u - 3.5) / 6.4, v / (4.6 * sac)) - 1) * 4.4
    c.lit_body(sdf, tones, band=1.6, width=4.4)
    c.dot(*xy(1.5, -3.0), tones[4])
    c.dot(*xy(2.5, -3.0), tones[4])
    c.dot(*xy(0.5, -2.0), tones[4])
    c.outline(tones[0])
    length = NEEDLE[i] if use else 19 + 0.8 * np.sin(ph)
    # Needle: thick at the base and sharp at the tip, with a collar.
    tip = xy(length, 0)
    for j in range(12):
        s0, s1 = 9.0 + (length - 9.0) * j / 12, 9.0 + (length - 9.0) * (j + 1) / 12
        w = 1.3 - 0.9 * j / 11
        c.line(*xy(s0, 0), *xy(s1, 0), tones[2], w)
        c.line(*xy(s0, 0), *xy(s1, 0), tones[3], w * 0.55)
    c.disc(*xy(9.5, 0), 1.7, tones)
    if not use:  # a drop swells at the tip
        r = 0.7 + 0.8 * (0.5 + 0.5 * np.sin(ph))
        tx, ty = xy(length + 1.2, 0)
        c.paint(np.hypot(c.X - tx, c.Y - ty) < r, tones[3])
        c.plot(tx, ty, tones[4])
    elif 2 <= i <= 6:  # the spray
        angles = (-0.55, -0.33, -0.11, 0.11, 0.33, 0.55)
        speeds = (1.0, 1.2, 0.9, 1.1, 1.3, 0.95)
        for j, (ang, sp) in enumerate(zip(angles, speeds)):
            if i >= 5 and j % 2:
                continue  # the spray thins out
            dist = (i - 1) * 3.4 * sp
            x, y = xy(length + dist * np.cos(ang), dist * np.sin(ang))
            r = max(0.6, 1.5 - 0.2 * (i - 2))
            if r < 1.0:
                c.plot(x, y, tones[3])
            else:
                c.paint(np.hypot(c.X - x, c.Y - y) < r, tones[3])
                c.plot(x, y, tones[4])
    if use and i == 2:  # a flash at the tip as the needle fires
        for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            c.plot(tip[0] + ox * 1.5, tip[1] + oy * 1.5, tones[4])
    return c


def toxin_droplet(i: int = 0) -> Canvas:
    """The projectile that a toxin shot leaves. A round drop that pulses."""
    c = Canvas(12, 12)
    ph = 2 * np.pi * i / FRAMES
    sdf = np.hypot(c.X - 6, c.Y - 6) - 3.4 * (1 + 0.12 * np.sin(ph))
    c.lit_body(sdf, RAMPS["toxin"], width=3.4)
    c.dot(4.5, 4.5, RAMPS["toxin"][4])
    c.outline(RAMPS["toxin"][0])
    return c


# -- exchange and sensing proteins: transporter, lytic enzyme, fusion, receptor --

SUBSTRATES = {"carbon": "carbon", "mineral": "mineral", "metal": "metal"}  # name -> ramp


def _tri(c: Canvas, p0, p1, p2) -> np.ndarray:
    """Mask of a triangle, whichever way round the points go."""
    cross = []
    for (ax, ay), (bx, by) in ((p0, p1), (p1, p2), (p2, p0)):
        cross.append((bx - ax) * (c.Y - ay) - (by - ay) * (c.X - ax))
    return np.all([x >= 0 for x in cross], axis=0) | np.all([x <= 0 for x in cross], axis=0)


def _bead(c: Canvas, x: float, y: float, r: float, tones) -> None:
    """A small round thing with a highlight: a molecule."""
    c.paint(np.hypot(c.X - x, c.Y - y) < r, tones[3])
    c.paint(np.hypot(c.X - x, c.Y - y) < r * 0.55, tones[4])


def overlay_transporter(d: int = 0, i: int = 0, mode: str = "idle", substrate: str = "mineral") -> Canvas:
    """Transporter: a short barrel with a channel and two gates.

    It matches the pore in the world wall (same blue, same rungs in the channel).
    The colour of the molecules shows what the transporter takes: carbon (tan),
    mineral (cyan) or metal (gold).
    idle: the gates are shut and one molecule waits at the door. use: the gates
    are open, and molecules stream in through the channel. It loops while the
    transporter works.
    """
    use = mode == "use"
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["protein"]
    mol = RAMPS[SUBSTRATES[substrate]]
    uc, hl, hw, r = 3.2, 6.4, 5.8, 2.4
    dx, dy = np.abs(u - uc) - (hl - r), np.abs(v) - (hw - r)
    sdf = np.hypot(np.maximum(dx, 0), np.maximum(dy, 0)) + np.minimum(np.maximum(dx, dy), 0) - r
    c.lit_body(sdf, tones, band=1.4, width=5.8)
    # The channel, with rungs like the wall pore.
    slit = (np.abs(v) < 1.6) & (u > -3.2) & (u < 8.8)
    c.paint(slit, tones[0])
    c.paint(slit & (np.abs(v) < 0.7) & (np.abs((u - 0.3) % 2.6) < 0.6), tones[4])
    c.outline(tones[0])
    # Two gates at the outer end. They swing open for `use`.
    opening = 0.0 if not use else min(1.0, 0.45 + 0.55 * np.sin(ph + 1.2) ** 2 + 0.2)
    theta = np.radians(80) * opening
    for k in (-1, 1):
        px_, py_ = 8.6, k * 3.2
        ex, ey = px_ + 3.1 * np.sin(theta), py_ - k * 3.1 * np.cos(theta)
        c.line(*xy(px_, py_), *xy(ex, ey), tones[4], 0.95)
    if not use:  # a molecule waits at the shut door
        _bead(c, *xy(10.6 + 0.3 * np.sin(ph), 0), 1.8, mol)
    else:  # molecules go in, three at a time, 6 px apart. 8 frames = 6 px.
        for k in range(4):
            s = 21 - ((0.75 * i + 6 * k) % 24)
            side = 3.4 * np.sin(1.9 * s) * float(np.clip((s - 8.8) / 7.0, 0, 1))  # from s alone, so the loop is seamless
            if s > -3.0:
                _bead(c, *xy(s, side), 1.7, mol)
    return c


def overlay_lytic(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Lytic enzyme: a socket with three steel blades.

    It cuts a hole in a world wall. idle: the blades are nearly shut and breathe.
    use: they open wide and saw against each other, and sparks fly from the tips
    (the enzyme is eating the wall). It loops while it cuts.
    """
    use = mode == "use"
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["lytic"]
    _socket(c, *xy(0, 0), tones)
    spread = 0.55 if use else 0.30 + 0.04 * np.sin(ph)
    to_light = np.array([-0.6, -0.8])
    tips = []
    for k, (off, ln) in enumerate(((-1, 0.85), (0, 1.0), (1, 0.85))):
        ang = off * spread + (0.3 * np.sin(2 * ph + k * np.pi) if use else 0.0)
        length = (18.0 if use else 15.0) * ln
        ca, sa = np.cos(ang), np.sin(ang)
        base = np.array([3.0 * ca, 3.0 * sa])
        perp = np.array([-sa, ca])
        p1, p2 = base + 2.7 * perp, base - 2.7 * perp
        tip = np.array([length * ca, length * sa])
        pts = [xy(*p1), xy(*tip), xy(*p2)]
        blade = _tri(c, *pts)
        axis = np.array(pts[1]) - (np.array(pts[0]) + np.array(pts[2])) / 2
        side = (c.X - pts[1][0]) * axis[1] - (c.Y - pts[1][1]) * axis[0]
        lit = float(axis[0] * to_light[1] - axis[1] * to_light[0]) > 0  # which half faces the light
        c.paint(blade, tones[2])
        c.paint(blade & ((side > 0) == lit), tones[3])
        c.paint(blade & ((side > 0) != lit), tones[1])
        c.line(*(np.array(pts[0]) + np.array(pts[2])) / 2, *pts[1], tones[4], 0.55)
        tips.append(pts[1])
    c.outline(tones[0])
    if use:
        rng = np.random.RandomState(100 + i % 4)  # period 4 divides 8: seamless
        for x, y in tips:
            for _ in range(3):
                c.plot(x + rng.uniform(-3, 3), y + rng.uniform(-3, 3), FLASH if rng.rand() < 0.5 else tones[4])
            if i % 2 == 0:
                c.halo(x, y, 1.5, 4.2, [tones[1], tones[2], tones[3]], 0.6)
    return c


FUSION_REACH = [14, 17, 21, 24, 24, 21, 18, 16]  # fusion: stalk length in the use frames
FUSION_FOLD = [0, 0, 0, 0, 10, 70, 130, 150]  # fusion: how far the stalk folds back, in degrees
FUSION_GLOW = [0, 0, 0, 0, 1.0, 1.0, 0.6, 0.3]  # fusion: glow at the tip


def overlay_fusion(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Fusion protein: an ivory stalk with a head and a harpoon tip.

    It lets the cell join a membrane, so it can pass slowly through a wall or
    merge with a cell. idle: the stalk sways. use (plays once): the harpoon
    shoots out, sticks in the target, and the stalk folds back on itself (a
    hairpin), which pulls the two membranes together. hold (loops): the folded
    hairpin, pulsing while the membranes merge.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["fusion"]
    if mode == "use":
        length, fold, glow, sway = FUSION_REACH[i], np.radians(FUSION_FOLD[i]), FUSION_GLOW[i], 0.0
    elif mode == "hold":
        length, fold, glow, sway = 17.0, np.radians(150), 0.5 + 0.5 * np.sin(ph), 0.0
    else:
        length, fold, glow, sway = 15 + 0.7 * np.sin(ph), 0.0, 0.0, 0.07
    _socket(c, *xy(0, 0), tones)
    kink = 0.55 * length
    pts = []
    for j in range(15):
        s = 2 + (length - 2) * j / 14
        if s <= kink or fold == 0:
            pu, pv = s, 0.0
        else:  # past the kink the stalk turns by `fold`
            r = s - kink
            pu, pv = kink + r * np.cos(fold), r * np.sin(fold)
        pv += sway * np.sin(ph) * s * 0.6
        pts.append(xy(pu, pv))
    _strand(c, pts, tones, 1.9)
    hx_, hy_ = pts[-1]
    prev = np.array(pts[-2])
    heading = (np.array([hx_, hy_]) - prev) / (np.linalg.norm(np.array([hx_, hy_]) - prev) + 1e-9)
    c.disc(hx_, hy_, 3.0, tones)
    # The harpoon: a pointed tip that goes on from the head.
    tipx, tipy = hx_ + heading[0] * 4.5, hy_ + heading[1] * 4.5
    c.line(hx_, hy_, tipx, tipy, tones[3], 0.8)
    c.plot(tipx, tipy, tones[4])
    if glow > 0:
        c.halo(tipx, tipy, 2.5, 6.0, [tones[1], tones[2], tones[3]], 0.6 * glow)
    if mode == "hold":  # a bright pulse runs along the folded stalk
        c.plot(*pts[min((i * 2) % 16, 14)], tones[4])
    return c


def overlay_receptor(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Receptor: a stalk that forks into two cups.

    It senses what is outside. idle: it sways, and the cups are empty. use (plays
    once): a molecule drifts in, drops into a cup, the cup lights up, and a
    signal runs down the stalk into the cell.
    """
    use = mode == "use"
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    tones = RAMPS["receptor"]
    sway = 0.0 if use else 0.07 * np.sin(ph)
    spread = 0.62 + (0.0 if use else 0.04 * np.sin(ph + 1.0))
    _socket(c, *xy(0, 0), tones)
    junction = (11.0, sway * 11.0 * 0.6)
    _strand(c, [xy(2 + 9 * j / 9, sway * (2 + 9 * j / 9) * 0.6) for j in range(10)], tones, 1.7)
    cups = []
    for sign in (-1, 1):
        ang = sign * spread
        end = (junction[0] + 8.0 * np.cos(ang), junction[1] + 8.0 * np.sin(ang))
        _strand(c, [xy(*junction), xy(*end)], tones, 1.4)
        cups.append(end)
    lit_cup = 1  # the molecule goes into the cup on the +v side
    for k, end in enumerate(cups):
        x, y = xy(*end)
        c.disc(x, y, 2.9, tones)
        c.paint(np.hypot(c.X - x, c.Y - y) < 1.5, tones[0])
        if use and k == lit_cup and i >= 3:  # the cup lights up while it holds the molecule
            c.paint(np.hypot(c.X - x, c.Y - y) < 1.5, tones[4])
            if i <= 6:
                c.halo(x, y, 2.9, 6.5, [tones[1], tones[2], tones[3]], 0.8 if i < 6 else 0.4)
    if use:
        cup = cups[lit_cup]
        start = np.array([25.0, 10.0])
        if i <= 3:  # the molecule drifts in and drops into the cup
            s = i / 3.0
            pos = start + (np.array(cup) - start) * (1 - (1 - s) ** 2)
            _bead(c, *xy(*pos), 2.0, tones)
        signal_u = {4: 10.0, 5: 6.5, 6: 3.0, 7: -0.5}
        if i in signal_u:  # the signal runs down the stalk and into the cell
            su = signal_u[i]
            for step, tone in enumerate((tones[4], tones[3], tones[2])):
                sx, sy = xy(su + 2.5 * step, 0)
                c.paint(np.hypot(c.X - sx, c.Y - sy) < (1.9 - 0.4 * step), tone)
        if i == 7:
            sx, sy = xy(0, 0)
            c.halo(sx, sy, 3.5, 8.0, [tones[1], tones[2], tones[3]], 0.8)
    return c


# -- immune proteins: MHC I and II, T cell receptor, antibody, perforin --------
#
# The body of every immune protein is pearl, a neutral grey-white (white blood
# cell). The colour is in the cargo, the antigen: a piece of something, in the
# colour of where it comes from (rule 5). self = player mint, viral = virus
# lilac, prey = prey periwinkle. The same pearl body then shows what the cell
# presents, what its receptor looks for, or what its antibody tags.

IMMUNE = "immune"
ANTIGENS = {"self": "player", "viral": "virus", "prey": "prey"}  # antigen -> ramp of its origin


def _rbox(u, v, uc, vc, hu, hv, r) -> np.ndarray:
    """Distance to a rounded box, in the local (u, v) axes of an overlay."""
    dx, dy = np.abs(u - uc) - (hu - r), np.abs(v - vc) - (hv - r)
    return np.hypot(np.maximum(dx, 0), np.maximum(dy, 0)) + np.minimum(np.maximum(dx, dy), 0) - r


def _part(c: Canvas, sdf, tones, band=None, width=None) -> np.ndarray:
    """A lit part with a dark edge where it lies on top of what is already drawn."""
    before = c.filled.copy()
    mask = c.lit_body(sdf, tones, band=band, width=width)
    c.paint(c.dilate(mask) & ~mask & before, tones[0])
    return mask


def _chain(c: Canvas, xy, uvs, pep, bright=()) -> None:
    """A short chain of beads: a peptide (a piece of a protein)."""
    pts = [xy(*p) for p in uvs]
    for x, y in pts:
        c.paint(np.hypot(c.X - x, c.Y - y) < 1.45, pep[1])
    for j, (x, y) in enumerate(pts):
        c.paint(np.hypot(c.X - x, c.Y - y) < 0.95, pep[4] if j in bright else pep[3])


def _line_uv(u0: float, step: float, n: int, vamp: float = 0.45, vofs: float = 0.0):
    """n bead positions along u with a small zigzag across."""
    return [(u0 + step * j, vofs + (vamp if j % 2 else -vamp)) for j in range(n)]


MHC1_USE = [  # (bead on the stalk, beads in the groove, ridge flash, glow)
    (2.5, 0, 0, 0.0), (5.5, 0, 0, 0.0), (8.5, 0, 0, 0.0), (11.2, 0, 0, 0.0),
    (None, 2, 0, 0.0), (None, 4, 0, 0.0), (None, 6, 1, 0.3), (None, 6, 0, 0.55),
]


def overlay_mhc1(d: int = 0, i: int = 0, mode: str = "idle", antigen: str = "viral") -> Canvas:
    """MHC class I: a stalk with a flat head, a groove and a small ball (beta-2 microglobulin).

    It shows a piece of what is made inside the cell, so others can check it.
    The peptide colour says what it is: self (mint) or viral (lilac).
    idle: the head sways, the groove is empty. use (plays once): a peptide
    climbs the stalk, slides into the groove, the ridges flash. hold (loops):
    the peptide is on show, a bright bead runs along it, the head glows.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    t, pep = RAMPS[IMMUNE], RAMPS[ANTIGENS[antigen]]
    floor = [t[0], t[0], t[1], t[2], t[3]]  # a darker tone set: the floor of the groove
    sway = 0.5 * np.sin(ph) if mode == "idle" else 0.0
    bead, n, flash, glow = MHC1_USE[i] if mode == "use" else (None, 0 if mode == "idle" else 6, 0, 0.0)
    if mode == "hold":
        glow = 0.3 + 0.25 * (0.5 + 0.5 * np.sin(ph))
    _socket(c, *xy(0, 0), t)
    c.disc(*xy(8.6, 3.6), 2.1, t)  # beta-2 microglobulin
    _part(c, capsule_sdf(c, *xy(1.5, 0), *xy(10.5, sway), 1.5), t, width=1.5)
    _part(c, _rbox(u, v, 15.5, sway, 5.2, 5.4, 1.6), floor, width=3.0)
    for s in (-1, 1):  # the two helices of the groove
        _part(c, capsule_sdf(c, *xy(12.0, s * 3.5 + sway), *xy(19.0, s * 3.5 + sway), 1.3), t, width=1.3)
        if flash:
            c.line(*xy(12.4, s * 3.5 + sway), *xy(18.6, s * 3.5 + sway), t[4], 0.45)
    if n:
        bright = (i % 8,) if mode == "hold" else ()
        _chain(c, xy, _line_uv(12.2, 1.4, 6, vofs=sway)[:n], pep, bright)
    c.outline(t[0])
    if bead is not None:  # the peptide climbs the stalk
        _chain(c, xy, [(bead, 0.0)], pep, (0,))
    if glow > 0:
        c.halo(*xy(15.5, sway), 6.0, 11.5, [pep[1], pep[2], pep[3]], glow)
    return c


MHC2_USE = [  # (placeholder lift, antigen slide in px, ridge flash, glow)
    (1.5, None, 0, 0.0), (3.5, None, 0, 0.0), (None, None, 0, 0.0), (None, 9.0, 0, 0.0),
    (None, 6.0, 0, 0.0), (None, 3.0, 0, 0.0), (None, 0.0, 1, 0.3), (None, 0.0, 0, 0.55),
]


def overlay_mhc2(d: int = 0, i: int = 0, mode: str = "idle", antigen: str = "prey") -> Canvas:
    """MHC class II: two stalks, a wide head and an open groove.

    It shows a piece of what the cell ate (see the engulf animation), so
    prey (periwinkle) is its usual cargo. The long peptide sticks out of both
    ends of the groove. idle: a grey placeholder sits in the groove. use
    (plays once): the placeholder is thrown out and the real peptide slides
    in from the outside. hold (loops): the peptide on show.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    t, pep = RAMPS[IMMUNE], RAMPS[ANTIGENS[antigen]]
    floor = [t[0], t[0], t[1], t[2], t[3]]
    sway = 0.5 * np.sin(ph) if mode == "idle" else 0.0
    lift, slide, flash, glow = MHC2_USE[i] if mode == "use" else (None, None, 0, 0.0)
    if mode == "hold":
        slide, glow = 0.0, 0.3 + 0.25 * (0.5 + 0.5 * np.sin(ph))
    _socket(c, *xy(0, 0), t)
    for s in (-1, 1):
        _part(c, capsule_sdf(c, *xy(1.5, s * 1.9), *xy(10.0, s * 2.4 + sway), 1.05), t, width=1.05)
    _part(c, _rbox(u, v, 15.8, sway, 6.0, 6.3, 1.7), floor, width=3.0)
    for s in (-1, 1):
        _part(c, capsule_sdf(c, *xy(12.0, s * 4.4 + sway), *xy(19.6, s * 4.4 + sway), 1.3), t, width=1.3)
        if flash:
            c.line(*xy(12.4, s * 4.4 + sway), *xy(19.2, s * 4.4 + sway), t[4], 0.45)
    if mode == "idle" or (mode == "use" and slide is None):
        # The grey placeholder (CLIP). In `use` it lifts away and breaks up.
        up = 0.0 if lift is None else lift
        if lift is None and mode == "use":  # frame 2: only bits are left
            for k in range(4):
                c.plot(*xy(13.0 + 2.0 * k, 6.5 + 0.8 * (k % 2)), t[3])
            c.plot(*xy(15.8, 0), FLASH)
        else:
            beads = _line_uv(12.6, 1.5, 5, vofs=sway + up)
            if lift is not None and lift > 2.0:
                beads = beads[::2]
            _chain(c, xy, beads, t)
    else:
        off = 0.0 if slide is None else slide
        beads = [(uu + off, vv + sway) for uu, vv in _line_uv(10.0, 1.65, 9)]
        beads = [b for b in beads if b[0] < 26.5]
        bright = (i % 8,) if mode == "hold" else ()
        _chain(c, xy, beads, pep, bright)
    c.outline(t[0])
    if glow > 0:
        c.halo(*xy(15.8, sway), 6.5, 12.0, [pep[1], pep[2], pep[3]], glow)
    return c


TCR_PARTNER = [(27.0, 7.0), (24.0, 4.5), (21.0, 2.0), (18.8, 0.6), (17.8, 0.0), (17.8, 0.0), (17.8, 0.0), (17.8, 0.0)]


def overlay_tcr(d: int = 0, i: int = 0, mode: str = "idle", antigen: str = "viral") -> Canvas:
    """T cell receptor: a stalk with a cup at the tip, and two signal knobs (CD3).

    It looks for one antigen, so the inner rim of the cup has the colour of the
    antigen it matches. idle: the cup sways and waits. use (plays once): a
    small MHC head with that antigen drifts in, the cup clamps on it, the knobs
    light up, and a signal runs down the stalk into the cell. hold (loops):
    still clamped, the knobs light in turn, a pulse runs down the stalk.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    t, pep = RAMPS[IMMUNE], RAMPS[ANTIGENS[antigen]]
    floor = [t[0], t[0], t[1], t[2], t[3]]
    swing = round(0.10 * np.sin(ph), 9) if mode == "idle" else 0.0  # rounded: sin(2 pi) must be exactly 0
    ca, sa = np.cos(swing), np.sin(swing)

    def rot(uu, vv):
        return uu * ca - vv * sa, uu * sa + vv * ca

    _socket(c, *xy(0, 0), t)
    knobs = {1: i >= 5 if mode == "use" else mode == "hold" and (i // 2) % 2 == 0,
             -1: i >= 5 if mode == "use" else mode == "hold" and (i // 2) % 2 == 1}
    for s in (-1, 1):
        kx, ky = xy(4.6, s * 3.3)
        c.disc(kx, ky, 1.5, t)
        if knobs[s]:
            c.paint(np.hypot(c.X - kx, c.Y - ky) < 1.0, t[4])
    hc, pc = xy(*rot(13.5, 0)), xy(*rot(16.6, 0))
    _part(c, capsule_sdf(c, *xy(1.5, 0), *hc, 1.5), t, width=1.5)
    if mode != "idle":  # the partner: a small MHC head with the antigen
        pu, pv = TCR_PARTNER[i] if mode == "use" else TCR_PARTNER[-1]
        _part(c, _rbox(u, v, pu, pv, 2.3, 2.6, 1.2), floor, width=2.3)
        _chain(c, xy, [(pu - 0.8, pv), (pu + 0.8, pv)], pep)
    d_hc = np.hypot(c.X - hc[0], c.Y - hc[1])
    d_pc = np.hypot(c.X - pc[0], c.Y - pc[1])
    cup = _part(c, np.maximum(d_hc - 4.3, -(d_pc - 3.0)), t, width=3.0)
    c.paint(cup & (d_pc >= 3.0) & (d_pc < 4.1) & ((c.X - hc[0]) * (pc[0] - hc[0]) + (c.Y - hc[1]) * (pc[1] - hc[1]) > -0.5), pep[3])
    c.plot(*xy(*rot(14.2, 0)), pep[4])
    c.outline(t[0])
    if mode == "use" and i == 4:  # the clamp: a flash at the contact
        px_, py_ = xy(17.8, 0)
        for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            c.plot(px_ + ox * 3.8, py_ + oy * 3.8, FLASH)
    if mode == "use" and 4 <= i <= 6 or mode == "hold":
        c.halo(*xy(17.8, 0), 3.0, 7.0, [pep[1], pep[2], pep[3]], 0.6 if mode == "use" else 0.35)
    if mode == "use" and i >= 5:
        su = {5: 7.5, 6: 4.5, 7: 1.5}[i]  # the signal runs down the stalk
        for step, tone in enumerate((t[4], t[3], t[2])):
            sx, sy = xy(su + 2.2 * step, 0)
            c.paint(np.hypot(c.X - sx, c.Y - sy) < (1.5 - 0.3 * step), tone)
    if mode == "hold":
        sx, sy = xy(8.4 - 1.0 * (i % 8), 0)
        c.paint(np.hypot(c.X - sx, c.Y - sy) < 1.2, t[4])
    return c


AB_THETA = [0.72, 0.90, 1.00, 0.85, 0.50, 0.38, 0.38, 0.38]  # antibody use: angle of the arms
AB_BLOB = [28.0, 25.0, 22.0, 19.6, 18.6, 18.6, 18.6, 18.6]  # antibody use: where the antigen is


def overlay_antibody(d: int = 0, i: int = 0, mode: str = "idle", antigen: str = "viral") -> Canvas:
    """Antibody: a Y with a stem in the membrane and two arms.

    The tip of each arm has the colour of the antigen it fits. idle: the arms
    flex. use (plays once): the arms open, a piece of the antigen drifts in
    between them, the arms close on it and it is tagged. hold (loops): the
    tagged antigen stays in the grip, pulsing.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    t, pep = RAMPS[IMMUNE], RAMPS[ANTIGENS[antigen]]
    blob_r, blob_u, glow = 2.5, None, 0.0
    if mode == "use":
        thetas = [AB_THETA[i]] * 2
        blob_u = AB_BLOB[i]
        glow = {6: 0.45, 7: 0.6}.get(i, 0.0)
    elif mode == "hold":
        thetas = [0.38, 0.38]
        blob_u, blob_r = 18.6, 2.5 + 0.25 * np.sin(ph)
        glow = 0.3 + 0.25 * (0.5 + 0.5 * np.sin(ph))
    else:
        thetas = [0.62 + 0.10 * np.sin(ph), 0.62 + 0.10 * np.sin(ph + 1.2)]
    _socket(c, *xy(0, 0), t)
    if blob_u is not None:  # the antigen: a ball with spikes
        bx, by = xy(blob_u, 0)
        for k in range(6):
            ang = a + k * np.pi / 3 + 0.26
            c.line(bx, by, bx + (blob_r + 1.5) * np.cos(ang), by + (blob_r + 1.5) * np.sin(ang), pep[2], 0.55)
        c.lit_body(np.hypot(c.X - bx, c.Y - by) - blob_r, pep, width=blob_r)
        c.paint(c.dilate(np.hypot(c.X - bx, c.Y - by) < blob_r) & (np.hypot(c.X - bx, c.Y - by) >= blob_r), pep[0])
        c.plot(bx - blob_r * 0.35, by - blob_r * 0.4, pep[4])
    _part(c, capsule_sdf(c, *xy(1.5, 0), *xy(9.5, 0), 1.35), t, width=1.35)
    _part(c, capsule_sdf(c, *xy(4.5, 0), *xy(8.0, 0), 1.9), t, width=1.9)
    tips = []
    for s, th in zip((-1, 1), thetas):
        end = xy(9.5 + 8.5 * np.cos(th), s * 8.5 * np.sin(th))
        _part(c, capsule_sdf(c, *xy(9.5, 0), *end, 1.2), t, width=1.2)
        tips.append(end)
    c.disc(*xy(9.5, 0), 1.8, t)
    for ex, ey in tips:  # the antigen-binding tips
        c.paint(np.hypot(c.X - ex, c.Y - ey) < 1.4, pep[3])
        c.plot(ex, ey, pep[4])
    c.outline(t[0])
    if glow > 0 and blob_u is not None:
        c.halo(*xy(blob_u, 0), blob_r + 0.5, blob_r + 5.5, [pep[1], pep[2], pep[3]], glow)
    return c


PERFORIN_ARRIVE = {2: 2, 6: 2, 1: 4, 7: 4, 0: 6}  # ring part -> frame in which it clicks into the ring
PERFORIN_RING = (14.5, 5.0)  # centre u, radius


def overlay_perforin(d: int = 0, i: int = 0, mode: str = "idle") -> Canvas:
    """Perforin: a short stalk and a ring made of eight parts.

    It punches a hole in a membrane. idle: three parts of the ring (a C) rock
    gently, and small dots mark the places of the others. use (plays once): the
    other five parts fly in, two, then two, then one, and click into the ring.
    The ring closes with a flash, and a dark pore with a red rim opens in the
    middle. hold (loops): the closed ring, a bright part runs round it, and a
    red glow breathes.
    """
    c, u, v, xy, a = _overlay_canvas(d)
    ph = 2 * np.pi * i / FRAMES
    t, red = RAMPS[IMMUNE], RAMPS["predator"]
    cu, R = PERFORIN_RING
    _socket(c, *xy(0, 0), t)
    _part(c, capsule_sdf(c, *xy(1.5, 0), *xy(cu - R, 0), 1.4), t, width=1.4)
    closed = mode == "hold" or (mode == "use" and i >= 6)
    for k in range(8):
        th = k * np.pi / 4
        r = R
        if mode == "idle":
            if k not in (3, 4, 5):
                c.plot(*xy(cu + R * np.cos(th), R * np.sin(th)), t[1])
                continue
            r = R + 0.35 * np.sin(ph + 0.8 * k)
        elif mode == "use" and k in PERFORIN_ARRIVE:
            p = (i - (PERFORIN_ARRIVE[k] - 2)) / 2.0
            if p < 0:
                continue
            p = min(p, 1.0)
            r = R + 7.0 * (1 - p) ** 2  # fast at first, then it settles
        c.disc(*xy(cu + r * np.cos(th), r * np.sin(th)), 2.0, t)
    if closed:
        cx_, cy_ = xy(cu, 0)
        dist = np.hypot(c.X - cx_, c.Y - cy_)
        c.paint(dist < 2.6, red[0])
        c.paint((dist >= 1.5) & (dist < 2.6), red[2] if (mode == "use" or i % 2 == 0) else red[1])
        if mode == "hold":
            bx, by = xy(cu + R * np.cos(i * np.pi / 4), R * np.sin(i * np.pi / 4))
            c.paint(np.hypot(c.X - bx, c.Y - by) < 1.1, t[4])
    c.outline(t[0])
    if mode == "use" and i == 6:  # the ring closes: a flash
        fx, fy = xy(cu, 0)
        for ox, oy in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
            c.plot(fx + ox * 6.3, fy + oy * 6.3, FLASH)
    if closed:
        glow = 0.55 if mode == "use" else 0.25 + 0.2 * (0.5 + 0.5 * np.sin(ph))
        c.halo(*xy(cu, 0), R + 2.0, R + 7.5, [red[1], red[2], red[3]], glow)
    return c


IMMUNE_KINDS = {"idle": dict(fps=6, loop=True), "use": dict(fps=12, loop=False), "hold": dict(fps=8, loop=True)}
IMMUNE_NOTES = {
    "mhc1": "use = a peptide from inside loads into the groove (plays once). hold = on show, until it is withdrawn.",
    "mhc2": "use = the placeholder is thrown out and a peptide from an eaten cell slides in (plays once). hold = on show.",
    "tcr": "The cup rim shows the antigen it matches. use = clamps on an MHC with that antigen (plays once). hold = signalling.",
    "antibody": "The arm tips show the antigen it fits. use = catches and tags the antigen (plays once). hold = tagged, in the grip.",
    "perforin": "use = the ring closes and a pore opens (plays once). hold = the closed ring, active.",
}


# name -> what to draw and how it plays
OVERLAYS = {
    "overlay_flagellum": dict(
        make=overlay_flagellum, layer="above",
        kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=14, loop=True)},
        note="Point it away from the way you want to go. use = a burst of thrust, loops while it lasts."),
    "overlay_adhesin": dict(
        make=overlay_adhesin, layer="above",
        kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=12, loop=False),
               "hold": dict(fps=8, loop=True)},
        note="use = the grab (plays once). hold = stuck to something (loops until released)."),
    "overlay_toxin": dict(
        make=overlay_toxin, layer="above",
        kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=14, loop=False)},
        note="use = fire (plays once). Spawn a toxin_droplet projectile at the needle tip."),
    **{
        f"overlay_transporter_{sub}": dict(
            make=(lambda d, i, mode, sub=sub: overlay_transporter(d, i, mode, sub)), layer="above", sheet=2,
            kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=12, loop=True)},
            note=f"Takes {sub}. Match it to a wall pore of the same substrate. use loops while it takes up.")
        for sub in SUBSTRATES
    },
    "overlay_lytic": dict(
        make=overlay_lytic, layer="above", sheet=2,
        kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=14, loop=True)},
        note="Cuts a wall. use loops while it cuts (it costs a cofactor for each use)."),
    "overlay_fusion": dict(
        make=overlay_fusion, layer="above", sheet=2,
        kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=12, loop=False), "hold": dict(fps=8, loop=True)},
        note="use = the harpoon and the fold (plays once). hold = membranes merging (loops until done)."),
    "overlay_receptor": dict(
        make=overlay_receptor, layer="above", sheet=2,
        kinds={"idle": dict(fps=6, loop=True), "use": dict(fps=12, loop=False)},
        note="use = a molecule binds and the signal runs into the cell (plays once)."),
    **{
        f"overlay_{kind}_{ag}": dict(
            make=(lambda d, i, mode, fn=fn, ag=ag: fn(d, i, mode, ag)), layer="above", sheet=3,
            preview=(ag == "viral"),
            kinds=IMMUNE_KINDS, note=f"{IMMUNE_NOTES[kind]} Antigen: {ag}.")
        for kind, fn in (("mhc1", overlay_mhc1), ("mhc2", overlay_mhc2), ("tcr", overlay_tcr),
                         ("antibody", overlay_antibody))
        for ag in ANTIGENS
    },
    "overlay_perforin": dict(
        make=overlay_perforin, layer="above", sheet=3, kinds=IMMUNE_KINDS, note=IMMUNE_NOTES["perforin"]),
}


def build_overlays() -> dict:
    """{name: {kind: [[Canvas] for each direction]}}"""
    return {
        name: {
            kind: [[spec["make"](d, i, kind) for i in range(FRAMES)] for d in range(len(DIRECTIONS))]
            for kind in spec["kinds"]
        }
        for name, spec in OVERLAYS.items()
    }


def write_overlays(frames: dict) -> None:
    """Write the overlay sheets, and add them to manifest.json."""
    path = ANIM_DIR / "manifest.json"
    manifest = json.loads(path.read_text())
    for name, kinds in frames.items():
        spec = OVERLAYS[name]
        info = {
            "frame_size": [OVERLAY_SIZE, OVERLAY_SIZE],
            "anchor_px": [OVERLAY_C, OVERLAY_C],
            "layer": spec["layer"],
            "mount": f"Put anchor_px on the membrane, {MOUNT_RADIUS} x the cell radius from the "
                     "cell centre, in the direction of the row.",
            "note": spec["note"],
        }
        for kind, grid in kinds.items():
            write_png(ANIM_DIR / f"{name}_{kind}.png", _grid(grid))
            info[kind] = {
                "file": f"{name}_{kind}.png", "columns": FRAMES, "rows": len(DIRECTIONS),
                "fps": spec["kinds"][kind]["fps"], "loop": spec["kinds"][kind]["loop"],
                "directions": DIRECTIONS,
                "direction_meaning": "direction the protein points, away from the cell",
            }
        manifest[name] = info
    # The projectile: 8 frames, no directions.
    drop = [toxin_droplet(i) for i in range(FRAMES)]
    write_png(ANIM_DIR / "toxin_droplet.png", np.concatenate([f.px for f in drop], axis=1))
    manifest["toxin_droplet"] = {
        "frame_size": [12, 12], "anchor_px": [6, 6],
        "loop": {"file": "toxin_droplet.png", "columns": FRAMES, "rows": 1, "fps": 8, "loop": True},
    }
    path.write_text(json.dumps(manifest, indent=2) + "\n")


OVERLAY_DEMOS = {
    # name of the file -> overlays on the player: {overlay: direction}, and the modes of each phase
    "overlay_demo": dict(
        mounts={"overlay_flagellum": 4, "overlay_toxin": 0, "overlay_adhesin": 2},
        phases=[
            dict(overlay_flagellum="idle", overlay_toxin="idle", overlay_adhesin="idle"),
            dict(overlay_flagellum="use", overlay_toxin="use", overlay_adhesin="use"),
            dict(overlay_flagellum="use", overlay_toxin="idle", overlay_adhesin="hold"),
        ]),
    "overlay_demo_2": dict(
        mounts={"overlay_transporter_mineral": 4, "overlay_lytic": 0, "overlay_fusion": 2, "overlay_receptor": 6},
        phases=[
            dict(overlay_transporter_mineral="idle", overlay_lytic="idle", overlay_fusion="idle", overlay_receptor="idle"),
            dict(overlay_transporter_mineral="use", overlay_lytic="use", overlay_fusion="use", overlay_receptor="use"),
            dict(overlay_transporter_mineral="use", overlay_lytic="idle", overlay_fusion="hold", overlay_receptor="idle"),
        ]),
    # The immune proteins. Phases are all idle, all use, all hold (see write_overlay_previews).
    "overlay_demo_3": dict(
        mounts={"overlay_mhc1_viral": 0, "overlay_mhc2_prey": 2, "overlay_mhc1_self": 4, "overlay_antibody_viral": 6}),
    "overlay_demo_4": dict(
        mounts={"overlay_tcr_viral": 0, "overlay_perforin": 2, "overlay_antibody_prey": 4, "overlay_tcr_prey": 6}),
}


def write_overlay_previews(frames: dict, cell_frames: list[Canvas]) -> None:
    prev = PREVIEW_DIR / "anim"
    # Sheets: one row for each overlay and kind, pointing east. Group 2 has its own files.
    for sheet, suffix in ((1, ""), (2, "_2"), (3, "_3")):
        names = [n for n in frames if OVERLAYS[n].get("sheet", 1) == sheet and OVERLAYS[n].get("preview", True)]
        rows = [np.concatenate([f.px for f in grid[0]], axis=1) for n in names for grid in frames[n].values()]
        write_png(prev / f"overlays{suffix}.png", on_water(np.concatenate(rows, axis=0), 3))
        # Eight directions of one use frame, to check the rotation.
        rows = [np.concatenate([frames[n]["use"][d][3].px for d in range(8)], axis=1) for n in names]
        write_png(prev / f"overlay_directions{suffix}.png", on_water(np.concatenate(rows, axis=0), 3))

    # The immune proteins with each antigen: rows are the proteins, columns the
    # antigens. Frame 5 of `hold` pointing east, and frame 5 of `use`.
    rows = []
    for kind in ("mhc1", "mhc2", "tcr", "antibody"):
        rows.append(np.concatenate(
            [frames[f"overlay_{kind}_{ag}"][k][0][5].px for k in ("use", "hold") for ag in ANTIGENS], axis=1))
    write_png(prev / "overlay_antigens_3.png", on_water(np.concatenate(rows, axis=0), 3))

    # The overlays on the player.
    size, scale = 160, 3
    cx = cy = size / 2
    c0 = cell_frames[0]
    anchor = (24 + c0.ox, 24.5 + c0.oy)

    def board(i: int, mounts: dict, modes: dict) -> np.ndarray:
        px = np.zeros((size, size, 4), dtype=np.uint8)

        def put(f: Canvas, ax: float, ay: float, pivot) -> None:
            x0, y0 = int(round(ax - pivot[0])), int(round(ay - pivot[1]))
            for oy in range(f.h):
                for ox in range(f.w):
                    if f.px[oy, ox, 3] and 0 <= x0 + ox < size and 0 <= y0 + oy < size:
                        px[y0 + oy, x0 + ox] = f.px[oy, ox]

        def overlay(name: str, layer: str) -> None:
            if OVERLAYS[name]["layer"] != layer:
                return
            d = mounts[name]
            ang = d * np.pi / 4
            f = frames[name][modes[name]][d][i % FRAMES]
            put(f, cx + 19 * np.cos(ang), cy + 19 * np.sin(ang), (OVERLAY_C, OVERLAY_C))

        for name in mounts:
            overlay(name, "below")
        put(cell_frames[i % FRAMES], cx, cy, anchor)
        for name in mounts:
            overlay(name, "above")
        return px

    for file, demo in OVERLAY_DEMOS.items():
        mounts = demo["mounts"]
        phases = demo.get("phases") or [{n: k for n in mounts} for k in ("idle", "use", "hold")]
        write_png(prev / f"{file}.png", on_water(np.concatenate(
            [board(3, mounts, phases[0]), board(3, mounts, phases[1]), board(5, mounts, phases[2])], axis=1), 3, 1))
        if shutil.which("magick") is None:
            print("magick not found: skipped the overlay GIF")
            continue
        with tempfile.TemporaryDirectory() as tmp:
            for n in range(24):
                write_png(Path(tmp) / f"f{n:02d}.png", on_water(board(n, mounts, phases[n // 8]), scale, 1))
            subprocess.run(
                ["magick", "-delay", "9", "-loop", "0", f"{tmp}/f*.png", "-layers", "optimize",
                 str(prev / f"{file}.gif")],
                check=True,
            )


# --------------------------------------------------------------------------
# Engulf, digest, and the gate cell victory
# --------------------------------------------------------------------------
#
# Engulf is drawn for a pair: an engulfer and the smaller cell it eats. The
# two cells touch each other in the frame, so each pair has its own sheets. To
# add a pair, add a line to PAIRS.
#
# The game plays three things in order:
#   engulf      (one-shot) the engulfer wraps the target and pulls it in
#   digest      (loop)     the target sits in a vacuole, while it is digested
#   digest_end  (one-shot) the vacuole closes, and the gene goes to the nucleoid
# All three have 8 rows: the direction from the engulfer to the target.

PAIRS = {
    "engulf_player_prey": dict(engulfer="player_cell", prey="prey_bacillus"),
    "engulf_player_symbiont": dict(engulfer="player_cell", prey="symbiont_cell"),
}

ENGULF_FRAMES = 12
DIGEST_END_FRAMES = 10
ENGULF_SIZE = 112  # frame of the engulf sheet. The engulfer centre is at the middle.
# Frames 0-6: the arms grow. Frame 7: the ring is closed. Frames 8-11: the vacuole sinks in.
ENGULF_DIST = [2, 0, 0, 0, -1, -1, -2]  # target distance, added to the touching distance
ENGULF_STRETCH = [0.04, 0.08, 0.12, 0.14, 0.14, 0.12, 0.10, 0.08, 0.10, 0.06, -0.05, 0.02]
ENGULF_EXTENT = [0.0, 0.0, 0.15, 0.40, 0.65, 0.85, 1.0]  # how far the arms reach round the target
VACUOLE_Q = {7: 0.0, 8: 0.25, 9: 0.55, 10: 0.8, 11: 1.0}  # progress of the sink-in
VACUOLE_DIST = 10.0  # the final vacuole, from the engulfer centre
VACUOLE_R = 7.0
VACUOLE_SCALE = 0.5  # size of the target inside the vacuole
NUCLEOID = (21.5, 23.0)  # the player's nucleoid, in design coordinates


def _ease(q: float) -> float:
    return 1 - (1 - q) ** 2


def _paste(dst: Canvas, src: Canvas, src_anchor, point) -> None:
    """Copy the pixels of `src` onto `dst`, so that src_anchor (a pixel in src)
    lands on `point` (design coordinates of dst)."""
    x0 = int(round(point[0] + dst.ox - src_anchor[0]))
    y0 = int(round(point[1] + dst.oy - src_anchor[1]))
    for oy in range(src.h):
        ty = y0 + oy
        if not 0 <= ty < dst.h:
            continue
        for ox in range(src.w):
            tx = x0 + ox
            if 0 <= tx < dst.w and src.px[oy, ox, 3]:
                dst.px[ty, tx] = src.px[oy, ox]


def _paste_cell(dst: Canvas, frame: Canvas, centre, point) -> None:
    _paste(dst, frame, (centre[0] + frame.ox, centre[1] + frame.oy), point)


def _fade(c: Canvas, amount: float) -> None:
    """Dither a frame away: amount 0 = all there, 1 = gone."""
    bayer = np.tile(BAYER4, (c.h // 4 + 1, c.w // 4 + 1))[: c.h, : c.w]
    c.px[(1 - amount) <= bayer, 3] = 0


def _arms(c: Canvas, centre, psi: float, radius: float, extent: float, tones) -> None:
    """Two arms of the engulfer that curl round a point.

    `psi` is the angle from the point towards the engulfer. The arms start at
    20 degrees either side of it and grow round to the far side as extent goes
    from 0 to 1. They are thicker at the base.
    """
    if extent <= 0.01:
        return
    dx, dy = c.X - centre[0], c.Y - centre[1]
    r = np.hypot(dx, dy)
    alpha = np.abs((np.arctan2(dy, dx) - psi + np.pi) % (2 * np.pi) - np.pi)  # 0..pi
    a0 = np.radians(20)
    a1 = a0 + extent * (np.pi - a0)
    u = np.clip((alpha - a0) / max(a1 - a0, 1e-6), 0, 1)
    half = 2.9 * (1 - 0.4 * u)
    sdf = np.where((alpha >= a0) & (alpha <= a1), np.abs(r - radius) - half, 99.0)
    for sign in (-1, 1):  # round tips
        ta = psi + sign * a1
        tx, ty = centre[0] + radius * np.cos(ta), centre[1] + radius * np.sin(ta)
        sdf = np.minimum(sdf, np.hypot(c.X - tx, c.Y - ty) - 2.9 * 0.6)
    c.lit_body(sdf, tones, band=1.4, width=3.0)


def _vacuole(c: Canvas, x: float, y: float, r: float, tones) -> None:
    """A bubble in the cytoplasm: a lighter inside and a ring with a highlight."""
    d = np.hypot(c.X - x, c.Y - y)
    inside = d < r
    ring = inside & (d >= r - 1.1)
    c.paint(inside, tones[3])
    c.paint(ring, tones[0])
    c.paint(ring & ((c.X - x) * -0.6 + (c.Y - y) * -0.8 > 0.45 * r), tones[4])


def engulf_frame(pair: dict, d: int, i: int) -> Canvas:
    E, P = ANIMS[pair["engulfer"]], ANIMS[pair["prey"]]
    ang = d * np.pi / 4
    dv = np.array([np.cos(ang), np.sin(ang)])
    ec = np.array(E["centre"], float)
    c = Canvas(ENGULF_SIZE, ENGULF_SIZE, origin=(ENGULF_SIZE / 2 - ec[0], ENGULF_SIZE / 2 - ec[1]))
    tones = RAMPS[E["ramp"]]
    ra = P["radius"] + 2.5  # radius of the ring round the target
    d0 = E["radius"] + P["radius"] + 1  # distance when the two cells touch
    s = ENGULF_STRETCH[i]
    pose_e = Pose(t=i / FRAMES, heading=ang, stretch=s, lag=tuple(dv * s * 10))
    _paste_cell(c, E["make"](pose_e, anim=True), E["centre"], ec)
    if i < 7:
        pos = ec + dv * (d0 + ENGULF_DIST[i])
        # The target struggles and swims away from the engulfer.
        pose_p = Pose(t=i / FRAMES, heading=ang, stretch=0.08, scale=1.0 - 0.012 * i)
        _paste_cell(c, P["make"](pose_p, anim=True), P["centre"], pos)
        _arms(c, pos, ang + np.pi, ra, ENGULF_EXTENT[i], tones)
    else:
        q = _ease(VACUOLE_Q[i])
        start = ec + dv * (d0 + ENGULF_DIST[6])
        pos = start + (ec + dv * VACUOLE_DIST - start) * q
        r = ra + (VACUOLE_R - ra) * q
        _vacuole(c, pos[0], pos[1], r, tones)
        scale = (1.0 - 0.012 * 6) + (VACUOLE_SCALE - (1.0 - 0.012 * 6)) * q
        pose_p = Pose(t=i / FRAMES, scale=scale)
        _paste_cell(c, P["make"](pose_p, anim=True), P["centre"], pos)
    c.outline(tones[0])
    return c


def digest_frame(pair: dict, d: int, i: int, end: bool = False) -> Canvas:
    """The engulfer with a vacuole. `end` = the gene-gain sequence (10 frames)."""
    E, P = ANIMS[pair["engulfer"]], ANIMS[pair["prey"]]
    ang = d * np.pi / 4
    ec = np.array(E["centre"], float)
    base = E["make"](idle_pose(i % FRAMES), anim=True)
    c = Canvas(base.w, base.h, origin=(base.ox, base.oy))
    c.px[:] = base.px
    tones = RAMPS[E["ramp"]]
    ptones = RAMPS[P["ramp"]]
    ph = 2 * np.pi * i / FRAMES
    f = ec + np.array([np.cos(ang), np.sin(ang)]) * VACUOLE_DIST
    r, scale, flash, fade, glint = VACUOLE_R + 0.5 * np.sin(ph), VACUOLE_SCALE, 0, 0.0, None
    prey_on = True
    if end:
        r, scale = VACUOLE_R, VACUOLE_SCALE
        # frame: (vacuole r, prey scale, prey flash, prey fade, glint progress)
        seq = [
            (7.0, 0.50, 0, 0.0, None), (7.0, 0.50, 2, 0.0, None), (7.0, 0.50, 1, 0.0, None),
            (6.2, 0.40, 0, 0.3, None), (5.0, 0.25, 0, 0.6, None), (4.0, 0.0, 0, 1.0, 0.0),
            (3.0, 0.0, 0, 1.0, 0.3), (2.0, 0.0, 0, 1.0, 0.6), (0.0, 0.0, 0, 1.0, 0.9),
            (0.0, 0.0, 0, 1.0, 1.0),
        ]
        r, scale, flash, fade, glint = seq[i]
        prey_on = scale > 0
    if r > 0.5:
        _vacuole(c, f[0], f[1], r, tones)
    if prey_on:
        pf = P["make"](Pose(t=(i % FRAMES) / FRAMES, scale=scale), anim=True)
        fx_flash(pf, flash)
        _fade(pf, fade)
        _paste_cell(c, pf, P["centre"], f)
    if not end:  # specks drift in the vacuole
        for k in range(3):
            a = ph + k * 2.1
            c.plot(f[0] + (r - 2.2) * np.cos(a), f[1] + (r - 2.2) * np.sin(a), tones[4])
    n = np.array([NUCLEOID[0] + 0.9 * np.sin(ph), NUCLEOID[1] + 0.9 * np.cos(ph)])
    if glint is not None:
        # The gene is a bright speck. It flies from the vacuole to the nucleoid
        # on a curve, and leaves a short trail in the colours of the target.
        mid = (f + n) / 2 + np.array([-(n - f)[1], (n - f)[0]]) * 0.25
        for k, tone in enumerate((FLASH, ptones[4], ptones[3], ptones[2])):
            s = glint - 0.07 * k
            if 0 <= s <= 1:
                p = (1 - s) ** 2 * f + 2 * (1 - s) * s * mid + s**2 * n
                c.plot(p[0], p[1], tone)
                if k == 0:
                    c.plot(p[0] + 1, p[1], ptones[4])
                    c.plot(p[0], p[1] + 1, ptones[4])
    if end and i == 9:  # the nucleoid takes the gene
        flash_disc = np.hypot(c.X - n[0], c.Y - n[1]) < 5.5
        checker = (np.add.outer(np.arange(c.h), np.arange(c.w)) % 2) == 0
        c.paint(flash_disc & c.filled & checker, FLASH)
        fx_ring(c, n[0], n[1], 8.0, 0.9, RAMPS["nucleoid"][3])
    return c


# -- gate cell victory ---------------------------------------------------------

VICTORY_FRAMES = 16
VICTORY_SIZE = 96
RAMPS["gene"] = ramp("#5a3a08", "#d49a20", "#ffd24a", "#fff0a0", "#ffffff")  # the reward: gold-white
VICTORY_SWELL = [1.0, 1.03, 1.05, 1.07, 1.08, 1.10] + [1.0] * 10
VICTORY_SHAKE = [(0, 0), (1, 0), (-1, 1), (1, -1), (-1, 0), (0, 1)] + [(0, 0)] * 10
VICTORY_CRACK = [0.0, 0.0, 0.35, 0.6, 0.85, 1.0, 1.0] + [0.0] * 9
VICTORY_FADE = [0.0] * 7 + [0.3, 0.6, 0.85, 1.0] + [1.0] * 5
VICTORY_ORB = [0.0] * 8 + [1.5, 3.0, 4.2, 5.0, 5.4, 5.0, 5.4, 5.0]


def gate_victory_frames() -> list[Canvas]:
    """The gate cell gives up its gene.

    0-1  it strains: the body shakes and the halo flares.
    2-5  light leaks out of cracks.
    6-9  it breaks into shards. Specks of light stream out.
    8-15 the shards fade, and a gold orb (the gene) forms in the middle. A ring
         spreads out from it. `gene_orb` carries on from the last frame.
    """
    rng = np.random.RandomState(33)
    S = VICTORY_SIZE
    centre = S / 2
    g0 = gate_cell(REST)
    design_c = (36.0, 36.0)
    org = (S / 2 - design_c[0], S / 2 - design_c[1])
    # Shard layout from the first frame.
    src0 = np.zeros((S, S, 4), dtype=np.uint8)
    ys0, xs0 = np.nonzero(g0.px[..., 3] > 0)
    pick = rng.choice(len(xs0), 11, replace=False)
    seeds = np.stack([xs0[pick] + 0.5 + org[0], ys0[pick] + 0.5 + org[1]], axis=1)
    # Cracks: random walks from the centre.
    cracks = []
    for a in np.linspace(0, 2 * np.pi, 7, endpoint=False) + 0.3:
        pts, x, y = [], centre, centre
        for _ in range(int(25 * 0.95)):
            a += rng.uniform(-0.4, 0.4)
            x, y = x + np.cos(a), y + np.sin(a)
            pts.append((x, y))
        cracks.append(pts)
    rays = [(rng.uniform(0, 2 * np.pi), rng.uniform(0.8, 1.6), rng.randint(0, 4)) for _ in range(18)]
    ray_tones = [FLASH, RAMPS["vent"][3], RAMPS["gate"][3], RAMPS["pigment"][3]]
    bayer = np.tile(BAYER4, (S // 4 + 1, S // 4 + 1))[:S, :S]

    out = []
    for i in range(VICTORY_FRAMES):
        c = Canvas(S, S, origin=org)
        g = gate_cell(idle_pose(i % FRAMES))
        src = np.zeros((S, S, 4), dtype=np.uint8)
        x0, y0 = int(org[0]), int(org[1])
        src[y0 : y0 + g.h, x0 : x0 + g.w] = g.px
        sx, sy = VICTORY_SHAKE[i]
        src = np.roll(np.roll(src, sy, axis=0), sx, axis=1)
        src = _scale_px(src, centre, centre, VICTORY_SWELL[i])
        body = src[..., 3] > 0
        if 6 <= i <= 9:  # shards fly apart
            q = (i - 5) / 4
            ys, xs = np.nonzero(body)
            d2 = (xs[:, None] + 0.5 - seeds[None, :, 0]) ** 2 + (ys[:, None] + 0.5 - seeds[None, :, 1]) ** 2
            shard = d2.argmin(axis=1)
            moved = np.zeros_like(src)
            for k in range(len(seeds)):
                sel = shard == k
                v = np.array([xs[sel].mean() + 0.5 - centre, ys[sel].mean() + 0.5 - centre])
                v = v / (np.linalg.norm(v) + 1e-6)
                off = np.round(v * 11 * _ease(q)).astype(int)
                tx, ty = xs[sel] + off[0], ys[sel] + off[1]
                ok = (tx >= 0) & (tx < S) & (ty >= 0) & (ty < S)
                moved[ty[ok], tx[ok]] = src[ys[sel][ok], xs[sel][ok]]
            src = moved
        c.px[:] = src
        if VICTORY_FADE[i] > 0:
            _fade(c, VICTORY_FADE[i])
        # Halo flare at the start (the idle halo is already in the frame).
        if i < 2:
            c.halo(design_c[0], design_c[1], 27, 36 + 4 * i, [RAMPS["vent"][1], RAMPS["vent"][2], RAMPS["vent"][3]], 0.7 + 0.2 * i)
        if i == 1:
            fx_flash(c, 1)
        # Light leaks out of the cracks.
        if VICTORY_CRACK[i] > 0:
            for pts in cracks:
                for x, y in pts[: int(len(pts) * VICTORY_CRACK[i])]:
                    if body[int(y), int(x)]:
                        c.px[int(y), int(x)] = (*RAMPS["vent"][4], 255)
                        for ox, oy in ((1, 0), (0, 1)):
                            if body[int(y) + oy, int(x) + ox] and (int(x) + int(y)) % 2 == 0:
                                c.px[int(y) + oy, int(x) + ox] = (*RAMPS["vent"][3], 255)
        # Specks of light stream out.
        if 3 <= i <= 13:
            for a, sp, tone in rays:
                rad = 8 + sp * (i - 3) * 4.2
                if rad < 46:
                    x, y = int(centre + rad * np.cos(a)), int(centre + rad * np.sin(a))
                    c.px[y, x] = (*ray_tones[tone], 255)
                    if (i + tone) % 2 == 0:
                        c.px[y + 1, x] = (*ray_tones[tone], 255)
        # The gene orb and the ring.
        r = VICTORY_ORB[i]
        if r > 0:
            c.halo(36, 36, r + 1, r + 8, [RAMPS["gene"][1], RAMPS["gene"][2], RAMPS["gene"][3]], 0.8)
            c.disc(36, 36, r, RAMPS["gene"])
        if 9 <= i <= 15:
            rad = 6 + (i - 9) * 6
            strength = 1.0 - 0.1 * (i - 9)
            fx_ring(c, 36, 36, rad, strength, RAMPS["vent"][3])
            if i > 9:
                fx_ring(c, 36, 36, rad - 3, strength * 0.8, RAMPS["gate"][3])
        out.append(c)
    return out


def gene_orb(i: int = 0) -> Canvas:
    """The gene as an object: a gold orb with sparks that circle it. It loops.

    The gate cell victory ends with this orb. The game flies it to the player.
    """
    c = Canvas(24, 24)
    ph = 2 * np.pi * i / FRAMES
    c.disc(12, 12, 4.6 + 0.4 * np.sin(ph), RAMPS["gene"])
    for k in range(3):
        a = ph + k * 2 * np.pi / 3
        c.plot(12 + 8 * np.cos(a), 12 + 8 * np.sin(a), RAMPS["gene"][3])
        c.plot(12 + 8 * np.cos(a - 0.35), 12 + 8 * np.sin(a - 0.35), RAMPS["gene"][2])
    c.halo(12, 12, 6, 11, [RAMPS["gene"][1], RAMPS["gene"][2]], 0.6 + 0.2 * np.sin(ph))
    return c


# -- build, write, preview ---------------------------------------------------------


def build_engulf() -> dict:
    rows = range(len(DIRECTIONS))
    out = {}
    for name, pair in PAIRS.items():
        out[name] = {
            "engulf": [[engulf_frame(pair, d, i) for i in range(ENGULF_FRAMES)] for d in rows],
            "digest": [[digest_frame(pair, d, i) for i in range(FRAMES)] for d in rows],
            "digest_end": [[digest_frame(pair, d, i, end=True) for i in range(DIGEST_END_FRAMES)] for d in rows],
        }
    out["gate_cell_victory"] = {"victory": gate_victory_frames()}
    return out


ENGULF_KINDS = {
    "engulf": dict(fps=12, loop=False),
    "digest": dict(fps=6, loop=True),
    "digest_end": dict(fps=10, loop=False),
}


def write_engulf(frames: dict) -> None:
    path = ANIM_DIR / "manifest.json"
    manifest = json.loads(path.read_text())
    for name, kinds in frames.items():
        if name == "gate_cell_victory":
            write_png(ANIM_DIR / "gate_cell_victory.png", _grid(kinds["victory"]))
            manifest[name] = {
                "frame_size": [VICTORY_SIZE, VICTORY_SIZE],
                "anchor_px": [VICTORY_SIZE // 2, VICTORY_SIZE // 2],
                "victory": {"file": "gate_cell_victory.png", "columns": VICTORY_FRAMES, "rows": 1,
                            "fps": 10, "loop": False,
                            "note": "Centred on the gate cell. It ends on the gene orb: continue with gene_orb."},
            }
            continue
        pair = PAIRS[name]
        E = ANIMS[pair["engulfer"]]
        info = {"engulfer": pair["engulfer"], "target": pair["prey"]}
        for kind, grid in kinds.items():
            f0 = grid[0][0]
            write_png(ANIM_DIR / f"{name}_{kind}.png", _grid(grid))
            info[kind] = {
                "file": f"{name}_{kind}.png", "frame_size": [f0.w, f0.h],
                "anchor_px": [E["centre"][0] + f0.ox, E["centre"][1] + f0.oy],
                "columns": len(grid[0]), "rows": len(grid),
                "fps": ENGULF_KINDS[kind]["fps"], "loop": ENGULF_KINDS[kind]["loop"],
                "directions": DIRECTIONS,
                "direction_meaning": "direction from the engulfer to the target",
            }
        manifest[name] = info
    orb = [gene_orb(i) for i in range(FRAMES)]
    write_png(ANIM_DIR / "gene_orb.png", np.concatenate([f.px for f in orb], axis=1))
    manifest["gene_orb"] = {
        "frame_size": [24, 24], "anchor_px": [12, 12],
        "loop": {"file": "gene_orb.png", "columns": FRAMES, "rows": 1, "fps": 8, "loop": True},
    }
    path.write_text(json.dumps(manifest, indent=2) + "\n")


def _gif(path: Path, frames: list[np.ndarray], delay: int) -> None:
    if shutil.which("magick") is None:
        print("magick not found: skipped", path.name)
        return
    with tempfile.TemporaryDirectory() as tmp:
        for n, px in enumerate(frames):
            write_png(Path(tmp) / f"f{n:03d}.png", px)
        subprocess.run(
            ["magick", "-delay", str(delay), "-loop", "0", f"{tmp}/f*.png", "-layers", "optimize", str(path)],
            check=True,
        )


def write_engulf_previews(frames: dict) -> None:
    prev = PREVIEW_DIR / "anim"
    for name in PAIRS:
        write_png(prev / f"{name}.png", on_water(_grid(frames[name]["engulf"]), 1))
        write_png(prev / f"{name}_digest.png", on_water(_grid(frames[name]["digest_end"]), 2))
    # The whole story for each pair and two directions: engulf, digest, gene gain.
    S = ENGULF_SIZE
    panels = [(n, d) for n in PAIRS for d in (0, 5)]
    seq = []
    for step in range(ENGULF_FRAMES + 2 * FRAMES + DIGEST_END_FRAMES + 3):
        board = np.zeros((S, S * len(panels), 4), dtype=np.uint8)
        for col, (name, d) in enumerate(panels):
            k = frames[name]
            if step < ENGULF_FRAMES:
                f = k["engulf"][d][step]
            elif step < ENGULF_FRAMES + 2 * FRAMES:
                f = k["digest"][d][(step - ENGULF_FRAMES) % FRAMES]
            elif step < ENGULF_FRAMES + 2 * FRAMES + DIGEST_END_FRAMES:
                f = k["digest_end"][d][step - ENGULF_FRAMES - 2 * FRAMES]
            else:
                f = k["digest_end"][d][-1]
            E = ANIMS[PAIRS[name]["engulfer"]]
            ax, ay = E["centre"][0] + f.ox, E["centre"][1] + f.oy
            x0, y0 = int(round(col * S + S / 2 - ax)), int(round(S / 2 - ay))
            for oy in range(f.h):
                for ox in range(f.w):
                    if f.px[oy, ox, 3] and 0 <= y0 + oy < S and 0 <= x0 + ox < board.shape[1]:
                        board[y0 + oy, x0 + ox] = f.px[oy, ox]
        seq.append(on_water(board, 3, 1))
    _gif(prev / "engulf_story.gif", seq, 9)
    # Gate cell victory: a sheet in two rows, and a GIF that ends on the orb loop.
    v = frames["gate_cell_victory"]["victory"]
    half = VICTORY_FRAMES // 2
    rows = [np.concatenate([f.px for f in v[:half]], axis=1), np.concatenate([f.px for f in v[half:]], axis=1)]
    write_png(prev / "gate_victory.png", on_water(np.concatenate(rows, axis=0), 2, 1))
    seq = [on_water(f.px, 4, 1) for f in v]
    last = v[-1]
    for i in range(FRAMES):
        orb = gene_orb(i)
        c = Canvas(VICTORY_SIZE, VICTORY_SIZE)
        c.px[:] = last.px
        # The orb sprite replaces the orb in the last frame, at the centre.
        c.px[VICTORY_SIZE // 2 - 12 : VICTORY_SIZE // 2 + 12, VICTORY_SIZE // 2 - 12 : VICTORY_SIZE // 2 + 12] = orb.px
        seq.append(on_water(c.px, 4, 1))
    _gif(prev / "gate_victory.gif", seq, 9)
    orbs = np.concatenate([gene_orb(i).px for i in range(FRAMES)], axis=1)
    write_png(prev / "gene_orb.png", on_water(orbs, 4, 1))


# --------------------------------------------------------------------------
# Output
# --------------------------------------------------------------------------


def write_png(path: Path, px: np.ndarray) -> None:
    h, w, _ = px.shape
    raw = b"".join(b"\x00" + px[y].tobytes() for y in range(h))

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def upscale(px: np.ndarray, k: int) -> np.ndarray:
    return np.repeat(np.repeat(px, k, axis=0), k, axis=1)


# A 3x5 pixel font for the labels on the contact sheet.
FONT = {
    "a": "010101111101101", "b": "110101110101110", "c": "011100100100011",
    "d": "110101101101110", "e": "111100110100111", "f": "111100110100100",
    "g": "011100101101011", "h": "101101111101101", "i": "111010010010111",
    "j": "001001001101010", "k": "101101110101101", "l": "100100100100111",
    "m": "101111111101101", "n": "110101101101101", "o": "010101101101010",
    "p": "110101110100100", "q": "010101101111011", "r": "110101110101101",
    "s": "011100010001110", "t": "111010010010010", "u": "101101101101111",
    "v": "101101101101010", "w": "101101111111101", "x": "101101010101101",
    "y": "101101010010010", "z": "111001010100111", "_": "000000000000111",
    " ": "000000000000000", "-": "000000111000000",
    **{str(i): s for i, s in enumerate([
        "111101101101111", "010110010010111", "110001010100111", "110001010001110",
        "101101111001001", "111100110001110", "011100111101111", "111001010100100",
        "111101111101111", "111101111001110"])},
}


def draw_text(px: np.ndarray, x: int, y: int, text: str, color, k: int = 2) -> None:
    for ch in text.lower():
        glyph = FONT.get(ch, FONT[" "])
        for i, bit in enumerate(glyph):
            if bit == "1":
                gy, gx = divmod(i, 3)
                px[y + gy * k : y + (gy + 1) * k, x + gx * k : x + (gx + 1) * k] = (*color, 255)
        x += 4 * k


def contact_sheet(sprites: dict[str, Canvas]) -> np.ndarray:
    scale, pad, cols = 5, 14, 4
    cells = []
    for name, c in sprites.items():
        cells.append((name, upscale(c.px, scale)))
    cell_w = max(a.shape[1] for _, a in cells) + pad * 2
    cell_h = max(a.shape[0] for _, a in cells) + pad * 2 + 20
    rows = (len(cells) + cols - 1) // cols
    sheet = np.zeros((rows * cell_h, cols * cell_w, 4), dtype=np.uint8)
    sheet[..., :3] = WATER[2]
    sheet[..., 3] = 255
    for i, (name, arr) in enumerate(cells):
        r, col = divmod(i, cols)
        x0, y0 = col * cell_w, r * cell_h
        # Checker tile behind each sprite, so the transparent area is easy to see.
        sheet[y0 + 2 : y0 + cell_h - 2, x0 + 2 : x0 + cell_w - 2, :3] = WATER[1]
        h, w, _ = arr.shape
        ox = x0 + (cell_w - w) // 2
        oy = y0 + pad + (cell_h - 20 - pad * 2 - h) // 2
        region = sheet[oy : oy + h, ox : ox + w]
        a = arr[..., 3:4] > 0
        region[:] = np.where(a, arr, region)
        draw_text(sheet, x0 + pad, y0 + cell_h - 24, name, RAMPS["player"][3], 2)
    return sheet


def vent_scene(sprites: dict[str, Canvas]) -> np.ndarray:
    """A 320x180 top-down mock of the Run screen, drawn at native size."""
    W, H = 320, 180
    scene = Canvas(W, H)
    # Seabed: slow patches of silt, with dithered edges between the tones.
    bayer = np.tile(BAYER4, (H // 4 + 1, W // 4 + 1))[:H, :W]
    X, Y = scene.X, scene.Y
    n = (np.sin(X * 0.045 + Y * 0.031) + np.sin(X * 0.021 - Y * 0.052 + 2.0)
         + np.sin((X + Y) * 0.017 + 1.0) + np.sin(X * 0.09 - Y * 0.07 + 4.0) * 0.5)
    base = 1.6 + n * 0.35
    lvl = np.floor(base + bayer * 0.9).astype(int)
    lvl = np.clip(lvl, 0, len(WATER) - 1)
    for i, col in enumerate(WATER):
        scene.paint(lvl == i, col)
    rng = np.random.RandomState(3)
    # Pebbles and ripples on the seabed.
    for _ in range(70):
        x, y = rng.randint(0, W - 3), rng.randint(0, H - 2)
        w = rng.randint(1, 4)
        scene.px[y, x : x + w] = (*WATER[4], 255)
        if rng.rand() < 0.3:
            scene.px[y + 1, x : x + w] = (*WATER[0], 255)
    for _ in range(40):
        x, y = rng.randint(0, W), rng.randint(0, H)
        scene.spark([(x, y)], RAMPS["rock"][3])

    def put(name: str, cx: int, cy: int) -> None:
        s = sprites[name]
        scene.blit(s, cx - s.w // 2, cy - s.h // 2)

    # Vent at the right. Its heat glows on the seabed around it.
    vx, vy = 262, 112
    scene.halo(vx, vy, 40, 80, [RAMPS["vent"][0], RAMPS["vent"][1]], 0.3)
    put("vent_chimney_top", vx, vy)
    # Membrane wall with a pore, part of a tissue.
    for i in range(10):
        if i not in (4, 5):
            scene.blit(sprites["wall_membrane"], 8 + i * 16, 22)
    scene.blit(sprites["wall_membrane_pore"], 8 + 4 * 16, 22)
    # Gate cell, big, at the left.
    put("gate_cell_phototroph", 52, 120)
    # Life.
    put("player_cell", 150, 108)
    put("prey_bacillus", 205, 66)
    put("prey_bacillus", 222, 78)
    put("prey_bacillus", 196, 52)
    put("predator_cell", 110, 64)
    put("symbiont_cell", 142, 152)
    put("virus_phage", 196, 150)
    # Pickups.
    for x, y in [(180, 118), (187, 126), (171, 130), (112, 118), (121, 126)]:
        put("food_carbon", x, y)
    for x, y in [(212, 112), (221, 106), (206, 122)]:
        put("pickup_mineral", x, y)
    for x, y in [(236, 152), (244, 160)]:
        put("pickup_metal_fes", x, y)
    # Smoke goes over everything near the vent.
    put("vent_plume_top", vx, vy)
    return scene.px


def main() -> None:
    sprites = {name: make() for name, make in SPRITES.items()}
    for name, c in sprites.items():
        write_png(SPRITE_DIR / f"{name}.png", c.px)
        print(f"{name:24s} {c.w}x{c.h}")
    write_png(PREVIEW_DIR / "contact_sheet.png", contact_sheet(sprites))
    write_png(PREVIEW_DIR / "vent_scene.png", upscale(vent_scene(sprites), 4))
    frames = build_animations()
    write_animations(frames)
    write_anim_previews(frames)
    overlays = build_overlays()
    write_overlays(overlays)
    write_overlay_previews(overlays, frames["player_cell"]["idle"])
    engulf = build_engulf()
    write_engulf(engulf)
    write_engulf_previews(engulf)
    write_metabolism_anims()
    print("wrote", SPRITE_DIR, "and", PREVIEW_DIR)


if __name__ == "__main__":
    main()
