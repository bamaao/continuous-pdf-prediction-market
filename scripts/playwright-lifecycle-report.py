#!/usr/bin/env python3
"""Playwright full lifecycle across every distribution family (en-US).

For each family (Skellam, Gaussian, Lognormal, Dirichlet, Bernoulli):
  create → review approve/open → Open session → trade → risk auction quote
  → wait close → resolve Open window → Submit result → Finalize → Lock ρ

Outputs: tmp/playwright-lifecycle/report.md + report.json + per-family screenshots.

Video (optional): set LIFECYCLE_VIDEO=1 — or use scripts/playwright-lifecycle-video.py
for one family main-flow WebM under tmp/playwright-lifecycle/video/.
"""

from __future__ import annotations

import importlib.util
import json
import os
import re
import sys
import time
import urllib.request
from dataclasses import asdict, dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path

from playwright.sync_api import TimeoutError as PwTimeout
from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
BASE = os.environ.get("WEB_URL", "http://127.0.0.1:3000")
API = os.environ.get("MARKET_API", "http://127.0.0.1:8080")
GW = os.environ.get("GATEWAY", "http://127.0.0.1:8081")
RPC = os.environ.get("RPC_URL", "http://127.0.0.1:8899")
OUT = ROOT / "tmp" / "playwright-lifecycle"
CLOSE_IN = int(os.environ.get("LIFECYCLE_CLOSE_IN", "100"))
FAMILIES = os.environ.get("LIFECYCLE_FAMILIES", "Skellam,Gaussian,Lognormal,Dirichlet,Bernoulli").split(",")
# Set LIFECYCLE_VIDEO=1 to record a WebM under OUT/video/ (one family + LIFECYCLE_BRANCHES=0 is best).
RECORD_VIDEO = os.environ.get("LIFECYCLE_VIDEO", "0") == "1"
VIDEO_DIR = Path(os.environ.get("LIFECYCLE_VIDEO_DIR", str(OUT / "video")))
# Typing/click cadence (Playwright slow_mo). Video entry script defaults this higher.
SLOW_MO_MS = int(os.environ.get("LIFECYCLE_SLOW_MO_MS", "250" if RECORD_VIDEO else "0"))
# Dwell after a click / fill so the UI is readable on camera.
VIDEO_ACTION_PAUSE_MS = int(os.environ.get("LIFECYCLE_VIDEO_ACTION_PAUSE_MS", "1800"))
# Longer beat between major lifecycle stages (create → trade → auction …).
VIDEO_STEP_PAUSE_MS = int(os.environ.get("LIFECYCLE_VIDEO_STEP_PAUSE_MS", "3500"))
HEADED = os.environ.get("LIFECYCLE_HEADED", "0") == "1"
# Realistic listing copy: fixtures/lifecycle-demo.json (on by default for video).
_DEMO_DEFAULT = "1" if RECORD_VIDEO else "0"
USE_DEMO_DATA = os.environ.get("LIFECYCLE_DEMO_DATA", _DEMO_DEFAULT) == "1"
DEMO_DATA_PATH = Path(
    os.environ.get("LIFECYCLE_DEMO_DATA_PATH", str(ROOT / "fixtures" / "lifecycle-demo.json"))
)
DEMO_COVERS_DIR = Path(
    os.environ.get("LIFECYCLE_DEMO_COVERS_DIR", str(ROOT / "fixtures" / "lifecycle-covers"))
)
_DEMO_CACHE: dict | None = None


def load_demo_data() -> dict:
    global _DEMO_CACHE
    if _DEMO_CACHE is not None:
        return _DEMO_CACHE
    if not USE_DEMO_DATA or not DEMO_DATA_PATH.is_file():
        _DEMO_CACHE = {}
        return _DEMO_CACHE
    _DEMO_CACHE = json.loads(DEMO_DATA_PATH.read_text(encoding="utf-8"))
    return _DEMO_CACHE


def demo_family(family: str, stamp: str) -> dict:
    """Resolved listing fields for one family; empty dict if demo data off/missing."""
    root = load_demo_data()
    raw = (root.get("families") or {}).get(family) or {}
    if not raw:
        return {}
    out = dict(raw)
    for key in ("title", "event", "description", "tags", "topic", "tag_release", "evidence"):
        if isinstance(out.get(key), str):
            out[key] = out[key].replace("{stamp}", stamp)
    topic = str(out.get("topic") or "")
    if len(topic) > 28:
        out["topic"] = topic[:28]
    return out

spec = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec)
sys.modules["flow"] = flow
assert spec.loader
spec.loader.exec_module(flow)


@dataclass
class Step:
    name: str
    ok: bool
    elapsed_ms: int
    detail: str = ""
    shot: str = ""


@dataclass
class FamilyRun:
    family: str
    market: str = ""
    ok: bool = False
    steps: list[Step] = field(default_factory=list)
    error: str = ""


RUNS: list[FamilyRun] = []


def fill_label(page, name: str, value: str, *, textarea: bool = False) -> None:
    if textarea or name in ("Description", "Native description", "Evidence (off-chain note or URL)"):
        page.get_by_role("textbox", name=name, exact=True).fill(value)
        return
    page.get_by_label(name, exact=True).fill(value)


def human_pause(page, label: str = "", *, step: bool = False) -> None:
    """Hold the frame so a recording looks like a person reading the screen."""
    if not RECORD_VIDEO:
        return
    ms = VIDEO_STEP_PAUSE_MS if step else VIDEO_ACTION_PAUSE_MS
    if label:
        flow.out(f"video pause{' step' if step else ''} {label} {ms}ms")
    try:
        page.wait_for_timeout(ms)
    except Exception:
        time.sleep(ms / 1000.0)


def human_goto(page, url: str, label: str = "") -> None:
    page.goto(url, wait_until="networkidle")
    human_pause(page, label or url, step=True)


def human_click(locator, page, label: str = "") -> None:
    target = locator.first if hasattr(locator, "first") else locator
    try:
        target.scroll_into_view_if_needed(timeout=5_000)
    except Exception:
        pass
    human_pause(page, f"before:{label or 'click'}")
    target.click()
    human_pause(page, label or "click")


def shot(page, name: str) -> str:
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / f"{name}.png"
    page.screenshot(path=str(path), full_page=True)
    return str(path)


def record(run: FamilyRun, name: str, ok: bool, t0: float, detail: str = "", shot_path: str = "") -> None:
    run.steps.append(Step(name, ok, int((time.time() - t0) * 1000), detail[:400], shot_path))
    flow.out(f"[{'OK' if ok else 'FAIL'}] {run.family}/{name} {detail[:120]}")


def pool_clock() -> dict:
    with urllib.request.urlopen(API + "/v1/pool", timeout=5) as r:
        return json.loads(r.read().decode())


def gateway_ok() -> bool:
    try:
        with urllib.request.urlopen(GW + "/v1/health", timeout=4) as r:
            return bool(json.loads(r.read().decode()).get("ok"))
    except Exception:
        return False


def stack_ok() -> dict:
    out = {}
    try:
        with urllib.request.urlopen(API + "/v1/health", timeout=5) as r:
            out["api"] = json.loads(r.read().decode())
    except Exception as e:
        out["api"] = {"error": str(e)}
    try:
        out["pool"] = pool_clock()
    except Exception as e:
        out["pool"] = {"error": str(e)}
    out["gateway"] = gateway_ok()
    try:
        with urllib.request.urlopen(BASE + "/", timeout=8) as r:
            out["web"] = r.status
    except Exception as e:
        out["web"] = str(e)
    return out


def wait_trading_closed(page, timeout_s: int) -> None:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        body = page.locator("body").inner_text().lower()
        if "trading closed" in body:
            return
        page.wait_for_timeout(2000)
        if time.time() + 3 > deadline:
            break
        page.reload(wait_until="networkidle")
    # last chance — deadline may have passed without UI refresh
    page.reload(wait_until="networkidle")


def ensure_session(page) -> None:
    body = page.locator("body").inner_text().lower()
    if "session live" in body or "cap " in body:
        return
    btn = page.get_by_role("button", name="Open session")
    if btn.count() == 0:
        raise RuntimeError("Open session not available")
    btn.click()
    flow.wait_session_live(page, timeout_ms=60_000)


def aside_mask(page) -> str:
    m = re.search(r"mask=([0-9a-fA-F]+)", page.locator("aside").inner_text())
    return (m.group(1) if m else "").lower()


def mask_empty(hex_s: str) -> bool:
    return not hex_s or all(c == "0" for c in hex_s)


def wait_buy_confirmed(page) -> None:
    try:
        flow.wait_text(page, "confirmed", timeout_ms=25_000)
        return
    except PwTimeout:
        body = page.locator("aside").inner_text() + "\n" + page.locator("body").inner_text()
        if "confirmed" in body.lower():
            return
        nonce = page.locator("aside input[type='number']").nth(1)
        if nonce.count():
            try:
                cur = nonce.input_value()
                nonce.fill(str(int(cur or "1") + 1))
            except Exception:
                pass
        buy = page.get_by_role("button", name="Buy set")
        if buy.count() == 0:
            buy = page.get_by_role("button", name="Buy line")
        if buy.count() == 0:
            buy = page.locator("aside button").filter(has_text=re.compile(r"Buy (set|line)", re.I))
        if buy.count() and buy.first.is_enabled():
            buy.first.click()
        flow.wait_text(page, "confirmed", timeout_ms=45_000)


def buy_for_family(page, family: str, stamp: str = "") -> None:
    demo = demo_family(family, stamp or "demo")
    shares = str(demo.get("buy_shares") or ("5" if family == "Bernoulli" else "1"))
    spins = page.locator("aside input[type='number']")
    if spins.count():
        try:
            spins.first.fill(shares)
        except Exception:
            pass
    if family == "Skellam":
        ticket = str(demo.get("ticket") or "1X2 Home")
        for _ in range(16):
            if page.get_by_role("button", name=ticket).count() or page.get_by_role("button", name="1X2 Home").count() or page.get_by_role("button", name="Home", exact=True).count():
                break
            page.wait_for_timeout(400)
            page.reload(wait_until="networkidle")
        if page.get_by_role("button", name=ticket).count():
            page.get_by_role("button", name=ticket).click()
        elif page.get_by_role("button", name="1X2 Home").count():
            page.get_by_role("button", name="1X2 Home").click()
        else:
            page.get_by_role("button", name="Home", exact=True).click()
        page.wait_for_timeout(600)
        buy = page.get_by_role("button", name="Buy line")
        if buy.count() == 0:
            buy = page.get_by_role("button", name="Buy set")
        buy.first.click()
        wait_buy_confirmed(page)
        return

    bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
    for _ in range(24):
        if bars.count() >= 1:
            break
        page.wait_for_timeout(400)
        page.reload(wait_until="networkidle")
        bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
    if bars.count() < 1:
        raise RuntimeError(f"no PDF bars (count={bars.count()})")

    # Board hydrates with cell 0 selected. Clicking it toggles OFF → mask=00 empty set.
    page.wait_for_timeout(400)
    mx = aside_mask(page)
    if mask_empty(mx):
        bars.nth(0).click()
        page.wait_for_timeout(300)
        mx = aside_mask(page)
    if family != "Bernoulli" and bars.count() > 1:
        # add a second cell without toggling the first off
        bars.nth(1).click()
        page.wait_for_timeout(300)
        mx = aside_mask(page)
        if mask_empty(mx):
            bars.nth(0).click()
            page.wait_for_timeout(300)
            mx = aside_mask(page)
    if mask_empty(mx):
        raise RuntimeError(f"{family} empty mask after select: {page.locator('aside').inner_text()[:160]}")

    buy = page.get_by_role("button", name="Buy set")
    if buy.count() == 0:
        buy = page.locator("aside button").filter(has_text=re.compile(r"Buy set", re.I))
    expect(buy.first).to_be_enabled(timeout=10_000)
    buy.first.click()
    wait_buy_confirmed(page)


def _fill_number(loc, text: str) -> None:
    """React controlled number inputs need click + clear + type so onChange updates state."""
    target = loc.first
    target.click()
    target.fill("")
    target.press_sequentially(str(text), delay=40)


def fill_outcome(page, family: str, stamp: str = "") -> None:
    oc = (demo_family(family, stamp or "demo").get("outcome") or {}) if USE_DEMO_DATA else {}
    if family == "Skellam":
        _fill_number(page.get_by_label("Home", exact=True), str(oc.get("home", "1")))
        _fill_number(page.get_by_label("Away", exact=True), str(oc.get("away", "0")))
    elif family == "Gaussian":
        _fill_number(page.get_by_label("Scalar x*", exact=True), str(oc.get("scalar", "2.4")))
    elif family == "Lognormal":
        _fill_number(page.get_by_label("Scalar x*", exact=True), str(oc.get("scalar", "100000")))
    elif family == "Dirichlet":
        _fill_number(page.get_by_label("Winning atom", exact=True), str(oc.get("atom", "0")))
    elif family == "Bernoulli":
        yes = page.get_by_label("YES", exact=True)
        no = page.get_by_label("NO", exact=True)
        if oc.get("yes", True):
            if yes.count():
                yes.check()
        elif no.count():
            no.check()
        elif yes.count():
            yes.check()


def bump_datetime_local(value: str, extra_s: int) -> str:
    raw = (value or "")[:16]
    try:
        dt = datetime.strptime(raw, "%Y-%m-%dT%H:%M")
    except ValueError:
        dt = datetime.now()
    dt = dt + timedelta(seconds=max(0, extra_s))
    return dt.strftime("%Y-%m-%dT%H:%M")


def submit_application(
    page,
    family: str,
    stamp: str,
    *,
    title: str | None = None,
    event: str | None = None,
    close_in: int | None = None,
    report_extra_s: int = 0,
    topic: str | None = None,
    early_void: bool = False,
) -> str:
    demo = demo_family(family, stamp)
    title = title or demo.get("title") or f"Life {family} {stamp}"
    event = event or demo.get("event") or f"{family} lifecycle event {stamp}"
    desc = demo.get("description") or (
        f"Playwright lifecycle ({family}): English canonical. Settle on committee submit_result."
    )
    tags = demo.get("tags") or f"{family.lower()}, lifecycle"
    topic_v = (topic or demo.get("topic") or f"lf{family[:2].lower()}{stamp}")[:28]
    tag_rel = demo.get("tag_release") or "life"
    close_in = CLOSE_IN if close_in is None else close_in
    page.goto(BASE + "/create", wait_until="networkidle")
    page.get_by_role("button", name=family, exact=False).first.click()
    page.wait_for_timeout(400)
    n_grid = demo.get("n_grid")
    if family in ("Gaussian", "Lognormal") and page.get_by_label("n_grid / atoms").count():
        page.get_by_label("n_grid / atoms").select_option(str(n_grid or "8"))
    if early_void and family == "Bernoulli":
        box = page.get_by_role("checkbox", name=re.compile(r"early occurrence", re.I))
        if box.count() and not box.first.is_checked():
            box.first.check()
    fill_label(page, "Market title", title)
    fill_label(page, "Tags", tags)
    fill_label(page, "Trading event", event)
    fill_label(page, "Description", desc, textarea=True)
    fill_label(page, "Topic / series (on-chain id)", topic_v)
    if page.get_by_label("Tag / release", exact=True).count():
        fill_label(page, "Tag / release", str(tag_rel)[:32])
    if page.get_by_label("Or seconds from now", exact=True).count():
        fill_label(page, "Or seconds from now", str(close_in))
    close_at = page.get_by_label("Close at (absolute)")
    report_from = page.get_by_label("Committee may report from")
    if close_at.count() and report_from.count():
        report_from.fill(bump_datetime_local(close_at.input_value(), report_extra_s))
    bond = page.get_by_label("Committee bond (USDC)")
    if bond.count() and bond.first.is_enabled():
        bond.fill("100")
    cover_name = str(demo.get("cover") or "").strip()
    if USE_DEMO_DATA and cover_name:
        cover_path = Path(cover_name)
        if not cover_path.is_file():
            cover_path = DEMO_COVERS_DIR / cover_name
        if cover_path.is_file():
            file_input = page.locator('input[type="file"][accept*="image"]')
            expect(file_input.first).to_be_attached(timeout=10_000)
            file_input.first.set_input_files(str(cover_path))
            try:
                page.get_by_text("cover uploaded", exact=False).wait_for(timeout=20_000)
            except PwTimeout:
                # Preview img is enough proof the desk accepted the file.
                if page.locator("img[src^='blob:'], img.max-h-32").count() == 0:
                    raise RuntimeError(f"cover upload failed for {cover_path}")
            human_pause(page, "after-cover-upload")
            flow.out(f"demo cover ← {cover_path.name}")
        else:
            flow.out(f"demo cover missing: {cover_path}")
    if USE_DEMO_DATA and demo:
        flow.out(f"demo listing: {title[:72]}")
        human_pause(page, "review-listing-copy", step=True)
    page.get_by_role("button", name="Submit for review").click()
    return title


def create_family(
    page, family: str, stamp: str, *, close_in: int | None = None, report_extra_s: int = 0, early_void: bool = False
) -> str:
    demo = demo_family(family, stamp)
    title = demo.get("title") or f"Life {family} {stamp}"
    submit_application(
        page, family, stamp, title=title, close_in=close_in, report_extra_s=report_extra_s, early_void=early_void
    )
    flow.wait_text(page, "after review", timeout_ms=45_000)

    page.get_by_role("link", name="Review").click()
    page.wait_for_load_state("networkidle")
    page.locator("li").filter(has_text=title).get_by_role(
        "button", name="Approve and open prediction market"
    ).click()
    link = page.get_by_role("link", name="Open prediction market", exact=True)
    try:
        expect(link).to_be_visible(timeout=90_000)
    except Exception:
        raise RuntimeError(flow.note_or_err(page)[:500])
    href = link.get_attribute("href") or ""
    market = href.split("/m/")[-1]
    wait_market_indexed(market)
    return market


def api_json(path: str):
    with urllib.request.urlopen(API + path, timeout=8) as r:
        return json.loads(r.read().decode())


def official_clocks() -> tuple[int, int]:
    try:
        p = api_json("/v1/protocol")
        return int(p.get("report_window_secs") or 90), int(p.get("challenge_secs") or 8)
    except Exception:
        return 90, 8


def wait_market_indexed(market: str, timeout_s: int = 90) -> None:
    deadline = time.time() + timeout_s
    last = ""
    while time.time() < deadline:
        try:
            info = api_json(f"/v1/markets/{market}/info")
            if info.get("n") or info.get("family") is not None:
                return
            last = str(info)[:120]
        except Exception as e:
            last = str(e)[:120]
        time.sleep(2)
    raise RuntimeError(f"market not indexed: {market} {last}")


def ensure_official_protocol(kp) -> str:
    """Write Protocol PDA from operator env (compose stamps PLATFORM_* / PROTOCOL_*)."""
    from solana.rpc.api import Client

    rpc = Client(RPC)
    owner = str(kp.pubkey())
    try:
        flow.send_ixs(rpc, kp, [flow.compose("init_protocol", owner=owner)])
        return "init_protocol"
    except Exception as e:
        msg = str(e).lower()
        if "already in use" in msg or "0x0" in msg:
            pass
        try:
            flow.send_ixs(rpc, kp, [flow.compose("set_protocol", owner=owner)])
            return f"set_protocol after {e.__class__.__name__}"
        except Exception as e2:
            return f"protocol_ix_failed init={e} set={e2}"


def try_cover_lp_loss_chain(market: str) -> str:
    """Send cover_lp_loss as Market.platform. No calendar gate.

    Inner no-op (uncovered=0 or empty pool) still succeeds. A chain error is a FAIL.
    """
    try:
        kp = flow.load_kp(flow.KP_PATH)
    except Exception as e:
        return f"FAIL cover_chain=no_kp:{e}"
    from solana.rpc.api import Client

    rpc = Client(RPC)
    try:
        ix = flow.compose("cover_lp_loss", owner=str(kp.pubkey()), market=market)
        flow.send_ixs(rpc, kp, [ix])
        return "cover_lp_loss=ok"
    except Exception as e:
        msg = str(e).replace("\n", " ")
        return f"FAIL cover_lp_loss={msg[:400]}"


def click_if(root, name: str, timeout_ms: int = 8_000, page=None) -> str:
    _ = timeout_ms
    btn = root.get_by_role("button", name=name, exact=True)
    if btn.count() == 0:
        btn = root.locator("button").filter(has_text=re.compile(rf"^{re.escape(name)}$", re.I))
    if btn.count() == 0 or not btn.first.is_enabled():
        return f"{name}=skip"
    if RECORD_VIDEO and page is not None:
        human_click(btn, page, name)
    else:
        btn.first.click()
        time.sleep(1.2)
    return f"{name}=ok"


def claim_lp(page, market: str) -> str:
    """Draw H / premium / surplus / fund cover on /lp after Lock ρ. Cover is platform-signed."""
    bits: list[str] = []
    settled = False
    for i in range(24):
        page.goto(BASE + "/lp", wait_until="networkidle")
        if RECORD_VIDEO and i == 0:
            human_pause(page, "lp-desk", step=True)
        row = page.locator("li").filter(has_text=market[:8])
        if row.count() == 0:
            row = page.locator("li").filter(has_text=market)
        if row.count() and row.first.get_by_role("button", name="Draw H").count():
            settled = True
            break
        page.wait_for_timeout(1500)
    if row.count() == 0:
        return "no_quote_row"
    if not settled:
        return "not_settled"
    human_pause(page, "lp-row", step=True)
    for label in ("Draw H", "Premium", "Surplus", "Fund cover"):
        bits.append(click_if(row.first, label, page=page))
    page.wait_for_timeout(800)
    page.reload(wait_until="networkidle")
    human_pause(page, "lp-after-reload", step=True)
    body = page.locator("body").inner_text().lower()
    if "cover paid" not in body:
        bits.append("ui_cover_paid_missing")
    cover = page.get_by_role("button", name="Claim cover")
    if cover.count() and cover.first.is_enabled():
        human_click(cover, page, "Claim cover")
        bits.append("Claim cover=ok")
    elif cover.count():
        bits.append("Claim cover=disabled")
    else:
        bits.append("Claim cover=skip")
    chain = try_cover_lp_loss_chain(market)
    bits.append(chain)
    detail = " ".join(bits)
    if "ui_cover_paid_missing" in detail or chain.startswith("FAIL"):
        return "FAIL " + detail
    return detail


def claim_platform(page, market: str) -> str:
    """Claim φ and S_P as Market.platform. Fund C_P stays a separate button."""
    info = None
    for _ in range(24):
        try:
            info = api_json(f"/v1/markets/{market}/info")
            if info.get("platform"):
                break
        except Exception:
            info = None
        time.sleep(1)
    if not info or not info.get("platform"):
        return "platform_not_indexed"
    human_goto(page, BASE + "/create", "platform-claims")
    # Platform claim section uses the first "market pubkey" placeholder.
    inp = page.get_by_placeholder("market pubkey", exact=True)
    if inp.count() == 0:
        inp = page.get_by_placeholder("market pubkey")
    if inp.count():
        inp.first.fill(market)
        human_pause(page, "after-fill-platform-market")
    fees = page.get_by_role("button", name="Claim fees (φ)")
    sp = page.get_by_role("button", name="Claim S_P")
    fund = page.get_by_role("button", name="Fund C_P")
    if fund.count() == 0:
        return "missing_fund_cp"
    deadline = time.time() + 30
    while time.time() < deadline:
        if fees.count() and fees.first.is_enabled() and sp.count() and sp.first.is_enabled():
            break
        page.wait_for_timeout(800)
    if fees.count() == 0 or not fees.first.is_enabled():
        return f"fees_disabled platform={info.get('platform')}"
    human_click(fees, page, "Claim fees (φ)")
    note_fees = ""
    deadline = time.time() + 45
    while time.time() < deadline:
        body = page.locator("body").inner_text()
        if "claim_fees" in body or "forbidden" in body.lower():
            note_fees = next((ln for ln in body.splitlines() if "claim_fees" in ln or "forbidden" in ln.lower()), body[-200:])
            break
        if fees.count() and fees.first.is_enabled() and time.time() > deadline - 40:
            # busy cleared; tx may have finished without visible note yet
            note_fees = flow.note_or_err(page)
            if "claim_fees" in note_fees or fees.first.is_enabled():
                break
        page.wait_for_timeout(500)
    if "forbidden" in note_fees.lower() or "403" in note_fees:
        return f"fees_forbidden {note_fees[:120]}"
    deadline = time.time() + 20
    while time.time() < deadline:
        if sp.count() and sp.first.is_enabled():
            break
        page.wait_for_timeout(500)
    if sp.count() == 0 or not sp.first.is_enabled():
        return f"sp_disabled {note_fees[:80]}"
    human_click(sp, page, "Claim S_P")
    note_sp = ""
    deadline = time.time() + 45
    while time.time() < deadline:
        body = page.locator("body").inner_text()
        if "pay_surplus_platform" in body or "NoSurplus" in body or "claim S_P failed" in body:
            note_sp = next(
                (
                    ln
                    for ln in body.splitlines()
                    if "pay_surplus_platform" in ln or "NoSurplus" in ln or "claim S_P" in ln
                ),
                body[-200:],
            )
            break
        if "forbidden" in body.lower() and "platform" in body.lower():
            note_sp = body[-200:]
            break
        if sp.count() and sp.first.is_enabled() and time.time() > deadline - 40:
            note_sp = flow.note_or_err(page)
            break
        page.wait_for_timeout(500)
    if "forbidden" in note_sp.lower() and "claim_fees" not in note_sp:
        return f"sp_forbidden {note_sp[:120]}"
    claimed = "claim_fees" in note_fees or "claim_fees" in page.locator("body").inner_text()
    surplus = (
        "pay_surplus_platform" in note_sp
        or "NoSurplus" in note_sp
        or "pay_surplus_platform" in page.locator("body").inner_text()
    )
    if not claimed:
        return f"fees_no_sig {note_fees[:120]}"
    if not surplus and not (sp.count() and sp.first.is_enabled()):
        return f"sp_no_note {note_sp[:120]}"
    return f"platform={info['platform'][:8]} fees_ok={int(claimed)} sp_ok={int(bool(surplus or (sp.count() and sp.first.is_enabled())))} fund_cp_separate=1"


def wait_submit_ready(page, family: str, market: str, timeout_s: int = 130) -> None:
    """Report window starts at report_open_ts (≥ close_ts). Button stays disabled until then."""
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        btn = page.get_by_role("button", name="Submit result", exact=True)
        if btn.count() and btn.first.is_enabled():
            return
        waiting = page.get_by_role("button", name="Waiting for report open")
        lockb = page.get_by_role("button", name="Lock committee bond")
        if lockb.count() and lockb.first.is_enabled():
            lockb.first.click()
            page.wait_for_timeout(1500)
        first = page.get_by_role("button", name="Lock committee bond first")
        if first.count() and lockb.count() and lockb.first.is_enabled():
            lockb.first.click()
        lockb = page.get_by_role("button", name="Lock committee bond")
        if lockb.count() and lockb.first.is_enabled():
            lockb.first.click()
            page.wait_for_timeout(1500)
        first = page.get_by_role("button", name="Lock committee bond first")
        if first.count() and lockb.count() and lockb.first.is_enabled():
            lockb.first.click()
        if waiting.count() == 0 and btn.count() == 0:
            # window not opened yet / still loading
            open_btn = page.get_by_role("button", name="Open window")
            if open_btn.count() and open_btn.first.is_enabled():
                open_btn.first.click()
                page.wait_for_timeout(1500)
        page.wait_for_timeout(1000)
        if int(time.time()) % 6 == 0:
            page.reload(wait_until="networkidle")
            fill_outcome(page, family)
            ev = page.get_by_role("textbox", name=re.compile(r"Evidence", re.I))
            if ev.count():
                ev.first.fill(f"Playwright lifecycle evidence {family} {market[:8]}")
    raise RuntimeError("Submit result stayed disabled (report_open_ts not reached)")


def wait_proposed(page, market: str, timeout_s: int = 90) -> None:
    """Confirm submit_result indexed (has_proposed / Challenge), not just UI copy."""
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            res = api_json(f"/v1/markets/{market}/resolution")
            if res.get("has_proposed") or res.get("phase_name") in ("Challenge", "Vote", "Final"):
                page.reload(wait_until="networkidle")
                try:
                    flow.wait_any_text(
                        page,
                        ["Proposed x*", "Challenge window", "challenge window", "Finalize"],
                        timeout_ms=12_000,
                    )
                except PwTimeout:
                    pass
                return
        except Exception:
            pass
        page.wait_for_timeout(1000)
        if int(time.time()) % 8 == 0:
            page.reload(wait_until="networkidle")
    raise RuntimeError("submit_result never indexed (has_proposed still false)")


def wait_report_form(page, market: str, timeout_s: int = 90) -> None:
    """After resolve_open, indexer lag can leave the UI on 'Open window' even though the sig landed.
    Poll /resolution and reload until Propose / Submit / Waiting appears."""
    deadline = time.time() + timeout_s
    needles = ["Propose x*", "Submit result", "Waiting for report open"]
    while time.time() < deadline:
        try:
            flow.wait_any_text(page, needles, timeout_ms=4_000)
            return
        except PwTimeout:
            pass
        indexed = False
        try:
            res = api_json(f"/v1/markets/{market}/resolution")
            # phase 0 + members means the record exists (Open), even before a proposal.
            if res.get("members") is not None or res.get("phase_name") in ("Open", "Propose", "Challenge", "Vote"):
                indexed = True
        except Exception:
            indexed = False
        if indexed or "resolve_open" in page.locator("body").inner_text().lower():
            page.reload(wait_until="networkidle")
            human_pause(page, "reload-after-resolve-open")
            continue
        page.wait_for_timeout(1000)
    raise RuntimeError("report form never appeared after Open window")


def resolve_and_settle(page, family: str, market: str, stamp: str = "") -> None:
    human_goto(page, f"{BASE}/resolve/{market}", "resolve-desk")
    open_btn = page.get_by_role("button", name="Open window")
    if open_btn.count() and open_btn.first.is_enabled():
        human_click(open_btn, page, "Open window")
    wait_report_form(page, market)
    human_pause(page, "resolve-form", step=True)
    fill_outcome(page, family, stamp)
    human_pause(page, "after-outcome")
    ev = page.get_by_role("textbox", name=re.compile(r"Evidence", re.I))
    if ev.count():
        demo = demo_family(family, stamp or "demo")
        note = demo.get("evidence") or f"Playwright lifecycle evidence {family} {market[:8]}"
        ev.first.fill(str(note))
        human_pause(page, "after-evidence")
    wait_submit_ready(page, family, market)
    # Re-fill immediately before submit — reloads in wait_submit_ready can reset controlled inputs.
    fill_outcome(page, family, stamp)
    human_click(page.get_by_role("button", name="Submit result", exact=True), page, "Submit result")
    # Do NOT match bare "submit_result" — resolve-desk copy contains that token even before the ix.
    wait_proposed(page, market)
    human_pause(page, "after-submit", step=True)

    _report_s, challenge_s = official_clocks()
    # Finalize after the official challenge length (Protocol PDA), plus indexer lag.
    deadline = time.time() + challenge_s + 40
    fin = page.get_by_role("button", name="Finalize", exact=True)
    while time.time() < deadline:
        try:
            res = api_json(f"/v1/markets/{market}/resolution")
            if res.get("has_final") or res.get("phase_name") == "Finalized" or int(res.get("phase") or 0) >= 3:
                break
        except Exception:
            pass
        fin = page.get_by_role("button", name="Finalize", exact=True)
        if fin.count() == 0:
            fin = page.locator("button").filter(has_text=re.compile(r"^Finalize$", re.I))
        if fin.count() and fin.first.is_enabled():
            page.wait_for_timeout(2500)
            break
        page.wait_for_timeout(1000)
        if int(time.time()) % 5 == 0:
            page.reload(wait_until="networkidle")
    if fin.count() == 0:
        # May already be finalized from a prior click / indexer catch-up.
        try:
            res = api_json(f"/v1/markets/{market}/resolution")
            if not (res.get("has_final") or int(res.get("phase") or 0) >= 3):
                raise RuntimeError("Finalize button never appeared after challenge window")
        except RuntimeError:
            raise
        except Exception as e:
            raise RuntimeError(f"Finalize button never appeared after challenge window ({e})") from e

    for _ in range(8):
        try:
            res = api_json(f"/v1/markets/{market}/resolution")
            if res.get("has_final") or int(res.get("phase") or 0) >= 3:
                break
        except Exception:
            res = {}
        fin = page.get_by_role("button", name="Finalize", exact=True)
        if fin.count() == 0:
            fin = page.locator("button").filter(has_text=re.compile(r"^Finalize$", re.I))
        if fin.count() and fin.first.is_enabled():
            human_click(fin, page, "Finalize")
            page.wait_for_timeout(2000)
        page.reload(wait_until="networkidle")
        human_pause(page, "reload-after-finalize")
        if page.get_by_role("button", name="Lock ρ").count():
            break
    else:
        # Final API check before giving up on the click loop.
        res = api_json(f"/v1/markets/{market}/resolution")
        if not (res.get("has_final") or int(res.get("phase") or 0) >= 3):
            raise RuntimeError("finalize never indexed | " + flow.note_or_err(page)[:400])

    # Lock ρ only mounts when UI rec.phase >= 3; poll API + reload until the button appears.
    lock_deadline = time.time() + 60
    lock = page.get_by_role("button", name="Lock ρ")
    while time.time() < lock_deadline:
        lock = page.get_by_role("button", name="Lock ρ")
        if lock.count() and lock.first.is_visible():
            break
        page.reload(wait_until="networkidle")
        human_pause(page, "reload-for-lock-rho")
        page.wait_for_timeout(500)
    else:
        raise RuntimeError("Lock ρ never appeared after finalize | " + flow.note_or_err(page)[:400])
    human_pause(page, "before-lock-rho", step=True)
    last_err = "lock ρ not confirmed"
    for attempt in range(4):
        if lock.count() and lock.first.is_enabled():
            human_click(lock, page, "Lock ρ")
        try:
            wait_board_phase(market, timeout_s=20)
            last_err = ""
            break
        except RuntimeError as e:
            last_err = str(e)
            page.wait_for_timeout(500)
    if last_err:
        raise RuntimeError(last_err + " | " + flow.note_or_err(page)[:400])


def wait_board_phase(market: str, timeout_s: float = 45) -> int:
    deadline = time.time() + timeout_s
    last = -1
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"{API}/v1/markets/{market}/info", timeout=5) as resp:
                body = json.loads(resp.read().decode())
            last = int(body.get("board_phase") or 0)
            if last >= 1:
                return last
        except Exception:
            pass
        time.sleep(1)
    raise RuntimeError(f"indexer board_phase stayed {last} (want >=1)")


def run_family(page, family: str, stamp: str) -> FamilyRun:
    run = FamilyRun(family=family.strip())
    family = run.family
    try:
        t0 = time.time()
        market = create_family(page, family, stamp)
        run.market = market
        record(run, "create+review", True, t0, market, shot(page, f"{family}-01-open"))
        human_pause(page, "after-create", step=True)

        t0 = time.time()
        human_goto(page, f"{BASE}/m/{market}", "market-desk")
        ensure_session(page)
        human_pause(page, "after-session", step=True)
        record(run, "open-session", True, t0, "", shot(page, f"{family}-02-session"))

        t0 = time.time()
        buy_for_family(page, family, stamp)
        human_pause(page, "after-trade", step=True)
        record(run, "trade", True, t0, "confirmed", shot(page, f"{family}-03-trade"))

        t0 = time.time()
        human_goto(page, f"{BASE}/auction/{market}", "auction-desk")
        quote_btn = page.get_by_role("button", name="Quote pool")
        if quote_btn.count() == 0:
            quote_btn = page.locator("button").filter(has_text=re.compile(r"Quote (pool|layer)", re.I))
        expect(quote_btn.first).to_be_visible(timeout=20_000)
        human_click(quote_btn, page, "Quote pool")
        try:
            flow.wait_text(page, "quoted ", timeout_ms=45_000)
        except PwTimeout:
            aside = ""
            try:
                aside = page.locator("aside").inner_text()[:480]
            except Exception:
                aside = page.locator("body").inner_text()[:480]
            shot(page, f"{family}-04-auction")
            raise RuntimeError("quote not confirmed: " + aside.replace("\n", " "))
        human_pause(page, "after-quote", step=True)
        record(run, "auction-quote", True, t0, "", shot(page, f"{family}-04-auction"))

        t0 = time.time()
        human_goto(page, f"{BASE}/m/{market}", "wait-close")
        wait_trading_closed(page, timeout_s=CLOSE_IN + 40)
        human_pause(page, "trading-closed", step=True)
        record(run, "wait-close", True, t0, "trading closed", shot(page, f"{family}-05-closed"))

        t0 = time.time()
        resolve_and_settle(page, family, market, stamp)
        human_pause(page, "after-settle", step=True)
        record(run, "resolve+settle", True, t0, "Lock ρ", shot(page, f"{family}-06-settle"))

        t0 = time.time()
        lp_detail = claim_lp(page, market)
        lp_ok = "no_quote_row" not in lp_detail and "not_settled" not in lp_detail and not lp_detail.startswith("FAIL")
        human_pause(page, "after-lp", step=True)
        record(run, "lp-claims", lp_ok, t0, lp_detail, shot(page, f"{family}-07-lp"))
        if not lp_ok:
            raise RuntimeError(f"lp-claims {lp_detail}")

        t0 = time.time()
        plat_detail = claim_platform(page, market)
        plat_ok = (
            "platform_not_indexed" not in plat_detail
            and "fees_disabled" not in plat_detail
            and "sp_disabled" not in plat_detail
            and "forbidden" not in plat_detail
            and "missing_fund_cp" not in plat_detail
            and "sp_no_note" not in plat_detail
            and "fees_no_sig" not in plat_detail
        )
        human_pause(page, "after-platform", step=True)
        record(run, "platform-claims", plat_ok, t0, plat_detail, shot(page, f"{family}-07b-platform"))
        if not plat_ok:
            raise RuntimeError(f"platform-claims {plat_detail}")

        t0 = time.time()
        human_goto(page, BASE + "/portfolio", "portfolio")
        claim = page.get_by_role("button", name=re.compile(r"Claim", re.I))
        detail = f"claim_buttons={claim.count()}"
        if claim.count():
            try:
                human_click(claim, page, "Claim")
                detail += " clicked"
            except Exception as e:
                detail += f" click_err={e}"
        record(run, "portfolio", True, t0, detail, shot(page, f"{family}-08-portfolio"))

        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        if not run.steps or run.steps[-1].name != "fatal":
            record(run, "fatal", False, time.time(), run.error, shot(page, f"{family}-fatal") if page else "")
        flow.out(f"FATAL {family}: {run.error}")
    return run


def open_and_lock(page, family: str, market: str) -> None:
    page.goto(f"{BASE}/resolve/{market}", wait_until="networkidle")
    open_btn = page.get_by_role("button", name="Open window")
    if open_btn.count() and open_btn.first.is_enabled():
        open_btn.first.click()
        flow.wait_any_text(page, ["Propose x*", "Submit result", "Lock committee bond", "Waiting for report"], timeout_ms=45_000)
    lockb = page.get_by_role("button", name="Lock committee bond")
    if lockb.count() and lockb.first.is_enabled():
        lockb.first.click()
        page.wait_for_timeout(1500)


def wait_report_deadline(page, market: str, timeout_s: int | None = None) -> None:
    report_s, _ = official_clocks()
    timeout_s = int(report_s + 50) if timeout_s is None else timeout_s
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(f"{API}/v1/markets/{market}/resolution", timeout=5) as resp:
                rec = json.loads(resp.read().decode())
            due = float(rec.get("report_deadline") or 0)
            if due and time.time() >= due and int(rec.get("phase") or 0) == 0:
                return
        except Exception:
            pass
        page.wait_for_timeout(2000)
        if int(time.time()) % 8 == 0:
            page.goto(f"{BASE}/resolve/{market}", wait_until="networkidle")
    raise RuntimeError("report_deadline not reached while still Open")


def prep_live(page, family: str, stamp: str) -> tuple[FamilyRun, str]:
    run = FamilyRun(family=family)
    t0 = time.time()
    market = create_family(page, family.split("-")[0] if "-" in family else family, stamp)
    run.market = market
    record(run, "create+review", True, t0, market, shot(page, f"{family}-01-open"))
    t0 = time.time()
    page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
    ensure_session(page)
    record(run, "open-session", True, t0, "")
    t0 = time.time()
    buy_for_family(page, family)
    record(run, "trade", True, t0, "confirmed")
    t0 = time.time()
    page.goto(f"{BASE}/auction/{market}", wait_until="networkidle")
    quote_btn = page.get_by_role("button", name="Quote pool")
    if quote_btn.count() == 0:
        quote_btn = page.locator("button").filter(has_text=re.compile(r"Quote (pool|layer)", re.I))
    expect(quote_btn.first).to_be_visible(timeout=20_000)
    quote_btn.first.click()
    flow.wait_text(page, "quoted ", timeout_ms=45_000)
    record(run, "auction-quote", True, t0, "")
    t0 = time.time()
    page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
    wait_trading_closed(page, timeout_s=CLOSE_IN + 40)
    record(run, "wait-close", True, t0, "trading closed")
    return run, market


def run_committee_void(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="CommitteeVoid")
    try:
        live, market = prep_live(page, "Skellam", stamp + "cv")
        run.market = market
        run.steps = live.steps
        t0 = time.time()
        open_and_lock(page, "Skellam", market)
        page.get_by_role("button", name="Void market").click()
        flow.wait_any_text(page, ["void_resolution", "VOID", "refund"], timeout_ms=45_000)
        record(run, "committee-void", True, t0, "VOID no slash", shot(page, "CommitteeVoid-06"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "CommitteeVoid-fatal"))
        flow.out(f"FATAL CommitteeVoid: {run.error}")
    return run


def run_admin_slash(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="AdminSlash")
    try:
        live, market = prep_live(page, "Gaussian", stamp + "ax")
        run.market = market
        run.steps = live.steps
        t0 = time.time()
        open_and_lock(page, "Gaussian", market)
        wait_report_deadline(page, market)
        fill_outcome(page, "Gaussian")
        admin = page.get_by_role("button", name="Admin submit result (slash bond)")
        expect(admin.first).to_be_visible(timeout=20_000)
        admin.first.click()
        flow.wait_any_text(page, ["admin_submit_result", "slash", "Lock ρ"], timeout_ms=45_000)
        lock = page.get_by_role("button", name="Lock ρ")
        if lock.count() and lock.first.is_enabled():
            lock.first.click()
            wait_board_phase(market, timeout_s=25)
        record(run, "admin-submit-slash", True, t0, "slash_due", shot(page, "AdminSlash-06"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "AdminSlash-fatal"))
        flow.out(f"FATAL AdminSlash: {run.error}")
    return run


def run_admin_void(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="AdminVoid")
    try:
        live, market = prep_live(page, "Bernoulli", stamp + "av")
        run.market = market
        run.steps = live.steps
        t0 = time.time()
        open_and_lock(page, "Bernoulli", market)
        wait_report_deadline(page, market)
        admin = page.get_by_role("button", name="Admin VOID (no slash)")
        expect(admin.first).to_be_visible(timeout=20_000)
        admin.first.click()
        flow.wait_any_text(page, ["admin_void_resolution", "VOID", "refund"], timeout_ms=45_000)
        record(run, "admin-void", True, t0, "no slash", shot(page, "AdminVoid-06"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "AdminVoid-fatal"))
        flow.out(f"FATAL AdminVoid: {run.error}")
    return run


def run_reject_review(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="RejectReview")
    try:
        t0 = time.time()
        title = f"Reject {stamp}"
        submit_application(page, "Bernoulli", stamp + "rj", title=title, event=f"reject event {stamp}")
        flow.wait_text(page, "after review", timeout_ms=45_000)
        record(run, "submit", True, t0, title)
        t0 = time.time()
        page.get_by_role("link", name="Review").click()
        page.wait_for_load_state("networkidle")
        page.locator("label").filter(has_text="Reason").locator("input").fill("off spec / not a market")
        page.locator("li").filter(has_text=title).get_by_role("button", name="Reject").click()
        flow.wait_any_text(page, ["rejected", "reject"], timeout_ms=20_000)
        record(run, "reject", True, t0, "application rejected", shot(page, "RejectReview-02"))
        t0 = time.time()
        page.goto(BASE + "/", wait_until="networkidle")
        cards = page.locator("article, li, a").filter(has_text=title)
        if cards.count() == 0:
            record(run, "not-on-lobby", True, t0, "title absent")
        else:
            record(run, "not-on-lobby", False, t0, "rejected title still listed")
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "RejectReview-fatal"))
        flow.out(f"FATAL RejectReview: {run.error}")
    return run


def run_duplicate_409(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="Duplicate409")
    try:
        t0 = time.time()
        title = f"Dup {stamp}"
        event = f"same event {stamp}"
        submit_application(page, "Gaussian", stamp + "d1", title=title, event=event, topic=f"dup1{stamp}"[:28])
        flow.wait_text(page, "after review", timeout_ms=45_000)
        record(run, "first-submit", True, t0, title)
        t0 = time.time()
        submit_application(page, "Gaussian", stamp + "d2", title=title, event=event, topic=f"dup2{stamp}"[:28])
        flow.wait_any_text(page, ["duplicate listing", "duplicate"], timeout_ms=20_000)
        record(run, "second-409", True, t0, "duplicate listing", shot(page, "Duplicate409-02"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "Duplicate409-fatal"))
        flow.out(f"FATAL Duplicate409: {run.error}")
    return run


def run_sells_closed(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="SellsClosed")
    try:
        t0 = time.time()
        market = create_family(page, "Bernoulli", stamp + "sc", close_in=max(CLOSE_IN, 80))
        run.market = market
        record(run, "create+review", True, t0, market)
        page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
        ensure_session(page)
        t0 = time.time()
        sells = page.get_by_role("button", name="Sells closed")
        expect(sells.first).to_be_visible(timeout=15_000)
        expect(sells.first).to_be_disabled()
        buy = page.get_by_role("button", name=re.compile(r"Buy (set|line)", re.I))
        expect(buy.first).to_be_enabled()
        record(run, "sells-gated", True, t0, "Sells closed; buy still open", shot(page, "SellsClosed-02"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "SellsClosed-fatal"))
        flow.out(f"FATAL SellsClosed: {run.error}")
    return run


def run_closed_gates(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="ClosedGates")
    try:
        t0 = time.time()
        market = create_family(page, "Dirichlet", stamp + "cg", close_in=45)
        run.market = market
        record(run, "create+review", True, t0, market)
        page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
        ensure_session(page)
        buy_for_family(page, "Dirichlet")
        page.goto(f"{BASE}/auction/{market}", wait_until="networkidle")
        quote_btn = page.get_by_role("button", name="Quote pool")
        if quote_btn.count() == 0:
            quote_btn = page.locator("button").filter(has_text=re.compile(r"Quote (pool|layer)", re.I))
        quote_btn.first.click()
        flow.wait_text(page, "quoted ", timeout_ms=45_000)
        t0 = time.time()
        page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
        wait_trading_closed(page, timeout_s=90)
        buy = page.get_by_role("button", name=re.compile(r"Trading closed", re.I))
        expect(buy.first).to_be_disabled()
        record(run, "buy-closed", True, t0, "buy disabled after close_ts")
        t0 = time.time()
        page.goto(f"{BASE}/auction/{market}", wait_until="networkidle")
        aq = page.get_by_role("button", name="Trading closed")
        expect(aq.first).to_be_visible(timeout=15_000)
        expect(aq.first).to_be_disabled()
        record(run, "auction-closed", True, t0, "new quotes stop", shot(page, "ClosedGates-03"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "ClosedGates-fatal"))
        flow.out(f"FATAL ClosedGates: {run.error}")
    return run


def run_report_open_wait(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="ReportOpenWait")
    try:
        t0 = time.time()
        market = create_family(page, "Skellam", stamp + "rw", close_in=40, report_extra_s=70)
        run.market = market
        record(run, "create+review", True, t0, market)
        page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
        ensure_session(page)
        buy_for_family(page, "Skellam")
        page.goto(f"{BASE}/auction/{market}", wait_until="networkidle")
        quote_btn = page.get_by_role("button", name="Quote pool")
        if quote_btn.count() == 0:
            quote_btn = page.locator("button").filter(has_text=re.compile(r"Quote (pool|layer)", re.I))
        quote_btn.first.click()
        flow.wait_text(page, "quoted ", timeout_ms=45_000)
        page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
        wait_trading_closed(page, timeout_s=80)
        t0 = time.time()
        page.goto(f"{BASE}/resolve/{market}", wait_until="networkidle")
        ow = page.get_by_role("button", name="Open window")
        if ow.count():
            ow.first.click()
            flow.wait_any_text(page, ["Propose x*", "Submit result", "Lock committee bond", "Waiting for report"], timeout_ms=45_000)
        lockb = page.get_by_role("button", name="Lock committee bond")
        if lockb.count() and lockb.first.is_enabled():
            lockb.first.click()
            page.wait_for_timeout(1500)
        waiting = page.get_by_role("button", name="Waiting for report open")
        submit = page.get_by_role("button", name="Submit result", exact=True)
        gated = waiting.count() > 0 or (submit.count() and not submit.first.is_enabled())
        if not gated:
            raise RuntimeError("submit enabled before delayed report_open_ts")
        record(run, "submit-gated", True, t0, "waiting for report open", shot(page, "ReportOpenWait-02"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "ReportOpenWait-fatal"))
        flow.out(f"FATAL ReportOpenWait: {run.error}")
    return run


def run_bond_lock_gate(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="BondLockGate")
    try:
        live, market = prep_live(page, "Lognormal", stamp + "bg")
        run.market = market
        run.steps = live.steps
        t0 = time.time()
        page.goto(f"{BASE}/resolve/{market}", wait_until="networkidle")
        ow = page.get_by_role("button", name="Open window")
        if ow.count():
            ow.first.click()
        flow.wait_any_text(page, ["lock_committee_bond", "Propose x*", "Submit result"], timeout_ms=45_000)
        page.wait_for_timeout(2000)
        first = page.get_by_role("button", name="Lock committee bond first")
        if first.count() and first.first.is_visible():
            raise RuntimeError("bond still unlocked after resolve_open auto-lock")
        record(run, "bond-required", True, t0, "auto-lock after resolve_open", shot(page, "BondLockGate-02"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "BondLockGate-fatal"))
        flow.out(f"FATAL BondLockGate: {run.error}")
    return run


def run_halt_void(page, stamp: str) -> FamilyRun:
    run = FamilyRun(family="HaltVoid")
    try:
        t0 = time.time()
        market = create_family(page, "Bernoulli", stamp + "hv", close_in=max(CLOSE_IN, 90), early_void=True)
        run.market = market
        record(run, "create+review", True, t0, market)
        page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
        ensure_session(page)
        t0 = time.time()
        page.goto(f"{BASE}/resolve/{market}", wait_until="networkidle")
        ow = page.get_by_role("button", name="Open window")
        if ow.count():
            ow.first.click()
            flow.wait_any_text(page, ["Halt trading", "Propose x*", "Lock committee bond"], timeout_ms=45_000)
        halt = page.get_by_role("button", name="Halt trading")
        expect(halt.first).to_be_visible(timeout=20_000)
        halt.first.click()
        flow.wait_any_text(page, ["halt", "Halted"], timeout_ms=45_000)
        record(run, "halt", True, t0, "Bernoulli early halt")
        t0 = time.time()
        page.get_by_role("button", name="Void market").click()
        flow.wait_any_text(page, ["void_resolution", "VOID", "refund"], timeout_ms=45_000)
        record(run, "void-after-halt", True, t0, "early VOID refund", shot(page, "HaltVoid-03"))
        run.ok = all(s.ok for s in run.steps)
    except Exception as e:
        run.error = str(e)[:500]
        run.ok = False
        record(run, "fatal", False, time.time(), run.error, shot(page, "HaltVoid-fatal"))
        flow.out(f"FATAL HaltVoid: {run.error}")
    return run


def merge_prior_runs() -> None:
    """When re-running a subset (e.g. Bernoulli only), keep prior family results."""
    prev_path = OUT / "report.json"
    if not prev_path.exists() or not RUNS:
        return
    try:
        prev = json.loads(prev_path.read_text(encoding="utf-8"))
    except Exception:
        return
    done = {r.family for r in RUNS}
    for row in prev.get("runs") or []:
        fam = row.get("family")
        if not fam or fam in done:
            continue
        fr = FamilyRun(family=fam, market=row.get("market") or "", ok=bool(row.get("ok")), error=row.get("error") or "")
        for s in row.get("steps") or []:
            fr.steps.append(
                Step(
                    s.get("name", ""),
                    bool(s.get("ok")),
                    int(s.get("elapsed_ms") or 0),
                    s.get("detail") or "",
                    s.get("shot") or "",
                )
            )
        RUNS.append(fr)
    # Stable family order
    order = [
        "Skellam",
        "Gaussian",
        "Lognormal",
        "Dirichlet",
        "Bernoulli",
        "RejectReview",
        "Duplicate409",
        "SellsClosed",
        "ClosedGates",
        "ReportOpenWait",
        "BondLockGate",
        "HaltVoid",
        "CommitteeVoid",
        "AdminSlash",
        "AdminVoid",
    ]
    RUNS.sort(key=lambda r: order.index(r.family) if r.family in order else 99)


def write_report(stack: dict) -> Path:
    OUT.mkdir(parents=True, exist_ok=True)
    merge_prior_runs()
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    passed = sum(1 for r in RUNS if r.ok)
    failed = sum(1 for r in RUNS if not r.ok)
    steps_ok = sum(1 for r in RUNS for s in r.steps if s.ok)
    steps_bad = sum(1 for r in RUNS for s in r.steps if not s.ok)
    verdict = "PASS" if failed == 0 and passed == len(RUNS) and passed > 0 else "FAIL"
    lines = [
        "# Playwright full-lifecycle report",
        "",
        f"- Generated: `{now}`",
        f"- Web: `{BASE}` · API: `{API}` · Gateway: `{GW}` · RPC: `{RPC}`",
        f"- Locale: **en-US** (English canonical listings)",
        f"- close_in: **{CLOSE_IN}s** (report window / challenge length from official Protocol PDA, not the application form)",
        f"- Families: **{passed} passed** / **{failed} failed** / {len(RUNS)} total",
        f"- Steps: **{steps_ok}** ok / **{steps_bad}** failed",
        f"- Verdict: **{verdict}**",
        "",
        "## Stack",
        "",
        "```json",
        json.dumps(stack, indent=2)[:1200],
        "```",
        "",
        "## Flow covered (each family)",
        "",
        "1. Create application (English title/event/description; `report_open_ts` = `close_ts`; `committee_bond=100`)",
        "2. Review → Approve and open prediction market",
        "3. Open SessionToken",
        "4. Trade (Buy set / Buy line)",
        "5. Risk auction → Quote pool",
        "6. Wait trading close",
        "7. Committee resolve: Open window → lock bond → wait report_open → Submit result → Finalize",
        "8. Settlement: Lock ρ (begin_settle)",
        "9. /lp: Draw H, Premium, Surplus, Fund cover (20% S_C); Claim cover is platform-signed; vault shows Π and cover paid",
        "10. /create: Claim fees (φ) + Claim S_P as Market.platform; Fund C_P is a separate fund_pool",
        "11. Portfolio claim surface",
        "12. Branch RejectReview: reviewer Reject with reason — not listed on lobby",
        "13. Branch Duplicate409: same title+event+family → `duplicate listing`",
        "14. Branch SellsClosed: sell button stays disabled while buys are open",
        "15. Branch ClosedGates: after close_ts, buy and new risk quotes show Trading closed",
        "16. Branch ReportOpenWait: delayed report_open_ts keeps Submit disabled",
        "17. Branch BondLockGate: resolve_open auto-locks committee bond (submit stays gated until then)",
        "18. Branch HaltVoid: Bernoulli early occurrence → Halt trading → committee VOID refund",
        "19. Branch CommitteeVoid: after close, lock bond, committee VOID (no slash)",
        "20. Branch AdminSlash: miss report window, platform `admin_submit_result` + slash holder",
        "21. Branch AdminVoid: miss report window, platform `admin_void_resolution` (return bond)",
        "",
        "## Results by family",
        "",
    ]
    for r in RUNS:
        lines.append(f"### {r.family} — {'PASS' if r.ok else 'FAIL'}")
        lines.append("")
        if r.market:
            lines.append(f"- Market: `{r.market}`")
        if r.error:
            lines.append(f"- Error: `{r.error[:240]}`")
        lines.append("")
        lines.append("| Status | Step | ms | Detail | Shot |")
        lines.append("| --- | --- | ---: | --- | --- |")
        for s in r.steps:
            det = s.detail.replace("|", "\\|").replace("\n", " ")[:140]
            shot_name = Path(s.shot).name if s.shot else ""
            lines.append(f"| {'✅' if s.ok else '❌'} | {s.name} | {s.elapsed_ms} | {det} | `{shot_name}` |")
        lines.append("")
    lines += [f"Artifacts: `{OUT}`", ""]
    path = OUT / "report.md"
    path.write_text("\n".join(lines), encoding="utf-8")
    (OUT / "report.json").write_text(
        json.dumps(
            {
                "generated": now,
                "verdict": verdict,
                "close_in": CLOSE_IN,
                "passed": passed,
                "failed": failed,
                "stack": stack,
                "runs": [
                    {
                        "family": r.family,
                        "market": r.market,
                        "ok": r.ok,
                        "error": r.error,
                        "steps": [asdict(s) for s in r.steps],
                    }
                    for r in RUNS
                ],
            },
            indent=2,
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )
    return path


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    flow.OUT = OUT / "flow-misc"
    flow.OUT.mkdir(parents=True, exist_ok=True)

    stack = stack_ok()
    if not (isinstance(stack.get("api"), dict) and stack["api"].get("ok") and stack.get("web") == 200):
        path = write_report(stack)
        print("STACK_FAIL", path)
        return 1
    if isinstance(stack.get("pool"), dict) and stack["pool"].get("error"):
        path = write_report(stack)
        print("POOL_CLOCK_FAIL", path)
        return 1
    if not stack.get("gateway"):
        flow.out("WARN gateway /v1/health not ok — trades may fail")

    families = [f.strip() for f in FAMILIES if f.strip()]
    stamp = str(int(time.time()) % 10_000_000)
    # Full five-family run is a fresh report (do not keep last week's PASS rows).
    if len(families) >= 5:
        for p in (OUT / "report.json", OUT / "report.md"):
            if p.exists():
                p.unlink()

    try:
        kp = flow.fund_chain()
    except Exception as e:
        flow.out(f"fund_chain failed: {e}")
        path = write_report(stack)
        print("FUND_FAIL", path)
        return 1
    proto = ensure_official_protocol(kp)
    flow.out(f"official protocol: {proto}")
    secret = list(bytes(kp))

    video_path = None
    with sync_playwright() as p:
        launch_kw: dict = {"headless": not HEADED}
        if SLOW_MO_MS > 0:
            launch_kw["slow_mo"] = SLOW_MO_MS
        browser = p.chromium.launch(**launch_kw)
        ctx_kw: dict = {
            "locale": "en-US",
            "extra_http_headers": {"Accept-Language": "en-US,en;q=0.9"},
            "viewport": {"width": 1500, "height": 960},
        }
        if RECORD_VIDEO:
            VIDEO_DIR.mkdir(parents=True, exist_ok=True)
            ctx_kw["record_video_dir"] = str(VIDEO_DIR)
            ctx_kw["record_video_size"] = {"width": 1500, "height": 960}
            flow.out(f"recording video → {VIDEO_DIR}")
        ctx = browser.new_context(**ctx_kw)
        page = ctx.new_page()
        flow.connect_wallet(page, secret)
        human_pause(page, "wallet-ready", step=True)
        human_click(page.get_by_role("link", name="Portfolio"), page, "Portfolio")
        page.wait_for_load_state("networkidle")
        human_pause(page, "portfolio", step=True)
        demo_root = load_demo_data()
        deposit_amt = str(demo_root.get("deposit_usdc") or 50000)
        fund_cp_amt = str(demo_root.get("fund_cp_usdc") or 10)
        if USE_DEMO_DATA and demo_root:
            flow.out(f"demo data ← {DEMO_DATA_PATH}")
        page.locator("input[type='number']").first.fill(deposit_amt)
        human_pause(page, "after-fill-deposit")
        human_click(page.get_by_role("button", name="Deposit", exact=True), page, "Deposit")
        try:
            flow.wait_text(page, "deposit", timeout_ms=45_000)
        except PwTimeout:
            flow.out("deposit note timeout — continuing")
        human_pause(page, "after-deposit", step=True)
        shot(page, "00-deposit")

        human_goto(page, BASE + "/create", "create-desk")
        # Separate C_P fund_pool — never mixed with Claim fees / Claim S_P.
        fund = page.get_by_role("button", name="Fund C_P")
        if fund.count():
            inp = fund.locator("xpath=preceding-sibling::input[1]")
            if inp.count():
                inp.fill(fund_cp_amt)
                human_pause(page, "after-fill-fund-cp")
            human_click(fund, page, "Fund C_P")
            try:
                flow.wait_any_text(page, ["fund_pool", "init_pool", "already"], timeout_ms=45_000)
            except PwTimeout:
                flow.out("init/fund pool timeout — continuing")
        human_pause(page, "after-pool", step=True)
        shot(page, "00-pool")

        for fam in families:
            # Fresh stamp slice so topics stay unique under retries
            RUNS.append(run_family(page, fam, f"{stamp}{fam[:1].lower()}"))

        extra = os.environ.get("LIFECYCLE_BRANCHES", "1") != "0"
        if extra:
            RUNS.append(run_reject_review(page, stamp))
            RUNS.append(run_duplicate_409(page, stamp))
            RUNS.append(run_sells_closed(page, stamp))
            RUNS.append(run_closed_gates(page, stamp))
            RUNS.append(run_report_open_wait(page, stamp))
            RUNS.append(run_bond_lock_gate(page, stamp))
            RUNS.append(run_halt_void(page, stamp))
            RUNS.append(run_committee_void(page, stamp))
            RUNS.append(run_admin_slash(page, stamp))
            RUNS.append(run_admin_void(page, stamp))

        if RECORD_VIDEO:
            try:
                video_path = page.video.path() if page.video else None
            except Exception:
                video_path = None
        ctx.close()
        browser.close()

    if RECORD_VIDEO:
        # After context close, Playwright finalizes the webm; rename to a stable name.
        fams = "-".join(f.strip() for f in families if f.strip()) or "lifecycle"
        dest = VIDEO_DIR / f"lifecycle-{fams}-{stamp}.webm"
        src = None
        if video_path and Path(video_path).exists():
            src = Path(video_path)
        else:
            webs = sorted(VIDEO_DIR.glob("*.webm"), key=lambda p: p.stat().st_mtime, reverse=True)
            src = webs[0] if webs else None
        if src is not None:
            if dest.exists():
                dest.unlink()
            src.replace(dest)
            print("VIDEO", dest)
        else:
            print("VIDEO_MISSING", VIDEO_DIR)

    path = write_report(stack)
    print("REPORT", path)
    verdict = "PASS" if RUNS and all(r.ok for r in RUNS) else "FAIL"
    print("VERDICT", verdict)
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
