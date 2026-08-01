# .dskin file-association icon generator: a simple parcel illustration in
# the main logo's palette (icon/Logo_rounded.png) — pale sky-gradient
# rounded-square canvas with a corner-facing 3/4-view cardboard box (in the
# spirit of icon/1.webp: top rhombus + two side faces, tape over the top
# draping down the front edge, barcode on the left face), plus a shipping
# label, gradient-shaded faces and a soft ground shadow.
#
# Rendered large and downscaled per size (illustration style, unlike the
# flat tray mark which is drawn per size).
#
# Requires Pillow (pip install pillow).
# Usage: python tools/make-dskin-icon.py
#   master → icon/dskin-icon.png, frames → .tmp-ico-src/*.png, then:
#   node tools/make-ico.cjs src-tauri/icons/dskin.ico .tmp-ico-src/*.png
import os

from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICO_SRC = os.path.join(ROOT, ".tmp-ico-src")
MASTER = os.path.join(ROOT, "icon", "dskin-icon.png")
SIZES = [16, 20, 24, 28, 32, 40, 48, 64, 128, 256]

C = 2048  # master canvas

SKY_STOPS = [  # vertical gradient, pale like the logo sky
    (0.00, (232, 246, 252)),
    (0.45, (206, 235, 249)),
    (0.75, (165, 214, 244)),
    (1.00, (134, 198, 239)),
]
SHADOW = (60, 110, 160)       # soft ground shadow under the box, cool blue
# box faces are vertical gradients (top → bottom), light from above
BOX_LEFT = ((243, 187, 114), (225, 164, 89))    # cardboard tan
BOX_TOP = ((252, 220, 160), (242, 200, 138))    # clearly lightest face
BOX_RIGHT = ((206, 148, 80), (186, 126, 60))    # darker
TAPE = ((255, 246, 222), (246, 226, 187))       # cream, like the logo scroll
TAPE_EDGE = (168, 120, 60)    # tape outline, drawn at low alpha
CREASE = (122, 76, 30)        # face-boundary shading, drawn at low alpha
LABEL_LINE = (140, 186, 224)

# box faces in the corner-facing 3/4 view (like icon/1.webp), normalized to
# the box layer, symmetric around (0.5, 0.5): top rhombus + left/right faces
BK = (0.50, 0.18)             # top rhombus back vertex
LF = (0.12, 0.295)            # top rhombus left vertex
RT = (0.88, 0.295)            # top rhombus right vertex
FR = (0.50, 0.41)             # top rhombus front vertex (edge facing viewer)
LB = (0.12, 0.705)            # left face bottom vertex
RB = (0.88, 0.705)            # right face bottom vertex
FB = (0.50, 0.82)             # front edge bottom vertex
TOP = [LF, BK, RT, FR]
LEFT = [LF, FR, FB, LB]
RIGHT = [FR, RT, RB, FB]
SLOPE = (FR[1] - LF[1]) / (FR[0] - LF[0])  # face-edge run per unit x
# packing tape: a strip over the top center that wraps the front edge and
# hangs down the right face with a V-notched end
TAPE_TOP = [(0.4625, 0.1913), BK, (0.5375, 0.1913),
            (0.5375, 0.3987), FR, (0.4625, 0.3987)]
TAPE_TAIL = [FR, (0.57, 0.389), (0.57, 0.572), (0.535, 0.538), (0.50, 0.572)]
# barcode on the left face: bars parallel to the front edge, width-varying
BARCODE = dict(x=0.155, top=0.50, h=0.09, gap=0.018,
               widths=(0.014, 0.022, 0.016, 0.026, 0.014))
# shipping label on the right face, a parallelogram following the face
LABEL = [(0.58, 0.505), (0.74, 0.4566), (0.74, 0.5866), (0.58, 0.635)]


def lerp(c1, c2, t):
    return tuple(round(a + (b - a) * t) for a, b in zip(c1, c2))


def sky(C):
    img = Image.new("RGB", (C, C))
    d = ImageDraw.Draw(img)
    for y in range(C):
        p = y / (C - 1)
        for i in range(len(SKY_STOPS) - 1):
            p0, c0 = SKY_STOPS[i]
            p1, c1 = SKY_STOPS[i + 1]
            if p0 <= p <= p1:
                d.line([(0, y), (C, y)], fill=lerp(c0, c1, (p - p0) / (p1 - p0)))
                break
    return img


def box_layer(C):
    layer = Image.new("RGBA", (C, C), (0, 0, 0, 0))
    # scale the symmetric box around the layer center so it sits centered
    # inside the badge with breathing room, like the logo bottle
    cx, cy, k = 0.50, 0.50, 0.90

    def T(p):
        return ((cx + (p[0] - cx) * k) * C, (cy + (p[1] - cy) * k) * C)

    def shade(poly, colors):
        """Fill a face with a top→bottom gradient (light from above)."""
        pts = [T(p) for p in poly]
        mask = Image.new("L", (C, C), 0)
        ImageDraw.Draw(mask).polygon(pts, fill=255)
        y0, y1 = int(min(p[1] for p in pts)), int(max(p[1] for p in pts))
        ramp = Image.linear_gradient("L").resize((C, max(1, y1 - y0)),
                                                 Image.BICUBIC)
        face = Image.composite(Image.new("RGB", ramp.size, colors[1]),
                               Image.new("RGB", ramp.size, colors[0]), ramp)
        layer.paste(face, (0, y0), mask.crop((0, y0, C, y0 + ramp.height)))

    shade(TOP, BOX_TOP)
    shade(LEFT, BOX_LEFT)
    shade(RIGHT, BOX_RIGHT)
    d = ImageDraw.Draw(layer)
    # soft crease lines along face boundaries give the box structure;
    # drawn before the tape so the tape crosses them cleanly
    for a, b, alpha in ((LF, FR, 46),      # top/left
                        (FR, RT, 46),      # top/right
                        (FR, FB, 85),      # front vertical edge
                        (LB, FB, 34),      # bottom edges
                        (FB, RB, 34)):
        d.line([T(a), T(b)], fill=CREASE + (alpha,), width=int(C * 0.004))
    # packing tape over the top, tail draping down the front edge; thin
    # edge lines keep the cream strip defined against the top face
    shade(TAPE_TOP, TAPE)
    shade(TAPE_TAIL, TAPE)
    for a, b in ((TAPE_TOP[0], TAPE_TOP[5]), (TAPE_TOP[2], TAPE_TOP[3]),
                 (FR, (0.50, 0.572)), ((0.57, 0.389), (0.57, 0.572))):
        d.line([T(a), T(b)], fill=TAPE_EDGE + (70,), width=int(C * 0.003))
    # barcode bars on the left face, tops/bottoms following the face edges
    x = BARCODE["x"]
    for w in BARCODE["widths"]:
        y0 = BARCODE["top"] + (x - BARCODE["x"]) * SLOPE
        y1 = y0 + BARCODE["h"] + w * SLOPE
        d.polygon([T((x, y0)), T((x + w, y0 + w * SLOPE)),
                   T((x + w, y1)), T((x, y1 - w * SLOPE))],
                  fill=CREASE + (170,))
        x += w + BARCODE["gap"]
    # shipping label on the right face (soft drop shadow) + address lines
    (ax, ay), (bx, by), (cx2, cy2), (dx, dy) = LABEL
    off = (C * 0.004, C * 0.006)
    d.polygon([(T(p)[0] + off[0], T(p)[1] + off[1]) for p in LABEL],
              fill=CREASE + (36,))
    d.polygon([T(p) for p in LABEL], fill=(255, 255, 255, 255))
    for i, frac in enumerate((0.30, 0.50, 0.70)):  # down the label
        t1 = (0.66, 0.54, 0.40)[i]                 # line length along AB
        sx = ax + (bx - ax) * 0.10 + (dx - ax) * frac
        sy = ay + (by - ay) * 0.10 + (dy - ay) * frac
        ex = ax + (bx - ax) * t1 + (dx - ax) * frac
        ey = ay + (by - ay) * t1 + (dy - ay) * frac
        d.line([T((sx, sy)), T((ex, ey))], fill=LABEL_LINE,
               width=int(C * 0.007))
    # soft white highlight along the top-left silhouette, like the logo's
    # glass; faint dark rim on the far/right silhouette
    d.line([T(LF), T(BK)], fill=(255, 255, 255, 110), width=int(C * 0.006))
    d.line([T(LF), T(LB)], fill=(255, 255, 255, 70), width=int(C * 0.005))
    d.line([T(BK), T(RT)], fill=CREASE + (36,), width=int(C * 0.005))
    d.line([T(RT), T(RB)], fill=CREASE + (36,), width=int(C * 0.005))
    return layer


def master():
    img = sky(C).convert("RGBA")
    # soft ground shadow beneath the box
    sh = Image.new("RGBA", (C, C), (0, 0, 0, 0))
    ImageDraw.Draw(sh).ellipse([C * 0.24, C * 0.745, C * 0.76, C * 0.85],
                               fill=SHADOW + (44,))
    img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(C * 0.014)))
    img.alpha_composite(box_layer(C))
    # clip to the rounded-square badge
    badge = Image.new("L", (C, C), 0)
    ImageDraw.Draw(badge).rounded_rectangle([0, 0, C - 1, C - 1],
                                            radius=int(C * 0.225), fill=255)
    img.putalpha(badge)
    return img


if __name__ == "__main__":
    m = master()
    m.save(MASTER)
    print(f"master: {MASTER}")
    os.makedirs(ICO_SRC, exist_ok=True)
    for s in SIZES:
        m.resize((s, s), Image.LANCZOS).save(os.path.join(ICO_SRC, f"{s}.png"))
    print(f"frames: {ICO_SRC} ({len(SIZES)})")
