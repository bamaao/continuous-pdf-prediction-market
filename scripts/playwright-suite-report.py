#!/usr/bin/env python3
"""Run Playwright business + UI suites on localnet and write a combined test report.

Suites:
  1) phase6-flow-playwright.py  — SIWS → deposit → review-open Gaussian/Skellam → buy → auction
  2) business-multi-buy.py      — additional SessionToken buys on the opened Gaussian (inline)
  3) listing-i18n-playwright.py — Accept-Language + Show English
  4) phase6-playwright.py       — lobby/UI smoke (best-effort)
  5) wallet-shell-playwright.py — mobile wallet shell (best-effort)

Outputs: tmp/playwright-suite/report.md + report.json
"""

from __future__ import annotations

import importlib.util
import json
import os
import re
import subprocess
import sys
import time
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
OUT = ROOT / "tmp" / "playwright-suite"
N_BUYS = int(os.environ.get("SIM_BUYS", "5"))

spec = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(flow)


@dataclass
class SuiteResult:
    name: str
    ok: bool
    elapsed_ms: int
    detail: str = ""
    log_path: str = ""


RESULTS: list[SuiteResult] = []
EXTRA: dict = {"markets": {}, "trades": [], "steps": []}


def fill_label(page, name: str, value: str, *, textarea: bool = False) -> None:
    """Prefer role=textbox with exact name — get_by_label('Description', exact) is broken vs Native description."""
    if textarea or name in ("Description", "Native description"):
        page.get_by_role("textbox", name=name, exact=True).fill(value)
        return
    page.get_by_label(name, exact=True).fill(value)


def run_cmd(name: str, args: list[str], timeout_s: int = 600) -> SuiteResult:
    OUT.mkdir(parents=True, exist_ok=True)
    log = OUT / f"{name}.log"
    t0 = time.time()
    env = os.environ.copy()
    env["PYTHONIOENCODING"] = "utf-8"
    env["WEB_URL"] = BASE
    env["MARKET_API"] = API
    env["RPC_URL"] = RPC
    try:
        p = subprocess.run(
            args,
            cwd=str(ROOT),
            env=env,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_s,
        )
        text = (p.stdout or "") + ("\n" + p.stderr if p.stderr else "")
        log.write_text(text, encoding="utf-8")
        ok = p.returncode == 0
        detail = text.strip().splitlines()[-1] if text.strip() else f"exit {p.returncode}"
        return SuiteResult(name, ok, int((time.time() - t0) * 1000), detail[:500], str(log))
    except subprocess.TimeoutExpired as e:
        text = (e.stdout or "") + "\n" + (e.stderr or "")
        log.write_text(text + "\nTIMEOUT\n", encoding="utf-8")
        return SuiteResult(name, False, int((time.time() - t0) * 1000), "timeout", str(log))
    except Exception as e:
        log.write_text(str(e), encoding="utf-8")
        return SuiteResult(name, False, int((time.time() - t0) * 1000), str(e), str(log))


def health() -> bool:
    ok = True
    try:
        with urllib.request.urlopen(API + "/v1/health", timeout=5) as r:
            body = json.loads(r.read().decode())
            ok = bool(body.get("ok"))
            EXTRA["api_health"] = body
    except Exception as e:
        EXTRA["api_health"] = {"error": str(e)}
        ok = False
    try:
        with urllib.request.urlopen(BASE + "/", timeout=8) as r:
            EXTRA["web"] = r.status
            ok = ok and r.status == 200
    except Exception as e:
        EXTRA["web"] = str(e)
        ok = False
    return ok


def parse_flow_markets(log_text: str) -> None:
    for line in log_text.splitlines():
        if line.startswith("review opened skellam "):
            mid = line.strip().split()[-1]
            if len(mid) >= 32:
                EXTRA["markets"]["skellam"] = mid
        elif line.startswith("review opened "):
            mid = line.strip().split()[-1]
            if len(mid) >= 32:
                EXTRA["markets"]["gaussian"] = mid


def multi_buy_gaussian() -> SuiteResult:
    """Extra Playwright buys on the Gaussian market opened by phase6-flow / inline."""
    t0 = time.time()
    OUT.mkdir(parents=True, exist_ok=True)
    existing = [t for t in (EXTRA.get("trades") or []) if t.get("ok")]
    if len(existing) >= 3:
        return SuiteResult(
            "multi-buy-gaussian",
            True,
            0,
            f"skipped — inline already has {len(existing)} ok trades",
        )
    market = EXTRA["markets"].get("gaussian") or os.environ.get("PHASE6_MARKET", "").strip()
    if not market:
        try:
            with urllib.request.urlopen(API + "/v1/markets?limit=20&q=Suite%20Gaussian", timeout=8) as r:
                page = json.loads(r.read().decode())
            items = page.get("items") or []
            if items:
                market = items[0]["market"]
                EXTRA["markets"]["gaussian"] = market
        except Exception:
            pass
    if not market:
        return SuiteResult("multi-buy-gaussian", False, 0, "no gaussian market")

    kp_path = ROOT / "tmp" / "phase6-id.json"
    if not kp_path.exists():
        return SuiteResult("multi-buy-gaussian", False, 0, "missing tmp/phase6-id.json")
    kp = flow.load_kp(kp_path)
    secret = list(bytes(kp))
    trades: list[dict] = []
    failed = 0

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            ctx = browser.new_context(
                locale="en-US",
                extra_http_headers={"Accept-Language": "en-US,en;q=0.9"},
                viewport={"width": 1500, "height": 960},
            )
            page = ctx.new_page()
            flow.connect_wallet(page, secret)
            page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
            body = page.locator("body").inner_text().lower()
            # Match case-insensitively; wallet chrome uses CSS uppercase (CAP / SESSION LIVE).
            if "session live" not in body and "cap " not in body:
                open_btn = page.get_by_role("button", name="Open session")
                if open_btn.count() == 0:
                    return SuiteResult(
                        "multi-buy-gaussian",
                        False,
                        int((time.time() - t0) * 1000),
                        f"no Open session button | {flow.note_or_err(page)[:160]}",
                    )
                open_btn.click()
                try:
                    flow.wait_session_live(page, timeout_ms=60_000)
                except PwTimeout:
                    return SuiteResult(
                        "multi-buy-gaussian",
                        False,
                        int((time.time() - t0) * 1000),
                        f"session open timeout | {flow.note_or_err(page)[:200]}",
                    )

            plans = [(0, 1, 1), (1, 2, 1), (2, 3, 2), (0, 3, 1), (4, 5, 1)][: max(3, N_BUYS)]
            for i, (a, b, sh) in enumerate(plans, 1):
                page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
                spins = page.locator("input[type='number']")
                if spins.count():
                    try:
                        spins.first.fill(str(sh))
                    except Exception:
                        pass
                bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
                for _ in range(20):
                    if bars.count() > max(a, b):
                        break
                    page.wait_for_timeout(400)
                    page.reload(wait_until="networkidle")
                    bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
                if bars.count() <= max(a, b):
                    trades.append({"i": i, "ok": False, "detail": f"cells={bars.count()}"})
                    failed += 1
                    continue
                bars.nth(a).click()
                if b != a:
                    bars.nth(b).click()
                page.get_by_role("button", name="Buy set").click()
                try:
                    flow.wait_text(page, "confirmed", timeout_ms=45_000)
                    shot = OUT / f"multi-buy-{i}.png"
                    page.screenshot(path=str(shot), full_page=True)
                    trades.append({"i": i, "ok": True, "cells": [a, b], "shares": sh, "shot": str(shot)})
                except PwTimeout:
                    trades.append({"i": i, "ok": False, "detail": flow.note_or_err(page)[:200]})
                    failed += 1
            ctx.close()
            browser.close()
    except Exception as e:
        EXTRA["trades"] = (EXTRA.get("trades") or []) + trades
        return SuiteResult("multi-buy-gaussian", False, int((time.time() - t0) * 1000), str(e)[:400])

    EXTRA["trades"] = (EXTRA.get("trades") or []) + trades
    ok_n = sum(1 for t in trades if t.get("ok"))
    ok = ok_n >= 3
    detail = f"market={market[:12]}… trades_ok={ok_n}/{len(trades)}"
    (OUT / "multi-buy.json").write_text(json.dumps(trades, indent=2), encoding="utf-8")
    return SuiteResult("multi-buy-gaussian", ok, int((time.time() - t0) * 1000), detail, str(OUT / "multi-buy.json"))


def inline_create_review_buy() -> SuiteResult:
    """Self-contained Playwright path if phase6-flow is skipped/fails: create→review→N buys."""
    t0 = time.time()
    OUT.mkdir(parents=True, exist_ok=True)
    try:
        kp = flow.fund_chain()
    except Exception as e:
        return SuiteResult("inline-business", False, 0, f"fund {e}")
    stamp = str(int(time.time()) % 10_000_000)
    secret = list(bytes(kp))
    trades: list[dict] = []
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            # Primary business path: English UI + English canonical listing (not zh-only).
            ctx = browser.new_context(
                locale="en-US",
                extra_http_headers={"Accept-Language": "en-US,en;q=0.9"},
                viewport={"width": 1500, "height": 960},
            )
            page = ctx.new_page()
            flow.connect_wallet(page, secret)
            page.get_by_role("link", name="Portfolio").click()
            page.wait_for_load_state("networkidle")
            page.locator("input[type='number']").first.fill("2000")
            page.get_by_role("button", name="Deposit", exact=True).click()
            flow.wait_text(page, "deposit", timeout_ms=45_000)

            page.goto(BASE + "/create", wait_until="networkidle")
            # Default family is Gaussian — same as phase6-flow
            if page.get_by_label("n_grid / atoms").count():
                page.get_by_label("n_grid / atoms").select_option("8")
            g_title = f"Suite Gaussian {stamp}"
            fill_label(page, "Market title", g_title)
            fill_label(page, "Tags", "macro, suite")
            fill_label(page, "Trading event", f"US CPI YoY first print {stamp}")
            fill_label(
                page,
                "Description",
                "Suite Playwright: first official print. Revisions do not settle.",
                textarea=True,
            )
            fill_label(page, "Topic / series (on-chain id)", f"suiteg{stamp}")
            fill_label(page, "Tag / release", "suite")
            if page.locator("label").filter(has_text=re.compile(r"^Or seconds from now$")).count():
                fill_label(page, "Or seconds from now", "7200")
            # Leave Native * empty — English-only listing is the default product path.
            page.get_by_role("button", name="Submit for review").click()
            flow.wait_text(page, "after review", timeout_ms=45_000)
            page.get_by_role("link", name="Review").click()
            page.wait_for_load_state("networkidle")
            page.locator("li").filter(has_text=g_title).get_by_role(
                "button", name="Approve and open prediction market"
            ).click()
            link = page.get_by_role("link", name="Open prediction market", exact=True)
            expect(link).to_be_visible(timeout=90_000)
            market = (link.get_attribute("href") or "").split("/m/")[-1]
            EXTRA["markets"]["gaussian"] = market
            page.screenshot(path=str(OUT / "inline-opened.png"), full_page=True)

            page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
            expect(page.locator("h1.font-display")).to_contain_text("Suite Gaussian", timeout=15_000)
            for attempt in range(3):
                page.get_by_role("button", name="Open session").click()
                try:
                    flow.wait_session_live(page, timeout_ms=45_000)
                    break
                except PwTimeout:
                    if attempt == 2:
                        raise
                    page.reload(wait_until="networkidle")

            for i, (a, b, sh) in enumerate([(0, 1, 1), (1, 2, 1), (2, 3, 1), (0, 2, 2), (3, 4, 1)][:N_BUYS], 1):
                page.goto(f"{BASE}/m/{market}", wait_until="networkidle")
                bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
                for _ in range(20):
                    if bars.count() > max(a, b):
                        break
                    page.wait_for_timeout(400)
                    page.reload(wait_until="networkidle")
                    bars = page.locator("div.flex.h-64.items-end button, button[title^='cell ']")
                bars.nth(a).click()
                bars.nth(b).click()
                page.get_by_role("button", name="Buy set").click()
                try:
                    flow.wait_text(page, "confirmed", timeout_ms=45_000)
                    page.screenshot(path=str(OUT / f"inline-buy-{i}.png"), full_page=True)
                    trades.append({"i": i, "ok": True, "cells": [a, b], "shares": sh})
                except PwTimeout:
                    trades.append({"i": i, "ok": False, "detail": flow.note_or_err(page)[:160]})

            # Skellam second market + 2 buys
            page.goto(BASE + "/create", wait_until="networkidle")
            page.get_by_role("button", name="Skellam").first.click()
            fill_label(page, "Market title", f"Suite Skellam {stamp}")
            fill_label(page, "Tags", "football, suite")
            fill_label(page, "Trading event", f"suite match {stamp}")
            fill_label(page, "Description", "Suite Skellam: full-time score. Extra time does not count.", textarea=True)
            fill_label(page, "Topic / series (on-chain id)", f"suites{stamp}")
            if page.get_by_label("Tag / release", exact=True).count():
                fill_label(page, "Tag / release", "suite")
            if page.locator("label").filter(has_text=re.compile(r"^Or seconds from now$")).count():
                fill_label(page, "Or seconds from now", "7200")
            page.get_by_role("button", name="Submit for review").click()
            flow.wait_text(page, "after review", timeout_ms=45_000)
            page.get_by_role("link", name="Review").click()
            page.wait_for_load_state("networkidle")
            page.locator("li").filter(has_text=f"Suite Skellam {stamp}").get_by_role(
                "button", name="Approve and open prediction market"
            ).click()
            link = page.get_by_role("link", name="Open prediction market", exact=True)
            expect(link).to_be_visible(timeout=90_000)
            sm = (link.get_attribute("href") or "").split("/m/")[-1]
            EXTRA["markets"]["skellam"] = sm
            for i in range(1, 3):
                page.goto(f"{BASE}/m/{sm}", wait_until="networkidle")
                for _ in range(16):
                    if page.get_by_role("button", name="Home", exact=True).count() or page.get_by_role("button", name="1X2 Home").count():
                        break
                    page.wait_for_timeout(400)
                    page.reload(wait_until="networkidle")
                if page.get_by_role("button", name="1X2 Home").count():
                    page.get_by_role("button", name="1X2 Home").click()
                else:
                    page.get_by_role("button", name="Home", exact=True).click()
                page.wait_for_timeout(500)
                buy = page.get_by_role("button", name="Buy line")
                if buy.count() == 0:
                    buy = page.get_by_role("button", name="Buy set")
                buy.first.click()
                try:
                    flow.wait_text(page, "confirmed", timeout_ms=45_000)
                    trades.append({"i": f"s{i}", "ok": True, "line": "home"})
                    page.screenshot(path=str(OUT / f"inline-skellam-{i}.png"), full_page=True)
                except PwTimeout:
                    trades.append({"i": f"s{i}", "ok": False, "detail": flow.note_or_err(page)[:160]})

            ctx.close()
            browser.close()
    except Exception as e:
        EXTRA["trades"] = trades
        return SuiteResult("inline-business", False, int((time.time() - t0) * 1000), str(e)[:400])

    EXTRA["trades"] = trades
    ok_n = sum(1 for t in trades if t.get("ok"))
    ok = ok_n >= 3 and "gaussian" in EXTRA["markets"]
    return SuiteResult(
        "inline-business",
        ok,
        int((time.time() - t0) * 1000),
        f"trades_ok={ok_n}/{len(trades)} markets={EXTRA['markets']}",
        str(OUT / "inline-opened.png") if (OUT / "inline-opened.png").exists() else "",
    )


def write_report() -> Path:
    OUT.mkdir(parents=True, exist_ok=True)
    passed = sum(1 for r in RESULTS if r.ok)
    failed = sum(1 for r in RESULTS if not r.ok)
    trades = EXTRA.get("trades") or []
    trades_ok = sum(1 for t in trades if t.get("ok"))
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    core_ok = any(r.name == "inline-business" and r.ok for r in RESULTS) or (
        any(r.name == "phase6-flow" and r.ok for r in RESULTS)
        and any(r.name == "multi-buy-gaussian" and r.ok for r in RESULTS)
    )
    i18n_ok = any(r.name == "listing-i18n" and r.ok for r in RESULTS)
    # Core = English business path + listing i18n (en-US + zh-CN). Soft suites may still fail.
    if core_ok and i18n_ok and trades_ok >= 3:
        verdict = "PASS" if failed == 0 else "PASS*"
    else:
        verdict = "FAIL"
    lines = [
        "# Playwright suite report",
        "",
        f"- Generated: `{now}`",
        f"- Web: `{BASE}` · API: `{API}` · RPC: `{RPC}`",
        f"- Locale under test: **en-US** (primary) + **zh-CN** (listing i18n optional translation)",
        f"- Suites: **{passed} passed** / **{failed} failed** / {len(RESULTS)} total",
        f"- Extra trades: **{trades_ok}** ok / {len(trades)} attempted",
        f"- Verdict: **{verdict}**",
        "",
        "## Markets",
        "",
    ]
    if EXTRA.get("markets"):
        for k, v in EXTRA["markets"].items():
            lines.append(f"- **{k}**: `{v}`")
    else:
        lines.append("- _(none recorded)_")
    lines += ["", "## Suites", "", "| Status | Suite | ms | Detail | Log |", "| --- | --- | ---: | --- | --- |"]
    for r in RESULTS:
        det = r.detail.replace("|", "\\|").replace("\n", " ")[:160]
        log = Path(r.log_path).name if r.log_path else ""
        lines.append(f"| {'✅' if r.ok else '❌'} | {r.name} | {r.elapsed_ms} | {det} | `{log}` |")
    lines += ["", "## Trades", "", "```json", json.dumps(trades, indent=2, ensure_ascii=False), "```", ""]
    lines += [
        "## Coverage",
        "",
        "| Area | How |",
        "| --- | --- |",
        "| Connect / SIWS / deposit (en-US) | Playwright inline-business / phase6-flow |",
        "| Create → Review → Open (English listing) | Playwright |",
        "| SessionToken multi Buy set | Playwright multi-buy / inline |",
        "| Skellam Buy line | Playwright |",
        "| Listing i18n en-US default + zh-CN Show English | listing-i18n-playwright |",
        "| Lobby / wallet shell smoke | phase6-playwright / wallet-shell |",
        "",
        f"Artifacts directory: `{OUT}`",
        "",
    ]
    path = OUT / "report.md"
    path.write_text("\n".join(lines), encoding="utf-8")
    (OUT / "report.json").write_text(
        json.dumps(
            {
                "generated": now,
                "verdict": verdict,
                "passed": passed,
                "failed": failed,
                "trades_ok": trades_ok,
                "markets": EXTRA.get("markets"),
                "results": [asdict(r) for r in RESULTS],
                "trades": trades,
                "api_health": EXTRA.get("api_health"),
            },
            indent=2,
            ensure_ascii=False,
        ),
        encoding="utf-8",
    )
    return path


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    flow.OUT = OUT / "flow-shots"
    flow.OUT.mkdir(parents=True, exist_ok=True)

    try:
        if not health():
            RESULTS.append(SuiteResult("stack-health", False, 0, json.dumps(EXTRA.get("api_health"))))
            report = write_report()
            print("STACK_FAIL", report)
            return 1
        RESULTS.append(SuiteResult("stack-health", True, 0, json.dumps(EXTRA.get("api_health", {}))[:120]))

        # Primary English business path (en-US + English listing)
        RESULTS.append(inline_create_review_buy())

        # Canonical phase6-flow (English UI)
        flow_res = run_cmd(
            "phase6-flow",
            [sys.executable, str(ROOT / "scripts" / "phase6-flow-playwright.py")],
            timeout_s=700,
        )
        RESULTS.append(flow_res)
        if flow_res.log_path and Path(flow_res.log_path).exists():
            # Prefer full pubkeys from flow log; keep inline markets if already recorded.
            before = dict(EXTRA.get("markets") or {})
            parse_flow_markets(Path(flow_res.log_path).read_text(encoding="utf-8", errors="replace"))
            for k, v in before.items():
                if k not in EXTRA["markets"] or len(str(EXTRA["markets"].get(k, ""))) < len(str(v)):
                    EXTRA["markets"][k] = v

        if EXTRA.get("markets", {}).get("gaussian"):
            RESULTS.append(multi_buy_gaussian())

        # Listing i18n: en-US default + zh-CN optional translation
        RESULTS.append(
            run_cmd(
                "listing-i18n",
                [sys.executable, str(ROOT / "scripts" / "listing-i18n-playwright.py")],
                timeout_s=120,
            )
        )
        RESULTS.append(
            run_cmd(
                "phase6-ui-smoke",
                [sys.executable, str(ROOT / "scripts" / "phase6-playwright.py")],
                timeout_s=180,
            )
        )
        RESULTS.append(
            run_cmd(
                "wallet-shell",
                [sys.executable, str(ROOT / "scripts" / "wallet-shell-playwright.py")],
                timeout_s=90,
            )
        )
    except Exception as e:
        RESULTS.append(SuiteResult("suite-crash", False, 0, str(e)[:400]))
    finally:
        report = write_report()
        print("REPORT", report)

    core_ok = any(r.name == "inline-business" and r.ok for r in RESULTS) or (
        any(r.name == "phase6-flow" and r.ok for r in RESULTS)
        and any(r.name == "multi-buy-gaussian" and r.ok for r in RESULTS)
    )
    i18n_ok = any(r.name == "listing-i18n" and r.ok for r in RESULTS)
    verdict = "PASS" if core_ok and i18n_ok else "FAIL"
    print("VERDICT", verdict)
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
