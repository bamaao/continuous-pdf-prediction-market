"""Icon: C framing  ∫  [PDF interval figure]  = 1

Uses STIXGeneral ∫ (U+222B). Continuous density + buy-interval plot on the right of ∫.
"""
from __future__ import annotations

import math
import shutil
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageOps

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "apps" / "web"
BRAND = WEB / "brand"
PUBLIC = WEB / "public"
APP = WEB / "src" / "app"
CONCEPTS = BRAND / "concepts"
FONTS = BRAND / "fonts"
INTEGRAL_ASSET = BRAND / "integral-sign.png"

BG = (18, 17, 14, 255)
GOLD = (212, 160, 23, 255)
CREAM = (243, 234, 212, 255)
GRID = (243, 234, 212, 12)

_STIX_SRC = Path(
    r"D:\Users\zouyc\anaconda3\lib\site-packages\matplotlib\mpl-data\fonts\ttf\STIXGeneral.ttf"
)
_STIX_LOCAL = FONTS / "STIXGeneral.ttf"


def density(t: float) -> float:
    mode = 0.28
    if t < 0.06:
        return 0.04 + t * 1.5
    if t <= mode:
        u = (t - 0.06) / (mode - 0.06)
        return 0.12 + 0.88 * (u**0.65)
    return max(0.05, 0.95 * math.exp(-2.8 * (t - mode)) + 0.06 * (1.0 - t))


def ensure_stix_font() -> Path:
    FONTS.mkdir(parents=True, exist_ok=True)
    if _STIX_LOCAL.is_file():
        return _STIX_LOCAL
    if _STIX_SRC.is_file():
        shutil.copy2(_STIX_SRC, _STIX_LOCAL)
        return _STIX_LOCAL
    raise SystemExit("STIXGeneral.ttf not found")


def rasterize_stix_integral(*, force: bool = False) -> Path:
    """Pretty STIX ∫ → brand/integral-sign.png"""
    if not force and INTEGRAL_ASSET.is_file() and INTEGRAL_ASSET.stat().st_size > 200:
        return INTEGRAL_ASSET
    font_path = ensure_stix_font()
    font = ImageFont.truetype(str(font_path), 720)
    canvas = Image.new("L", (560, 1200), 255)
    draw = ImageDraw.Draw(canvas)
    draw.text((40, 60), "\u222b", font=font, fill=0)
    inv = ImageOps.invert(canvas)
    bbox = inv.getbbox()
    if not bbox:
        raise SystemExit("failed to rasterize STIX ∫")
    gray = canvas.crop(bbox)
    arr = np.array(gray)
    alpha = (255 - arr).astype(np.uint8)
    rgba = np.zeros((arr.shape[0], arr.shape[1], 4), dtype=np.uint8)
    rgba[:, :, 0] = GOLD[0]
    rgba[:, :, 1] = GOLD[1]
    rgba[:, :, 2] = GOLD[2]
    rgba[:, :, 3] = alpha
    img = Image.fromarray(rgba, "RGBA")
    pad = 12
    out = Image.new("RGBA", (img.width + 2 * pad, img.height + 2 * pad), (0, 0, 0, 0))
    out.paste(img, (pad, pad), img)
    BRAND.mkdir(parents=True, exist_ok=True)
    out.save(INTEGRAL_ASSET, format="PNG", optimize=True)
    CONCEPTS.mkdir(parents=True, exist_ok=True)
    out.save(CONCEPTS / "integral-STIXGeneral.png", format="PNG", optimize=True)
    return INTEGRAL_ASSET


def draw_integral_sign(height: int) -> Image.Image:
    path = rasterize_stix_integral()
    im = Image.open(path).convert("RGBA")
    w = max(1, int(im.width * (height / im.height)))
    return im.resize((w, height), Image.Resampling.LANCZOS)


def draw_horizontal_pdf(width: int = 900, height: int = 420) -> Image.Image:
    im = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    pad = int(min(width, height) * 0.08)
    left, right = pad, width - pad
    base = int(height * 0.82)
    w = right - left
    h = int(height * 0.70)
    size = width
    samples = 120
    peak = max(density(s / samples) for s in range(samples + 1))
    pts = [
        (int(left + (s / samples) * w), int(base - h * density(s / samples) / peak))
        for s in range(samples + 1)
    ]
    d = ImageDraw.Draw(im, "RGBA")
    d.polygon([(left, base), *pts, (right, base)], fill=(212, 160, 23, 70))
    a_t, b_t = 0.34, 0.56
    i0, i1 = int(a_t * samples), int(b_t * samples)
    seg = pts[i0 : i1 + 1]
    d.polygon([(seg[0][0], base), *seg, (seg[-1][0], base)], fill=(243, 234, 212, 140))
    d.line(pts, fill=GOLD, width=max(4, size // 80))
    d.line(seg, fill=CREAM, width=max(5, size // 70))
    d.line([(left, base), (right, base)], fill=CREAM, width=max(2, size // 160))
    tick = max(3, size // 50)
    for x in (seg[0][0], seg[-1][0]):
        d.line([(x, base - tick), (x, base + tick // 2)], fill=CREAM, width=max(2, size // 120))
    return im


def draw_equals_one(height: int) -> Image.Image:
    """Cream '= 1' stacked / compact for icon."""
    # Prefer a clean serif-ish look from STIX if available
    font_path = ensure_stix_font()
    # Two lines look cramped; single line "=1" is clearer at small sizes.
    font = ImageFont.truetype(str(font_path), max(28, int(height * 0.42)))
    text = "= 1"
    # Measure
    tmp = Image.new("RGBA", (8, 8), (0, 0, 0, 0))
    td = ImageDraw.Draw(tmp)
    bbox = td.textbbox((0, 0), text, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    pad_x, pad_y = 4, 2
    im = Image.new("RGBA", (tw + 2 * pad_x, max(height, th + 2 * pad_y)), (0, 0, 0, 0))
    d = ImageDraw.Draw(im, "RGBA")
    y = (im.height - th) // 2 - bbox[1]
    d.text((pad_x - bbox[0], y), text, font=font, fill=CREAM)
    return im


def compose(size: int = 1024) -> Image.Image:
    im = Image.new("RGBA", (size, size), BG)
    d = ImageDraw.Draw(im, "RGBA")
    step = size // 16
    for i in range(1, 16):
        d.line([(i * step, 0), (i * step, size)], fill=GRID, width=1)
        d.line([(0, i * step), (size, i * step)], fill=GRID, width=1)

    cx = cy = size // 2
    r = int(size * 0.36)
    d.arc([cx - r, cy - r, cx + r, cy + r], start=38, end=322, fill=CREAM, width=max(18, size // 22))

    # Formula row: ∫  [rotated PDF]  = 1
    row_h = int(r * 0.92)
    integ = draw_integral_sign(row_h)

    plot = draw_horizontal_pdf(720, 360)
    plot = plot.rotate(90, expand=True, resample=Image.Resampling.BICUBIC)
    bbox = plot.getbbox()
    if bbox:
        plot = plot.crop(bbox)
    # PDF figure height matches integral; keep readable width
    plot_h = int(row_h * 0.88)
    scale = plot_h / plot.height
    plot = plot.resize((max(1, int(plot.width * scale)), plot_h), Image.Resampling.LANCZOS)

    eq = draw_equals_one(int(row_h * 0.55))

    gap1 = max(6, size // 90)
    gap2 = max(8, size // 80)
    group_w = integ.width + gap1 + plot.width + gap2 + eq.width
    # Center group in C, slight bias left so "= 1" stays inside opening
    gx = cx - group_w // 2 - int(r * 0.02)
    gy = cy - row_h // 2

    im.alpha_composite(integ, (gx, gy + (row_h - integ.height) // 2))
    px = gx + integ.width + gap1
    im.alpha_composite(plot, (px, gy + (row_h - plot.height) // 2))
    ex = px + plot.width + gap2
    im.alpha_composite(eq, (ex, gy + (row_h - eq.height) // 2))
    return im.convert("RGB")


def export(im: Image.Image) -> None:
    for p in (BRAND, PUBLIC, APP, CONCEPTS):
        p.mkdir(parents=True, exist_ok=True)
    concept = CONCEPTS / "F12-integral-pdf-eq1.png"
    im.save(concept, format="PNG", optimize=True)
    print(f"wrote {concept}")

    def sized(n: int) -> Image.Image:
        return im.resize((n, n), Image.Resampling.LANCZOS)

    for n, dest in [
        (512, PUBLIC / "icon-512.png"),
        (192, PUBLIC / "icon-192.png"),
        (180, PUBLIC / "icon-180.png"),
        (32, PUBLIC / "icon-32.png"),
        (512, APP / "icon.png"),
        (180, APP / "apple-icon.png"),
    ]:
        sized(n).save(dest, format="PNG", optimize=True)
    sizes = [16, 32, 48]
    layers = [sized(s).convert("RGBA") for s in sizes]
    layers[0].save(
        PUBLIC / "favicon.ico",
        format="ICO",
        sizes=[(s, s) for s in sizes],
        append_images=layers[1:],
    )
    print("exported")


if __name__ == "__main__":
    rasterize_stix_integral(force=True)
    export(compose(1024))
