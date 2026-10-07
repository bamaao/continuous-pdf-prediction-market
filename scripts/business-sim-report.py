#!/usr/bin/env python3
"""Full localnet business simulation: fund → SIWS → deposit → review-open → multi buys → i18n → report.

Requires: L1 :8899, market-api :8080, web :3000, PG (api health pg=true).
"""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from playwright.sync_api import TimeoutError as PwTimeout
from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[1]
BASE = os.environ.get("WEB_URL", "http://127.0.0.1:3000")
API = os.environ.get("MARKET_API", "http://127.0.0.1:8080")
RPC = os.environ.get("RPC_URL", "http://127.0.0.1:8899")
OUT = ROOT / "tmp" / "business-sim"
N_BUYS = int(os.environ.get("SIM_BUYS", "5"))

spec = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(flow)


@dataclass
class Step:
    name: str
    ok: bool
    detail: str = ""
    elapsed_ms: int = 0
    artifacts: list[str] = field(default_factory=list)


STEPS: list[Step] = []
MARKETS: dict[str, str] = {}
TRADES: list[dict] = []


def http_json(method: str, url: str, body=None, headers=None, timeout=30):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"content-type": "application/json", **(headers or {})},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            return r.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw) if raw else {"error": str(e)}
        except json.JSONDecodeError:
            return e.code, {"error": raw.decode("utf-8", "replace")}
    except Exception as e:
        return 0, {"error": str(e)}


def record(name: str, ok: bool, detail: str = "", t0: float | None = None, artifacts: list[str] | None = None):
    ms = int((time.time() - t0) * 1000) if t0 is not None else 0
    STEPS.append(Step(name=name, ok=ok, detail=detail, elapsed_ms=ms, artifacts=artifacts or []))
    tag = "PASS" if ok else "FAIL"
    flow.out(f"[{tag}] {name}" + (f" — {detail}" if detail else "") + (f" ({ms}ms)" if ms else ""))


def check_stack() -> bool:
    t0 = time.time()
    ok_all = True
    st, health = http_json("GET", f"{API}/v1/health")
    api_ok = st == 200 and health.get("ok") is True
    record("market-api health", api_ok, json.dumps(health, ensure_ascii=False)[:200], t0)
    ok_all &= api_ok
    st, _ = http_json("POST", RPC, {"jsonrpc": "2.0", "id": 1, "method": "getHealth"})
    # solana returns result in body; urllib may get 200
    rpc_ok = st in (200, 0) or True
    try:
        with urllib.request.urlopen(
            urllib.request.Request(
                RPC,
                data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getHealth"}).encode(),
                headers={"content-type": "application/json"},
                method="POST",
            ),
            timeout=5,
        ) as r:
            rpc_ok = r.status == 200
            slot_body = json.loads(r.read().decode())
    except Exception as e:
        rpc_ok = False
        slot_body = {"error": str(e)}
    record("L1 RPC health", rpc_ok, json.dumps(slot_body, ensure_ascii=False)[:160])
    ok_all &= rpc_ok
    try:
        with urllib.request.urlopen(BASE + "/", timeout=8) as r:
            web_ok = r.status == 200
    except Exception as e:
        web_ok = False
        record("web :3000", False, str(e))
        return False
    record("web :3000", web_ok)
    ok_all &= web_ok
    return ok_all


def wait_text(page, needle: str, timeout_ms=45_000) -> None:
    page.wait_for_function(
        """(n) => document.body && document.body.innerText.toLowerCase().includes(n.toLowerCase())""",
        arg=needle,
        timeout=timeout_ms,
    )


def buy_cells(page, market: str, cell_a: int, cell_b: int, shares: int, label: str) -> bool:
    t0 = time.time()
    page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
    # shares spin if present
    spins = page.locator("input[type='number']")
    if spins.count():
        try:
            spins.first.fill(str(shares))
        except Exception:
            pass
    bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
    for _ in range(24):
        if bars.count() >= max(cell_a, cell_b) + 1:
            break
        page.wait_for_timeout(400)
        page.reload(wait_until="networkidle")
        bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
    if bars.count() < max(cell_a, cell_b) + 1:
        record(f"buy {label}", False, f"not enough cells ({bars.count()})", t0)
        return False
    # clear selection by reload then pick
    bars.nth(cell_a).click()
    if cell_b != cell_a:
        bars.nth(cell_b).click()
    page.get_by_role("button", name="Buy set").click()
    try:
        wait_text(page, "confirmed", timeout_ms=45_000)
        TRADES.append({"market": market, "label": label, "cells": [cell_a, cell_b], "shares": shares, "ok": True})
        shot = OUT / f"buy-{label}.png"
        page.screenshot(path=str(shot), full_page=True)
        record(f"buy {label}", True, f"cells {cell_a},{cell_b} shares={shares}", t0, [str(shot)])
        return True
    except PwTimeout:
        detail = flow.note_or_err(page)
        TRADES.append({"market": market, "label": label, "cells": [cell_a, cell_b], "shares": shares, "ok": False, "detail": detail})
        record(f"buy {label}", False, detail[:240], t0)
        return False


def buy_skellam_home(page, market: str, label: str) -> bool:
    t0 = time.time()
    page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
    for _ in range(20):
        if page.get_by_role("button", name="Home", exact=True).count() or page.get_by_role("button", name="1X2 Home").count():
            break
        page.wait_for_timeout(400)
        page.reload(wait_until="networkidle")
    btn_line = page.get_by_role("button", name="1X2 Home")
    btn_home = page.get_by_role("button", name="Home", exact=True)
    if btn_line.count():
        btn_line.click()
    elif btn_home.count():
        btn_home.click()
    else:
        record(f"buy {label}", False, "no Home / 1X2 Home", t0)
        return False
    page.wait_for_timeout(600)
    buy = page.get_by_role("button", name="Buy line")
    if buy.count() == 0:
        buy = page.get_by_role("button", name="Buy set")
    if buy.count() == 0:
        record(f"buy {label}", False, "Buy line/set missing", t0)
        return False
    buy.first.click()
    try:
        wait_text(page, "confirmed", timeout_ms=45_000)
        TRADES.append({"market": market, "label": label, "line": "home", "ok": True})
        shot = OUT / f"buy-{label}.png"
        page.screenshot(path=str(shot), full_page=True)
        record(f"buy {label}", True, "skellam home", t0, [str(shot)])
        return True
    except PwTimeout:
        detail = flow.note_or_err(page)
        TRADES.append({"market": market, "label": label, "ok": False, "detail": detail})
        record(f"buy {label}", False, detail[:240], t0)
        return False


def seed_i18n(market: str) -> None:
    t0 = time.time()
    st, cur = http_json("GET", f"{API}/v1/listings/{market}?locale=en")
    title = "US CPI YoY — sim"
    event = "US CPI YoY first print"
    description = "First official print. Revisions do not settle. Business sim seed."
    tags = ["macro", "sim"]
    if st == 200:
        title = cur.get("title_en") or cur.get("title") or title
        event = cur.get("event_en") or cur.get("event") or event
        description = cur.get("description_en") or cur.get("description") or description
        tags = cur.get("tags") or tags
    st, body = http_json(
        "POST",
        f"{API}/v1/listings",
        {
            "market": market,
            "title": title,
            "tags": tags,
            "event": event,
            "description": description,
            "topic": cur.get("topic") if st == 200 else "",
            "tag": cur.get("tag") if st == 200 else "",
            "source_locale": "en",
            "i18n": {
                "zh-Hans": {
                    "title": "美国 CPI 同比 — 模拟",
                    "event": "美国 CPI 首次官方打印",
                    "description": "首次官方打印。修订不结算。业务模拟种子。",
                }
            },
        },
    )
    record("seed listing i18n", st == 200, f"status={st} locked={body.get('canonical_locked')}", t0)


def run_i18n_playwright() -> None:
    t0 = time.time()
    env = os.environ.copy()
    env["PYTHONIOENCODING"] = "utf-8"
    env["WEB_URL"] = BASE
    env["MARKET_API"] = API
    p = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "listing-i18n-playwright.py")],
        cwd=str(ROOT),
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    out = (p.stdout or "") + (p.stderr or "")
    record("playwright listing-i18n", p.returncode == 0, out.strip()[-400:], t0)


def write_report() -> Path:
    OUT.mkdir(parents=True, exist_ok=True)
    passed = sum(1 for s in STEPS if s.ok)
    failed = sum(1 for s in STEPS if not s.ok)
    trades_ok = sum(1 for t in TRADES if t.get("ok"))
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    md = []
    md.append("# Business simulation report")
    md.append("")
    md.append(f"- Generated: `{now}`")
    md.append(f"- Web: `{BASE}` · API: `{API}` · RPC: `{RPC}`")
    md.append(f"- Steps: **{passed} passed** / **{failed} failed** / {len(STEPS)} total")
    md.append(f"- Trades: **{trades_ok}** ok / {len(TRADES)} attempted (target multi-buy ≥ {N_BUYS})")
    md.append(f"- Verdict: **{'PASS' if failed == 0 and trades_ok >= 3 else 'FAIL'}**")
    md.append("")
    md.append("## Markets")
    md.append("")
    if MARKETS:
        for k, v in MARKETS.items():
            md.append(f"- **{k}**: `{v}`")
    else:
        md.append("- _(none)_")
    md.append("")
    md.append("## Steps")
    md.append("")
    md.append("| Status | Step | ms | Detail |")
    md.append("| --- | --- | ---: | --- |")
    for s in STEPS:
        det = s.detail.replace("|", "\\|").replace("\n", " ")[:180]
        md.append(f"| {'✅' if s.ok else '❌'} | {s.name} | {s.elapsed_ms} | {det} |")
    md.append("")
    md.append("## Trades")
    md.append("")
    md.append("```json")
    md.append(json.dumps(TRADES, indent=2, ensure_ascii=False))
    md.append("```")
    md.append("")
    md.append("## Artifacts")
    md.append("")
    for s in STEPS:
        for a in s.artifacts:
            md.append(f"- `{a}`")
    md.append("")
    md.append("## Business path covered")
    md.append("")
    md.append("1. Stack health (L1 + market-api + web)")
    md.append("2. Fund localnet wallet (SOL airdrop + USDC mint + vault/committee)")
    md.append("3. SIWS + deposit")
    md.append("4. Create Gaussian → review approve/open")
    md.append("5. Open SessionToken + multiple Buy set fills")
    md.append("6. Create Skellam → review open + Buy line")
    md.append("7. Risk auction quote")
    md.append("8. Listing i18n seed + Playwright Accept-Language / Show English")
    md.append("")
    path = OUT / "report.md"
    path.write_text("\n".join(md), encoding="utf-8")
    (OUT / "report.json").write_text(
        json.dumps(
            {
                "generated": now,
                "passed": passed,
                "failed": failed,
                "trades_ok": trades_ok,
                "markets": MARKETS,
                "steps": [asdict(s) for s in STEPS],
                "trades": TRADES,
                "verdict": "PASS" if failed == 0 and trades_ok >= 3 else "FAIL",
            },
            indent=2,
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )
    return path


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    flow.OUT = OUT
    if not check_stack():
        write_report()
        flow.out("STACK_FAIL")
        return 1

    t0 = time.time()
    try:
        kp = flow.fund_chain()
        record("fund_chain", True, str(kp.pubkey()), t0)
    except Exception as e:
        record("fund_chain", False, str(e), t0)
        write_report()
        return 1

    stamp = str(int(time.time()) % 10_000_000)
    secret = list(bytes(kp))
    failed_hard = False

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": 1500, "height": 960})
        t0 = time.time()
        try:
            flow.connect_wallet(page, secret)
            record("SIWS connect", True, "", t0)
            page.screenshot(path=str(OUT / "01-siws.png"), full_page=True)
        except Exception as e:
            record("SIWS connect", False, str(e), t0)
            browser.close()
            write_report()
            return 1

        # Deposit
        t0 = time.time()
        page.get_by_role("link", name="Portfolio").click()
        page.wait_for_load_state("networkidle")
        page.locator("input[type='number']").first.fill("2000")
        page.get_by_role("button", name="Deposit", exact=True).click()
        try:
            wait_text(page, "deposit", timeout_ms=45_000)
            record("deposit", True, "2000 USDC", t0, [str(OUT / "02-deposit.png")])
            page.screenshot(path=str(OUT / "02-deposit.png"), full_page=True)
        except PwTimeout:
            record("deposit", False, flow.note_or_err(page)[:240], t0)
            failed_hard = True

        # Create Gaussian + native i18n fields if present
        t0 = time.time()
        g_market = None
        try:
            page.goto(BASE + "/create", wait_until="networkidle")
            page.get_by_role("button", name="Gaussian", exact=False).first.click()
            page.get_by_label("Market title", exact=True).fill(f"Sim Gaussian {stamp}")
            page.get_by_label("Tags", exact=True).fill("macro, sim")
            page.get_by_label("Trading event", exact=True).fill(f"sim CPI print {stamp}")
            page.get_by_role("textbox", name="Description", exact=True).fill(
                "Business sim: first official print. Revisions do not settle. Extra time does not count."
            )
            page.get_by_label("Topic / series (on-chain id)", exact=True).fill(f"simg{stamp}")
            page.get_by_label("Tag / release", exact=True).fill("sim")
            if page.get_by_label("n_grid / atoms", exact=True).count():
                page.get_by_label("n_grid / atoms", exact=True).select_option("8")
            if page.get_by_label("Or seconds from now", exact=True).count():
                page.get_by_label("Or seconds from now", exact=True).fill("7200")
            # English-only create path (Native locale left empty); zh covered by listing-i18n suite.
            page.get_by_role("button", name="Submit for review").click()
            wait_text(page, "after review", timeout_ms=45_000)
            page.get_by_role("link", name="Review").click()
            page.wait_for_load_state("networkidle")
            page.locator("li").filter(has_text=f"Sim Gaussian {stamp}").get_by_role(
                "button", name="Approve and open prediction market"
            ).click()
            link = page.get_by_role("link", name="Open prediction market", exact=True)
            expect(link).to_be_visible(timeout=90_000)
            href = link.get_attribute("href") or ""
            g_market = href.split("/m/")[-1]
            MARKETS["gaussian"] = g_market
            record("create+review gaussian", True, g_market, t0, [str(OUT / "03-gaussian.png")])
            page.screenshot(path=str(OUT / "03-gaussian.png"), full_page=True)
        except Exception as e:
            record("create+review gaussian", False, f"{e} | {flow.note_or_err(page)[:200]}", t0)
            failed_hard = True

        if g_market:
            t0 = time.time()
            page.goto(f"{BASE}/m/{g_market}", wait_until="networkidle")
            page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
            if "session live" not in page.locator("body").inner_text().lower():
                page.get_by_role("button", name="Open session").click()
                try:
                    wait_text(page, "session live", timeout_ms=45_000)
                    record("open session", True, "", t0)
                except PwTimeout:
                    record("open session", False, flow.note_or_err(page)[:200], t0)
                    failed_hard = True
            else:
                record("open session", True, "already live", t0)

            # Multi buys on gaussian
            plans = [
                (0, 1, 1, "g1"),
                (1, 2, 1, "g2"),
                (2, 3, 2, "g3"),
                (0, 2, 1, "g4"),
                (3, 4, 1, "g5"),
            ][: max(3, N_BUYS)]
            for a, b, sh, lab in plans:
                buy_cells(page, g_market, a, b, sh, lab)

            seed_i18n(g_market)

            # Auction
            t0 = time.time()
            page.goto(f"{BASE}/auction/{g_market}", wait_until="networkidle")
            if page.get_by_role("button", name="Quote pool").count():
                page.get_by_role("button", name="Quote pool").click()
                try:
                    wait_text(page, "quoted", timeout_ms=45_000)
                    record("risk_quote", True, "", t0, [str(OUT / "04-auction.png")])
                    page.screenshot(path=str(OUT / "04-auction.png"), full_page=True)
                except PwTimeout:
                    record("risk_quote", False, flow.note_or_err(page)[:200], t0)
            else:
                record("risk_quote", False, "Quote pool missing", t0)

        # Skellam market + buys
        t0 = time.time()
        s_market = None
        try:
            page.goto(BASE + "/create", wait_until="networkidle")
            page.get_by_role("button", name="Skellam").first.click()
            page.get_by_label("Market title", exact=True).fill(f"Sim Skellam {stamp}")
            page.get_by_label("Tags", exact=True).fill("football, sim")
            page.get_by_label("Trading event", exact=True).fill(f"sim match {stamp}")
            page.get_by_role("textbox", name="Description", exact=True).fill(
                "Business sim Skellam: regulation full-time score. Extra time does not count."
            )
            page.get_by_label("Topic / series (on-chain id)", exact=True).fill(f"sims{stamp}")
            page.get_by_label("Tag / release", exact=True).fill("sim")
            if page.get_by_label("Or seconds from now", exact=True).count():
                page.get_by_label("Or seconds from now", exact=True).fill("7200")
            page.get_by_role("button", name="Submit for review").click()
            wait_text(page, "after review", timeout_ms=45_000)
            page.get_by_role("link", name="Review").click()
            page.wait_for_load_state("networkidle")
            page.locator("li").filter(has_text=f"Sim Skellam {stamp}").get_by_role(
                "button", name="Approve and open prediction market"
            ).click()
            link = page.get_by_role("link", name="Open prediction market", exact=True)
            expect(link).to_be_visible(timeout=90_000)
            href = link.get_attribute("href") or ""
            s_market = href.split("/m/")[-1]
            MARKETS["skellam"] = s_market
            record("create+review skellam", True, s_market, t0)
        except Exception as e:
            record("create+review skellam", False, f"{e} | {flow.note_or_err(page)[:200]}", t0)

        if s_market:
            buy_skellam_home(page, s_market, "s1")
            buy_skellam_home(page, s_market, "s2")

        # Portfolio snapshot
        page.goto(BASE + "/portfolio", wait_until="networkidle")
        page.screenshot(path=str(OUT / "05-portfolio.png"), full_page=True)
        record("portfolio snapshot", True, "", artifacts=[str(OUT / "05-portfolio.png")])

        browser.close()

    run_i18n_playwright()
    report = write_report()
    flow.out(f"REPORT {report}")
    trades_ok = sum(1 for t in TRADES if t.get("ok"))
    fails = sum(1 for s in STEPS if not s.ok)
    if fails or trades_ok < 3 or failed_hard:
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        record("fatal", False, str(e))
        write_report()
        flow.out("FATAL " + str(e))
        raise
