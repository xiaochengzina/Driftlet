# Tray icon generator: renders the upright drift-bottle artwork at every
# notification-area size (16/20/24/28/32/40/48 px for 100-300% DPI scales)
# straight into src-tauri/icons/.
#
# Every size is drawn from grid-snapped parametric geometry and supersampled
# 32x (never downscaled from a large master) so the small sizes stay
# legible. src-tauri/src/tray.rs picks the exact size at runtime via
# GetSystemMetrics(SM_CXSMICON).
#
# Design: logo-form badge (L) — rounded-square #2da0db tile + white
# silhouette of the ORIGINAL logo bottle (exact shape and tilt, extracted
# from icon/托盘图标/托盘图标_1024.png), inner elements removed. The tile
# carries its own background, so it reads on both light and dark taskbars.
# Other variants remain available as CLI args: L2 (parametric logo-form),
# A (outlined), B1 (two-color), P1/P2/P3 (mono silhouettes), C1/C2/C3
# (badge, upright parametric bottle).
# Replaces the old diagonal master-downscale pipeline
# (tools/make-tray-pngs.ps1), which was unrecognizable at tray sizes.
#
# Requires Pillow (pip install pillow).
# Usage: python tools/make-tray-icon.py [variant]   (default L)
import math
import os
from PIL import Image, ImageDraw

SS = 32  # supersample factor
SIZES = [16, 20, 24, 28, 32, 40, 48]

NAVY = (31, 78, 140, 255)
BLUE = (190, 227, 248, 255)
ORANGE = (232, 154, 85, 255)
CREAM = (253, 243, 216, 255)
WHITE = (255, 255, 255, 255)
DEEPSEA = (14, 117, 195, 255)   # #0e75c3 UI wave-deep (src/css/style.css)
WAVE = (45, 160, 219, 255)      # #2da0db UI wave-light

OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                       "..", "src-tauri", "icons")


def smoothstep(t):
    return 3 * t * t - 2 * t * t * t


def bottle_points(s):
    """Bottle outline as a closed point list, in target-pixel units."""
    body_l, body_r = s * 0.27, s * 0.73
    neck_w = s * 0.24
    neck_l, neck_r = (s - neck_w) / 2, (s + neck_w) / 2
    neck_top = s * 0.13
    sh_top = s * 0.36          # shoulder curve starts
    body_top = s * 0.50        # shoulder ends, body begins
    bottom = s * 0.94
    r = s * 0.17               # bottom corner radius (logo has a round bottom)

    pts = [(neck_l, neck_top), (neck_l, sh_top)]
    # left shoulder: S-curve, vertical tangent at both ends
    for i in range(1, 9):
        t = i / 8
        pts.append((neck_l + (body_l - neck_l) * smoothstep(t),
                    sh_top + (body_top - sh_top) * t))
    pts.append((body_l, bottom - r))
    # bottom-left quarter arc (screen coords: y grows downward, so the
    # bottom corners sweep through POSITIVE sin)
    for i in range(1, 5):
        a = math.pi - (math.pi / 2) * (i / 4)
        pts.append((body_l + r + r * math.cos(a), bottom - r + r * math.sin(a)))
    pts.append((body_r - r, bottom))
    # bottom-right quarter arc
    for i in range(1, 5):
        a = (math.pi / 2) * (1 - i / 4)
        pts.append((body_r - r + r * math.cos(a), bottom - r + r * math.sin(a)))
    pts.append((body_r, body_top))
    # right shoulder, mirrored
    for i in range(1, 9):
        t = i / 8
        pts.append((body_r + (neck_r - body_r) * smoothstep(t),
                    body_top + (sh_top - body_top) * t))
    pts.append((neck_r, neck_top))
    return pts


def cork_rect(s):
    # Slightly wider than the neck so it still reads as a cork, but short
    # and seated low — a mushroom-cap cork reads as "protruding" at tray
    # sizes.
    cork_w = s * 0.31
    return [(s - cork_w) / 2, s * 0.045, (s + cork_w) / 2, s * 0.045 + s * 0.12]


def scroll_rect(s):
    body_l, body_r = s * 0.27, s * 0.73
    body_top, bottom = s * 0.50, s * 0.94
    w = (body_r - body_l) * 0.30
    cx = (body_l + body_r) / 2
    return [cx - w / 2, body_top + (bottom - body_top) * 0.16,
            cx + w / 2, body_top + (bottom - body_top) * 0.84]


def render(size, variant="A"):
    if variant == "P1":
        return render_mono(size, WHITE)
    if variant == "P2":
        return render_mono(size, BLUE)
    if variant == "P3":
        return render_mono(size, WHITE, cork_gap=True)
    if variant in ("C1", "C2", "C3"):
        tile = {"C1": DEEPSEA, "C2": WAVE, "C3": NAVY}[variant]
        return render_badge(size, tile)
    if variant == "L":
        return render_logo_badge(size, WAVE)
    if variant == "L2":
        return render_logo_badge(size, WAVE, parametric=True)
    if variant != "A":
        body = BLUE if variant in ("B1", "B2") else WHITE
        return render_silhouette(size, body, with_scroll=(variant != "B2"))

    S = size * SS
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pts = [(x * SS, y * SS) for x, y in bottle_points(size)]
    cork = [v * SS for v in cork_rect(size)]
    cork_rad = (cork[3] - cork[1]) * 0.35

    w = max(1.5, size * 0.095) * SS
    d.polygon(pts, fill=BLUE)
    # thick closed outline: overshoot a few points to close the joint
    d.line(pts + pts[:4], fill=NAVY, width=int(w), joint="curve")
    # cork & scroll: fill only, no outline — Pillow draws outlines inside
    # the shape, and at these tiny feature sizes a navy outline swallows
    # the interior whole, rendering as a dark bar that reads as a crack.
    d.rounded_rectangle(cork, radius=cork_rad, fill=ORANGE)
    sc = [v * SS for v in scroll_rect(size)]
    d.rounded_rectangle(sc, radius=(sc[2] - sc[0]) * 0.3, fill=CREAM)
    return img.resize((size, size), Image.LANCZOS)


def render_silhouette(size, body_color, with_scroll):
    """Flat silhouette bottle + orange cork; scroll is knocked out
    (transparent) so the taskbar shows through the gap — no stroke, no
    inner color, nothing to turn muddy at small sizes."""
    S = size * SS
    pts = [(x * SS, y * SS) for x, y in bottle_points(size)]
    cork = [v * SS for v in cork_rect(size)]

    mask = Image.new("L", (S, S), 0)
    md = ImageDraw.Draw(mask)
    md.polygon(pts, fill=255)
    if with_scroll:
        sc = [v * SS for v in scroll_rect(size)]
        md.rounded_rectangle(sc, radius=(sc[2] - sc[0]) * 0.3, fill=0)

    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    img.paste(Image.new("RGBA", (S, S), body_color), (0, 0), mask)
    d = ImageDraw.Draw(img)
    d.rounded_rectangle(cork, radius=(cork[3] - cork[1]) * 0.35, fill=ORANGE)
    return img.resize((size, size), Image.LANCZOS)


_LOGO_MASK = None


def logo_bottle_mask():
    """Outer silhouette of the original logo bottle — exact shape and tilt —
    from the alpha channel of icon/托盘图标/托盘图标_1024.png. Binarized,
    interior holes filled (glass highlights), cropped to the bounding box.
    Inner artwork (scroll/bow) vanishes into the solid silhouette."""
    global _LOGO_MASK
    if _LOGO_MASK is None:
        from PIL import ImageChops
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        path = os.path.join(root, "icon", "托盘图标", "托盘图标_1024.png")
        a = Image.open(path).convert("RGBA").split()[3]
        a = a.point(lambda v: 255 if v > 64 else 0)
        inv = a.point(lambda v: 255 - v)
        ImageDraw.floodfill(inv, (0, 0), 128)   # background → 128
        holes = inv.point(lambda v: 255 if v == 255 else 0)
        m = ImageChops.lighter(a, holes)
        _LOGO_MASK = m.crop(m.getbbox())
    return _LOGO_MASK


def logo_form_mask(px):
    """Parametric bottle silhouette drawn in the logo's form language
    (exaggerated slim neck / cap / shoulder, ~12° tilt) — fallback variant
    L2 kept from the legibility experiment."""
    m = mono_mask(px, with_scroll=False)
    m = m.rotate(12, resample=Image.BICUBIC, expand=True)
    return m.crop(m.getbbox())


def render_logo_badge(size, tile_color, parametric=False):
    """Logo-form badge: rounded-square tile + white silhouette of the
    original (tilted) logo bottle, no inner elements."""
    S = size * SS
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.225),
                        fill=tile_color)
    ratio = 0.74 if size <= 20 else (0.70 if size <= 32 else 0.66)
    inner = int(round(S * ratio))
    mark = logo_form_mask(512) if parametric else logo_bottle_mask()
    scale = inner / max(mark.width, mark.height)
    nw, nh = round(mark.width * scale), round(mark.height * scale)
    mark = mark.resize((nw, nh), Image.LANCZOS)
    img.paste(Image.new("RGBA", (nw, nh), WHITE),
              ((S - nw) // 2, (S - nh) // 2), mark)
    return img.resize((size, size), Image.LANCZOS)


def mono_mask(px, cork_gap=False, with_scroll=True):
    """Bottle+cork silhouette alpha mask at raw pixel resolution `px`."""
    mask = Image.new("L", (px, px), 0)
    md = ImageDraw.Draw(mask)
    md.polygon(bottle_points(px), fill=255)
    cork = cork_rect(px)
    md.rounded_rectangle(cork, radius=(cork[3] - cork[1]) * 0.35, fill=255)
    if cork_gap:
        md.rectangle([0, px * 0.10, px, px * 0.155], fill=0)
    if with_scroll:
        sc = scroll_rect(px)
        md.rounded_rectangle(sc, radius=(sc[2] - sc[0]) * 0.3, fill=0)
    return mask


def render_mono(size, color, cork_gap=False):
    """Single-color silhouette: bottle and cork share one ink — depth comes
    from transparent knockouts only (the scroll, and optionally a hairline
    gap between cork and neck)."""
    S = size * SS
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    img.paste(Image.new("RGBA", (S, S), color), (0, 0), mono_mask(S, cork_gap))
    return img.resize((size, size), Image.LANCZOS)


def render_badge(size, tile_color):
    """WeChat-style badge: rounded-square brand tile + white bottle
    silhouette (scroll knocked out to the tile color). The tile carries its
    own background, so it reads on both light and dark taskbars."""
    S = size * SS
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, S - 1, S - 1], radius=int(S * 0.225),
                        fill=tile_color)
    # Fit the bottle's bounding box into the inner square so the mark fills
    # the badge like WeChat's glyph instead of floating inside margins;
    # smaller tiles get a proportionally larger mark to stay legible.
    ratio = 0.80 if size <= 20 else (0.76 if size <= 32 else 0.72)
    inner = int(round(S * ratio))
    mark = mono_mask(512).crop(mono_mask(512).getbbox())
    scale = inner / max(mark.width, mark.height)
    nw, nh = round(mark.width * scale), round(mark.height * scale)
    mark = mark.resize((nw, nh), Image.LANCZOS)
    img.paste(Image.new("RGBA", (nw, nh), WHITE),
              ((S - nw) // 2, (S - nh) // 2), mark)
    return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
    import sys
    variant = sys.argv[1] if len(sys.argv) > 1 else "L"
    for s in SIZES:
        out = os.path.join(OUT_DIR, f"tray-{s}.png")
        render(s, variant).save(out)
        print(f"tray-{s}.png")
