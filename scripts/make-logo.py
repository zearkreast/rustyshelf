#!/usr/bin/env python3
"""RustyShelf's logo: the icon (an "RS" monogram drawn as one ribbon on an oxide-red tile) and the RustyShelf wordmark,
as standalone SVG files. The R's leg runs on into the S's bottom stroke, and a stencil gap keeps the R's bowl apart from
the S.

The wordmark's letters are outlines (Chakra Petch Bold, one of the app's bundled faces, SIL OFL), so it looks the same
wherever it's shown, fonts or not.   python3 -I scripts/make-logo.py renderer/fonts/chakra-petch-latin-700-normal.woff2 renderer/brand
Then the PNGs (app icon, Android launcher and splash images) are rendered from these SVGs (see scripts/make-icons.sh).
"""

import math
import os
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

FONT, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)

OXIDE = "#a23b1e"  # keep in sync with scripts/make-icons.sh and android-app/.../values/ic_launcher_background.xml
PAPER = "#fff4ea"
OXIDE_LIGHT = "#ec7044"
UMBER = "#2a1410"


def ribbon(s, H, r, xRb, gap, sw, leg0, foot, fil):
    """the monogram's centerlines (stroked at width s) on a box whose top-left is 0,0; returns paths, width, height"""
    h = s / 2
    yT, yM, yB = h, H / 2, H - h
    xSl = xRb + s + gap
    xSr = xSl + sw
    W = xSr + h
    r_ = f"{r:g} {r:g} 0 0"  # r must stay <= (xRb - s/2)/2 and sw/2, or the arcs overlap
    R = (f"M{h:g},{H:g} V{yT + r:g} A{r_} 1 {h + r:g},{yT:g} H{xRb - r:g} A{r_} 1 {xRb:g},{yT + r:g} "
         f"V{yM - r:g} A{r_} 1 {xRb - r:g},{yM:g} H{h:g}")
    dx, dy = foot - leg0, yB - yM
    L = math.hypot(dx, dy)
    ax, ay = foot - fil * dx / L, yB - fil * dy / L
    S = (f"M{leg0:g},{yM:g} L{ax:.2f},{ay:.2f} Q{foot:g},{yB:g} {foot + fil:g},{yB:g} H{xSr - r:g} A{r_} 0 {xSr:g},{yB - r:g} "
         f"V{yM + r:g} A{r_} 0 {xSr - r:g},{yM:g} H{xSl + r:g} A{r_} 1 {xSl:g},{yM - r:g} V{yT + r:g} A{r_} 1 {xSl + r:g},{yT:g} H{W:g}")
    return [R, S], W, H


FULL = dict(s=54, H=316, r=50, xRb=132, gap=18, sw=106, leg0=86, foot=172, fil=44)
SMALL = dict(s=74, H=360, r=56, xRb=156, gap=30, sw=118, leg0=104, foot=196, fil=54)


def mark(ink, p, box=512):
    paths, W, H = ribbon(**p)
    tx, ty = (box - W) / 2, (box - H) / 2
    body = "".join(f'<path d="{d}"/>' for d in paths)
    return (f'<g transform="translate({tx:.1f} {ty:.1f})" fill="none" stroke="{ink}" stroke-width="{p["s"]}" '
            f'stroke-linejoin="round">{body}</g>')


def tile(p=FULL):
    return f'<rect width="512" height="512" rx="112" fill="{OXIDE}"/>\n{mark(PAPER, p)}'


def svg(w, h, body, label="RustyShelf"):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" '
            f'aria-label="{label}">\n{body}\n</svg>\n')


font = TTFont(FONT)
gs, cmap, hmtx, upm = font.getGlyphSet(), font.getBestCmap(), font["hmtx"], font["head"].unitsPerEm
CAP = font["OS/2"].sCapHeight / upm


def word_paths(text, size, x0, base, track=0.0):
    out, x, k = [], x0, size / upm
    for ch in text:
        g = cmap[ord(ch)]
        pen = SVGPathPen(gs)
        gs[g].draw(TransformPen(pen, (k, 0, 0, -k, x, base)))
        out.append(pen.getCommands())
        x += hmtx[g][0] * k + size * track
    return out, x - size * track - x0


def wordmark(size, x0, base, ink, accent):
    """"Rusty" in the accent, "Shelf" in the theme's ink"""
    letters, width = word_paths("RustyShelf", size, x0, base, track=0.01)
    return "".join(f'<path d="{d}" fill="{accent if i < 5 else ink}"/>' for i, d in enumerate(letters)), width


files = {
    "rustyshelf-icon.svg": svg(512, 512, tile()),
    "rustyshelf-icon-small.svg": svg(512, 512, tile(SMALL)),
    # Android's adaptive icon: the monogram alone (the launcher draws the background and its own mask), inside the safe zone
    "rustyshelf-icon-foreground.svg": svg(512, 512, f'<g transform="translate(256 256) scale(.62) translate(-256 -256)">{mark(PAPER, FULL)}</g>'),
}
ICON, SIZE, X0 = 160, 84, 194
for theme, ink, accent in (("light", UMBER, OXIDE), ("dark", PAPER, OXIDE_LIGHT)):
    body, width = wordmark(96, 0, 96, ink, accent)
    files[f"rustyshelf-wordmark-{theme}.svg"] = svg(int(width) + 4, 124, body)
    icon = f'<svg x="0" y="0" width="{ICON}" height="{ICON}" viewBox="0 0 512 512">{tile()}</svg>'
    base = ICON / 2 + SIZE * CAP / 2
    body, width = wordmark(SIZE, X0, base, ink, accent)
    files[f"rustyshelf-logo-{theme}.svg"] = svg(int(X0 + width) + 8, ICON, icon + body)
for name, text in files.items():
    open(os.path.join(OUT, name), "w").write(text)
    print(name, len(text))
