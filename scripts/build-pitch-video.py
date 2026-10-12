"""Assemble a ~2.5min product pitch video: voiceover + on-screen text + B-roll."""
from __future__ import annotations

import asyncio
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

import edge_tts
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "apps" / "web"
OUT_DIR = ROOT / "website" / "assets" / "video"
WORK = ROOT / "tmp" / "pitch-video"
ICON = WEB / "public" / "icon-512.png"

SKELLAM = OUT_DIR / "skellam.mp4"
GAUSSIAN = OUT_DIR / "gaussian.mp4"
BERNOULLI = OUT_DIR / "bernoulli.mp4"
LOGNORMAL = OUT_DIR / "lognormal.mp4"

BG = (18, 17, 14)
CREAM = (243, 234, 212)
GOLD = (212, 160, 23)
DIM = (180, 170, 150)
W, H = 1280, 720

VOICE = "en-US-GuyNeural"
RATE = "+0%"
GAP = 0.2
TARGET_SEC = 150.0  # soft target ~2.5min


@dataclass
class Seg:
    voice: str
    kind: str  # "card" | "broll"
    kicker: str = ""
    title: str = ""
    body: str = ""
    caption: str = ""
    src: Path | None = None
    start: float = 0.0
    min_dur: float = 0.0
    pad_after: float = 0.35  # hold after speech


# ~2.5 min cut — tight VO, product-led middle
SEGS: list[Seg] = [
    Seg(
        kind="card",
        kicker="Continuous",
        title="Prediction markets for continuous outcomes",
        body="Score margins · CPI · crypto levels · vote shares — not only YES/NO.",
        voice=(
            "I'm building Continuous — prediction markets for continuous outcomes. "
            "Score margins, CPI prints, crypto levels, vote shares — not only yes or no."
        ),
        caption="Continuous — markets for continuous outcomes",
        min_dur=8,
        pad_after=0.5,
    ),
    Seg(
        kind="card",
        kicker="Builder",
        title="Architect — Java, Rust, production systems",
        body="Postgres · Redis · Spring · K8s · data systems. Settlement & microstructure — not a thin frontend.",
        voice=(
            "I'm a software architect — mainly Java and Rust — with production systems across "
            "Postgres, Redis, Spring Boot, Kubernetes, and large-scale data. "
            "Python and ML are tools. Continuous is settlement and market microstructure; "
            "I've already shipped that middle in the open."
        ),
        caption="Architect background — shipped the hard middle open-source",
        min_dur=16,
        pad_after=0.5,
    ),
    Seg(
        kind="card",
        kicker="Thesis",
        title="Wrong product shape for continuous events",
        body="Live density. Trade ranges and sets. Risk-capital auction for LPs. Soft solvency + USDC on Solana.",
        voice=(
            "Most platforms force continuous questions into binaries or thin sibling markets. "
            "Continuous keeps a live probability density, trades ranges and sets, "
            "and auctions risk capital so LPs underwrite shortfalls — "
            "with soft solvency and USDC settlement on Solana."
        ),
        caption="Live density · ranges & sets · risk auction · USDC on Solana",
        min_dur=16,
        pad_after=0.5,
    ),
    Seg(
        kind="broll",
        src=SKELLAM,
        start=128,
        voice=(
            "Here's a Skellam demo — trading the Arsenal–Chelsea score difference "
            "as one continuous market, not a pile of yes-no lines."
        ),
        caption="Skellam — EPL score difference as a continuous market",
        min_dur=16,
        pad_after=0.7,
    ),
    Seg(
        kind="broll",
        src=SKELLAM,
        start=100,
        voice=(
            "Traders buy probability mass on sets. LMSR updates the live density as tickets fill."
        ),
        caption="Trade sets — LMSR updates the live density",
        min_dur=14,
        pad_after=0.7,
    ),
    Seg(
        kind="broll",
        src=SKELLAM,
        start=55,
        voice=(
            "In parallel, LPs quote risk capital to cover payout shortfalls — risk with a clear job."
        ),
        caption="Risk-capital auction — LPs underwrite shortfalls",
        min_dur=13,
        pad_after=0.6,
    ),
    Seg(
        kind="broll",
        src=GAUSSIAN,
        start=125,
        voice=(
            "Same stack, five families — Bernoulli, Gaussian, lognormal, Dirichlet, Skellam — "
            "full create-to-claim demos open source."
        ),
        caption="Five families · create → claim lifecycle demos",
        min_dur=15,
        pad_after=0.6,
    ),
    Seg(
        kind="broll",
        src=BERNOULLI,
        start=130,
        voice=(
            "Soft solvency: one recovery rate before winners claim. The books stay operable under stress."
        ),
        caption="Soft solvency — one recovery rate ρ before claim",
        min_dur=13,
        pad_after=0.6,
    ),
    Seg(
        kind="card",
        kicker="Why me · Raise",
        title="Shipped create-to-claim — raising to go live",
        body="Clear thesis, working software, full time. Launch, operate, distribute — including on X. No fake users.",
        voice=(
            "Why me: I shipped the hard middle — rules, settlement, create to claim — not a landing page. "
            "I'm raising to launch, operate, and distribute, including on X, and keep building full time. "
            "I don't claim users yet — I claim a thesis, working software, and running it in public."
        ),
        caption="Raising to launch · operate · distribute — full time, in public",
        min_dur=18,
        pad_after=0.6,
    ),
    Seg(
        kind="card",
        kicker="Continuous",
        title="Open source · Solana · USDC",
        body=(
            "github.com/bamaao/continuous-pdf-prediction-market\n"
            "bamaao.github.io/continuous-pdf-prediction-market"
        ),
        voice="Continuous. Open source. Solana. USDC. Code and demos on GitHub.",
        caption="Open source · Solana · USDC",
        min_dur=8,
        pad_after=1.0,
    ),
]


def font(size: int, bold: bool = False) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = [
        Path(r"C:\Windows\Fonts\georgiab.ttf") if bold else Path(r"C:\Windows\Fonts\georgia.ttf"),
        Path(r"C:\Windows\Fonts\arialbd.ttf") if bold else Path(r"C:\Windows\Fonts\arial.ttf"),
    ]
    for p in candidates:
        if p.is_file():
            return ImageFont.truetype(str(p), size)
    return ImageFont.load_default()


def mono(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for p in (Path(r"C:\Windows\Fonts\consola.ttf"), Path(r"C:\Windows\Fonts\cour.ttf")):
        if p.is_file():
            return ImageFont.truetype(str(p), size)
    return font(size)


def wrap(draw: ImageDraw.ImageDraw, text: str, fnt, max_width: int) -> list[str]:
    lines: list[str] = []
    for paragraph in text.split("\n"):
        words = paragraph.split()
        if not words:
            lines.append("")
            continue
        cur = ""
        for w in words:
            trial = f"{cur} {w}".strip()
            if draw.textlength(trial, font=fnt) <= max_width:
                cur = trial
            else:
                if cur:
                    lines.append(cur)
                cur = w
        if cur:
            lines.append(cur)
    return lines


def run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)


def probe_dur(path: Path) -> float:
    out = subprocess.check_output(
        [
            "ffprobe",
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
            str(path),
        ],
        text=True,
    ).strip()
    return float(out)


def render_card(path: Path, kicker: str, title: str, body: str) -> None:
    im = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(im)
    d.rectangle([64, 72, 200, 76], fill=GOLD)

    show_icon = ICON.is_file() and kicker.lower().startswith("continuous")
    if show_icon:
        icon = Image.open(ICON).convert("RGBA").resize((88, 88), Image.Resampling.LANCZOS)
        im.paste(icon, (64, 100), icon)

    fk, ft, fb = mono(18), font(40, bold=True), font(24)
    y = 210 if show_icon else 120
    if kicker:
        d.text((64, 100 if not show_icon else 200), kicker.upper(), font=fk, fill=GOLD)
        y = 140 if not show_icon else 240

    for line in wrap(d, title, ft, W - 128):
        d.text((64, y), line, font=ft, fill=CREAM)
        y += 50
    y += 14
    for line in wrap(d, body, fb, W - 128):
        d.text((64, y), line, font=fb, fill=DIM)
        y += 34
    im.save(path)


def caption_overlay(caption: str, dest: Path) -> None:
    bar = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(bar)
    d.rectangle([0, H - 150, W, H], fill=(14, 13, 11, 220))
    fb = font(26)
    lines = wrap(d, caption, fb, W - 80)
    y = H - 128
    for line in lines[:3]:
        d.text((40, y), line, font=fb, fill=CREAM)
        y += 34
    bar.save(dest)


async def synth_voice(text: str, mp3: Path) -> None:
    communicate = edge_tts.Communicate(text, VOICE, rate=RATE)
    await communicate.save(str(mp3))


def pad_audio(src: Path, dest: Path, target: float) -> None:
    run(
        [
            "ffmpeg",
            "-y",
            "-i",
            str(src),
            "-af",
            f"apad=whole_dur={target:.3f}",
            "-t",
            f"{target:.3f}",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            str(dest),
        ]
    )


def card_clip(png: Path, seconds: float, dest: Path) -> None:
    run(
        [
            "ffmpeg",
            "-y",
            "-loop",
            "1",
            "-i",
            str(png),
            "-t",
            f"{seconds:.3f}",
            "-vf",
            f"scale={W}:{H},setsar=1,format=yuv420p",
            "-r",
            "25",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "20",
            "-an",
            str(dest),
        ]
    )


def broll_clip(src: Path, start: float, seconds: float, caption_png: Path, dest: Path) -> None:
    run(
        [
            "ffmpeg",
            "-y",
            "-ss",
            f"{start:.3f}",
            "-stream_loop",
            "-1",
            "-i",
            str(src),
            "-i",
            str(caption_png),
            "-t",
            f"{seconds:.3f}",
            "-filter_complex",
            (
                f"[0:v]scale={W}:{H}:force_original_aspect_ratio=decrease,"
                f"pad={W}:{H}:(ow-iw)/2:(oh-ih)/2:color=0x12110e,setsar=1,fps=25[v];"
                f"[v][1:v]overlay=0:0,format=yuv420p[out]"
            ),
            "-map",
            "[out]",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "22",
            "-an",
            str(dest),
        ]
    )


def mux(video: Path, audio: Path, dest: Path) -> None:
    # Re-encode both so every segment shares CFR 25fps + 44.1kHz AAC timestamps.
    run(
        [
            "ffmpeg",
            "-y",
            "-i",
            str(video),
            "-i",
            str(audio),
            "-r",
            "25",
            "-ar",
            "44100",
            "-ac",
            "1",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "21",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            "-fflags",
            "+genpts",
            "-movflags",
            "+faststart",
            str(dest),
        ]
    )


def concat_filter(parts: list[Path], dest: Path) -> None:
    """A/V concat via filter — avoids demuxer timestamp inflation."""
    args: list[str] = ["ffmpeg", "-y"]
    for p in parts:
        args += ["-i", str(p)]
    n = len(parts)
    streams = "".join(f"[{i}:v:0][{i}:a:0]" for i in range(n))
    fc = f"{streams}concat=n={n}:v=1:a=1[v][a]"
    args += [
        "-filter_complex",
        fc,
        "-map",
        "[v]",
        "-map",
        "[a]",
        "-r",
        "25",
        "-c:v",
        "libx264",
        "-preset",
        "fast",
        "-crf",
        "21",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-ar",
        "44100",
        "-movflags",
        "+faststart",
        str(dest),
    ]
    run(args)


def silence_aac(seconds: float, dest: Path) -> None:
    run(
        [
            "ffmpeg",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=mono",
            "-t",
            f"{seconds:.3f}",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            str(dest),
        ]
    )


async def build_segment(i: int, seg: Seg) -> Path:
    stem = f"{i:02d}"
    mp3 = WORK / f"{stem}.mp3"
    await synth_voice(seg.voice, mp3)
    voice_dur = probe_dur(mp3)
    target = max(voice_dur + seg.pad_after, seg.min_dur)
    aac = WORK / f"{stem}.m4a"
    pad_audio(mp3, aac, target)

    cap = seg.caption or seg.title or seg.voice[:80]
    v_silent = WORK / f"{stem}_v.mp4"

    if seg.kind == "card":
        png = WORK / f"{stem}_card.png"
        render_card(png, seg.kicker, seg.title, seg.body)
        cap_png = WORK / f"{stem}_cap.png"
        caption_overlay(cap, cap_png)
        raw = WORK / f"{stem}_card_raw.mp4"
        card_clip(png, target, raw)
        run(
            [
                "ffmpeg",
                "-y",
                "-i",
                str(raw),
                "-i",
                str(cap_png),
                "-filter_complex",
                "[0:v][1:v]overlay=0:0,format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "20",
                "-an",
                str(v_silent),
            ]
        )
    else:
        assert seg.src and seg.src.is_file(), f"missing b-roll {seg.src}"
        cap_png = WORK / f"{stem}_cap.png"
        caption_overlay(cap, cap_png)
        broll_clip(seg.src, seg.start, target, cap_png, v_silent)

    out = WORK / f"{stem}.mp4"
    mux(v_silent, aac, out)
    print(f"  {stem}: {target:.1f}s")
    return out


async def main_async() -> None:
    if not SKELLAM.is_file():
        raise SystemExit(f"missing {SKELLAM}")
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    parts: list[Path] = []
    for i, seg in enumerate(SEGS):
        print(f"[{i + 1}/{len(SEGS)}] synth + compose…")
        parts.append(await build_segment(i, seg))
        if i < len(SEGS) - 1 and GAP > 0:
            gap_v = WORK / f"{i:02d}_gap_v.mp4"
            gap_a = WORK / f"{i:02d}_gap_a.m4a"
            gap = WORK / f"{i:02d}_gap.mp4"
            black = WORK / f"{i:02d}_black.png"
            Image.new("RGB", (W, H), BG).save(black)
            card_clip(black, GAP, gap_v)
            silence_aac(GAP, gap_a)
            mux(gap_v, gap_a, gap)
            parts.append(gap)

    out = OUT_DIR / "pitch.mp4"
    print(f"concat {len(parts)} parts…")
    concat_filter(parts, out)
    dur = probe_dur(out)
    size = out.stat().st_size
    print(f"wrote {out} ({dur:.1f}s, {size / 1e6:.2f} MB)")
    if not (140 <= dur <= 165):
        print(f"WARN: duration {dur:.1f}s outside ~2.5min band (target ~{TARGET_SEC:.0f}s)")


def main() -> None:
    asyncio.run(main_async())


if __name__ == "__main__":
    main()
