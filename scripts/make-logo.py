#!/usr/bin/env python3
"""RustyShelf's logo: the icon (an R built from a game case, a disc and a leaning case on a shelf) and the RustyShelf
wordmark, as standalone SVG files.

The wordmark's letters are outlines (Unbounded Bold, one of the app's bundled faces, SIL OFL), so it looks the same
wherever it's shown, fonts or not.   python3 -I scripts/make-logo.py renderer/fonts/unbounded-latin-700-normal.woff2 renderer/brand
Then the PNGs (app icon, Android launcher and splash images) are rendered from these SVGs (see scripts/make-icons.sh).
"""
import os
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

FONT, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)

TEAL = "#173a3c"  # keep in sync with scripts/make-icons.sh and android-app/.../values/ic_launcher_background.xml
TEAL_DEEP = "#0f2a2c"
CREAM, CREAM_SHADE = "#f2e8d3", "#d9cbb0"
COPPER, COPPER_LIGHT, COPPER_DARK = "#c8622c", "#e4874a", "#8f3c18"
AMBER = "#e9b04a"


def mark(simple=False):
    """the R on a 512 grid, without its background: the stem is a case on its spine, the bowl a disc, the leg a leaning case"""
    p = ['<defs><linearGradient id="cu" x1="0" x2="1" y1="0" y2="1">'
         f'<stop offset="0" stop-color="{COPPER_LIGHT}"/><stop offset="1" stop-color="{COPPER_DARK}"/></linearGradient></defs>']
    # the disc sits behind the stem, so the stem's edge cuts the bowl off cleanly
    p.append('<circle cx="250" cy="182" r="96" fill="url(#cu)"/>')
    if not simple:
        p.append(f'<circle cx="250" cy="182" r="70" fill="none" stroke="{COPPER_LIGHT}" stroke-opacity=".45" stroke-width="3"/>')
    p.append(f'<circle cx="250" cy="182" r="{24 if simple else 26}" fill="{TEAL}"/>')
    if not simple:
        p.append(f'<circle cx="250" cy="182" r="36" fill="none" stroke="{CREAM}" stroke-opacity=".35" stroke-width="4"/>')
    # the leg: a case leaning on the disc, its foot on the shelf
    p.append(f'<g transform="rotate(-27 352 404)"><rect x="306" y="190" width="78" height="214" rx="10" fill="{AMBER}"/>'
             + ('' if simple else f'<rect x="318" y="214" width="54" height="16" rx="4" fill="{COPPER_DARK}" fill-opacity=".55"/>'
                f'<rect x="318" y="372" width="54" height="10" rx="3" fill="{COPPER_DARK}" fill-opacity=".35"/>')
             + '</g>')
    # the stem: a case standing on its spine
    p.append(f'<rect x="120" y="86" width="96" height="318" rx="12" fill="{CREAM}"/>')
    p.append(f'<rect x="196" y="86" width="20" height="318" fill="{CREAM_SHADE}"/>')
    if not simple:
        p += [f'<rect x="136" y="112" width="64" height="52" rx="6" fill="{COPPER}"/>',
              f'<rect x="136" y="186" width="44" height="8" rx="4" fill="{TEAL}" fill-opacity=".35"/>',
              f'<rect x="136" y="204" width="56" height="8" rx="4" fill="{TEAL}" fill-opacity=".35"/>',
              f'<rect x="136" y="360" width="64" height="18" rx="5" fill="{TEAL}" fill-opacity=".8"/>']
    # the shelf the R stands on
    p.append(f'<rect x="76" y="404" width="360" height="30" rx="9" fill="{COPPER}"/>')
    p.append(f'<rect x="76" y="424" width="360" height="10" rx="5" fill="{COPPER_DARK}"/>')
    return "\n".join(p)


def tile(simple=False):
    return f'<rect width="512" height="512" rx="112" fill="{TEAL}"/>\n{mark(simple)}'


def svg(w, h, body, label):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="{label}">\n{body}\n</svg>\n'


# ---------- the wordmark's letters as outlines ----------
font = TTFont(FONT)
gs, cmap, hmtx, upm = font.getGlyphSet(), font.getBestCmap(), font["hmtx"], font["head"].unitsPerEm


def word_paths(text, size, x0, base, track=-0.01):
    """[svg path d per letter], and the word's width, with the letters set left to right from x0 on the baseline"""
    out, x, s = [], x0, size / upm
    for ch in text:
        g = cmap[ord(ch)]
        pen = SVGPathPen(gs)
        gs[g].draw(TransformPen(pen, (s, 0, 0, -s, x, base)))
        out.append(pen.getCommands())
        x += hmtx[g][0] * s + size * track
    return out, x - size * track - x0


def wordmark(size, x0, base, ink, accent):
    """"Rusty" in copper, "Shelf" in the theme's ink"""
    letters, width = word_paths("RustyShelf", size, x0, base)
    body = "".join(f'<path d="{d}" fill="{accent if i < 5 else ink}"/>' for i, d in enumerate(letters))
    return body, width


files = {}
files["rustyshelf-icon.svg"] = svg(512, 512, tile(), "RustyShelf")
files["rustyshelf-icon-small.svg"] = svg(512, 512, tile(simple=True), "RustyShelf")
# Android's adaptive icon: the mark alone (the launcher draws the background and its own mask), inside the safe zone
files["rustyshelf-icon-foreground.svg"] = svg(512, 512, f'<g transform="translate(256 256) scale(.62) translate(-256 -256)">{mark()}</g>', "RustyShelf")
for theme, ink, accent in (("dark", "#f2e8d3", "#e4874a"), ("light", "#173a3c", "#b5541f")):
    body, width = wordmark(88, 0, 92, ink, accent)
    files[f"rustyshelf-wordmark-{theme}.svg"] = svg(int(width) + 4, 124, body, "RustyShelf")
    icon = f'<svg x="0" y="0" width="160" height="160" viewBox="0 0 512 512">{tile()}</svg>'
    body2, width2 = wordmark(70, 188, 106, ink, accent)
    files[f"rustyshelf-logo-{theme}.svg"] = svg(int(188 + width2) + 6, 160, icon + body2, "RustyShelf")
for name, text in files.items():
    open(os.path.join(OUT, name), "w").write(text)
    print(name, len(text))
