"""Export PWA / Next.js icons from brand/concepts/F12-integral-pdf-eq1.png."""
from __future__ import annotations

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "apps" / "web"
MASTER = WEB / "brand" / "concepts" / "F12-integral-pdf-eq1.png"
PUBLIC = WEB / "public"
APP = WEB / "src" / "app"


def main() -> None:
    if not MASTER.is_file():
        raise SystemExit(f"missing master icon: {MASTER}")
    PUBLIC.mkdir(parents=True, exist_ok=True)
    APP.mkdir(parents=True, exist_ok=True)
    im = Image.open(MASTER).convert("RGB")

    def sized(n: int) -> Image.Image:
        return im.resize((n, n), Image.Resampling.LANCZOS)

    for n, path in [
        (512, PUBLIC / "icon-512.png"),
        (192, PUBLIC / "icon-192.png"),
        (180, PUBLIC / "icon-180.png"),
        (32, PUBLIC / "icon-32.png"),
        (512, APP / "icon.png"),
        (180, APP / "apple-icon.png"),
    ]:
        sized(n).save(path, format="PNG", optimize=True)
        print(f"wrote {path} ({n}x{n})")

    sizes = [16, 32, 48]
    layers = [sized(s).convert("RGBA") for s in sizes]
    ico = PUBLIC / "favicon.ico"
    layers[0].save(ico, format="ICO", sizes=[(s, s) for s in sizes], append_images=layers[1:])
    print(f"wrote {ico}")


if __name__ == "__main__":
    main()
