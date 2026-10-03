#!/usr/bin/env python3
"""Localnet UI: Localnet keypair → SIWS → Open session → Buy set on an existing Gaussian board."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

from playwright.sync_api import TimeoutError as PwTimeout
from playwright.sync_api import sync_playwright
from solders.keypair import Keypair

ROOT = Path(__file__).resolve().parents[1]
BASE = os.environ.get("WEB_URL", "http://127.0.0.1:3000")
ENV = ROOT / "tmp" / "phase6.env"
KP_PATH = ROOT / "tmp" / "phase6-id.json"
OUT = ROOT / "tmp" / "phase6-session-fill"
FINDINGS: list[str] = []


def env_market() -> str:
    m = os.environ.get("PHASE6_MARKET", "").strip()
    if m:
        return m
    if ENV.exists():
        for line in ENV.read_text(encoding="utf-8").splitlines():
            if line.startswith("PHASE6_MARKET="):
                return line.split("=", 1)[1].strip()
    return ""


def load_kp(path: Path) -> Keypair:
    return Keypair.from_bytes(bytes(json.loads(path.read_text())))


def shot(page, name: str) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(OUT / f"{name}.png"), full_page=True)


def wait_text(page, needle: str, timeout_ms=20_000) -> None:
    page.wait_for_function(
        """(n) => document.body && document.body.innerText.toLowerCase().includes(n)""",
        arg=needle.lower(),
        timeout=timeout_ms,
    )


def connect_wallet(page, secret: list[int]) -> None:
    page.add_init_script(
        f"window.__cpmPendingKeypair = {json.dumps(secret)};"
        "try{localStorage.setItem('walletName', JSON.stringify('Localnet keypair'));}catch(e){}"
    )
    page.goto(BASE + "/", wait_until="networkidle")
    page.evaluate(
        """() => fetch('/api/siws/verify', {method:'POST', headers:{'content-type':'application/json'}, body:'{}'}).catch(() => {})"""
    )
    page.wait_for_timeout(400)
    if page.get_by_role("button", name="Select Wallet").count():
        page.get_by_role("button", name="Select Wallet").click()
        page.locator(".wallet-adapter-modal-list button").first.click()
        page.wait_for_timeout(800)
    if page.get_by_role("button", name="Sign in").count():
        page.get_by_role("button", name="Sign in").click()
        page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
    elif page.get_by_role("button", name="Open session").count() == 0:
        FINDINGS.append("wallet connect produced neither Sign in nor Open session")
        shot(page, "connect-fail")
        raise RuntimeError("wallet not connected")


def main() -> int:
    mid = env_market()
    if not mid:
        print("PHASE6_MARKET missing")
        return 1
    if not KP_PATH.exists():
        print("tmp/phase6-id.json missing — run fund_chain first")
        return 1
    kp = load_kp(KP_PATH)
    secret = list(bytes(kp))
    OUT.mkdir(parents=True, exist_ok=True)
    failed = 0

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": 1400, "height": 900})
        page.on("pageerror", lambda e: FINDINGS.append(f"pageerror {e}") if "Minified React error" not in str(e) else None)
        connect_wallet(page, secret)
        shot(page, "01-connected")

        page.goto(BASE + f"/m/{mid}", wait_until="networkidle")
        page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
        page.get_by_role("button", name="Open session").click()
        try:
            wait_text(page, "session live", timeout_ms=45_000)
            print("open session ok")
        except PwTimeout:
            body = page.locator("body").inner_text()[:600]
            FINDINGS.append("Open session did not confirm: " + body)
            print("open session FAIL")
            failed += 1
        shot(page, "02-session")

        bars = page.locator("div.flex.h-64.items-end button")
        for _ in range(20):
            if bars.count() >= 2:
                break
            page.wait_for_timeout(500)
            page.reload(wait_until="networkidle")
        if bars.count() < 2:
            FINDINGS.append(f"board {mid} has no PDF bars")
            failed += 1
        else:
            bars.nth(0).click()
            bars.nth(1).click()
            page.get_by_role("button", name="Buy set").click()
            try:
                wait_text(page, "confirmed ", timeout_ms=45_000)
                print("buy_set ok")
            except PwTimeout:
                FINDINGS.append("buy_set did not confirm: " + page.locator("body").inner_text()[-400:])
                print("buy_set FAIL")
                failed += 1
        shot(page, "03-buy")
        browser.close()

    (OUT / "findings.json").write_text(json.dumps(FINDINGS, indent=2), encoding="utf-8")
    print("FINDINGS", len(FINDINGS))
    for f in FINDINGS:
        print("-", f[:300])
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        print("FLOW_FATAL", e)
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "fatal.txt").write_text(str(e), encoding="utf-8")
        raise
