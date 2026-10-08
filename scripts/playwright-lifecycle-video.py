#!/usr/bin/env python3
"""Record one distribution family's main lifecycle as a human-paced Playwright WebM.

  python scripts/playwright-lifecycle-video.py
  python scripts/playwright-lifecycle-video.py Gaussian

Pacing (override via env):
  LIFECYCLE_SLOW_MO_MS              default 280  — per keystroke/click delay
  LIFECYCLE_VIDEO_ACTION_PAUSE_MS   default 1800 — dwell after each click
  LIFECYCLE_VIDEO_STEP_PAUSE_MS     default 4000 — beat between major stages

Output: tmp/playwright-lifecycle/video/lifecycle-<Family>-*.webm
"""

from __future__ import annotations

import os
import runpy
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FAMILY = (sys.argv[1] if len(sys.argv) > 1 else "Gaussian").strip()

os.environ["LIFECYCLE_FAMILIES"] = FAMILY
os.environ.setdefault("LIFECYCLE_BRANCHES", "0")
os.environ["LIFECYCLE_VIDEO"] = "1"
os.environ.setdefault("LIFECYCLE_DEMO_DATA", "1")
os.environ.setdefault(
    "LIFECYCLE_DEMO_DATA_PATH",
    str(ROOT / "fixtures" / "lifecycle-demo.json"),
)
os.environ.setdefault(
    "LIFECYCLE_VIDEO_DIR",
    str(ROOT / "tmp" / "playwright-lifecycle" / "video"),
)
# Human-like pacing for this entrypoint. Override only via explicit env *before*
# launch if you want different numbers; suite runs should call the report script
# directly (not this wrapper) so they stay fast.
os.environ["LIFECYCLE_SLOW_MO_MS"] = os.environ.get("LIFECYCLE_VIDEO_SLOW_MO_MS", "280")
os.environ["LIFECYCLE_VIDEO_ACTION_PAUSE_MS"] = os.environ.get(
    "LIFECYCLE_VIDEO_ACTION_PAUSE_MS", "1800"
)
os.environ["LIFECYCLE_VIDEO_STEP_PAUSE_MS"] = os.environ.get(
    "LIFECYCLE_VIDEO_STEP_PAUSE_MS", "4000"
)
os.environ.setdefault("LIFECYCLE_HEADED", "0")
os.environ.setdefault("LIFECYCLE_CLOSE_IN", "100")

print(
    f"== lifecycle video (human pace): family={FAMILY} "
    f"slow_mo={os.environ['LIFECYCLE_SLOW_MO_MS']}ms "
    f"action={os.environ['LIFECYCLE_VIDEO_ACTION_PAUSE_MS']}ms "
    f"step={os.environ['LIFECYCLE_VIDEO_STEP_PAUSE_MS']}ms =="
)
print(f"   video dir: {os.environ['LIFECYCLE_VIDEO_DIR']}")

sys.argv = [str(ROOT / "scripts" / "playwright-lifecycle-report.py")]
runpy.run_path(str(ROOT / "scripts" / "playwright-lifecycle-report.py"), run_name="__main__")
