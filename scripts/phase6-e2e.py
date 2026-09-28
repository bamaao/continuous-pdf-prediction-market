#!/usr/bin/env python3
"""Localnet path: inject keypair → SIWS → deposit → session → buy → withdraw → disconnect ≠ revoke."""

import json
import os
import sys
from pathlib import Path

from playwright.sync_api import expect, sync_playwright

BASE = os.environ.get("WEB_URL", "http://127.0.0.1:3000")
MARKET = os.environ["PHASE6_MARKET"]
KEYPAIR = json.loads(Path(os.environ["PHASE6_KEYPAIR"]).read_text())
OUT = Path(__file__).resolve().parents[1] / "tmp" / "phase6-e2e"
OUT.mkdir(parents=True, exist_ok=True)


def main() -> int:
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": 1400, "height": 900})
        page.add_init_script(f"window.__cpmPendingKeypair = {json.dumps(KEYPAIR)};")
        page.on(
            "filechooser",
            lambda chooser: chooser.set_files(os.environ["PHASE6_KEYPAIR"]),
        )

        page.goto(BASE + "/", wait_until="networkidle")
        page.evaluate(f"window.__cpmPendingKeypair = {json.dumps(KEYPAIR)}")
        page.get_by_role("button", name="Select Wallet").click()
        page.get_by_text("Localnet keypair").first.click()
        connect = page.get_by_role("button", name="Connect")
        if connect.count():
            connect.click()
        expect(page.get_by_role("button", name="Sign in")).to_be_visible(timeout=20_000)
        page.screenshot(path=str(OUT / "wallet.png"), full_page=True)
        page.get_by_role("button", name="Sign in").click()
        expect(page.get_by_role("button", name="Open session")).to_be_visible(timeout=20_000)
        page.screenshot(path=str(OUT / "siws.png"), full_page=True)

        page.get_by_role("link", name="Portfolio").click()
        page.wait_for_load_state("networkidle")
        page.get_by_role("button", name="Deposit", exact=True).click()
        page.wait_for_timeout(4000)
        expect(page.locator("text=/deposit [1-9A-HJ-NP-Za-km-z]{20,}/")).to_be_visible(timeout=20_000)

        page.get_by_role("link", name="Lobby").click()
        page.wait_for_load_state("networkidle")
        page.get_by_role("button", name="Open session").click()
        expect(page.get_by_text("session live")).to_be_visible(timeout=20_000)

        page.goto(BASE + f"/m/{MARKET}", wait_until="networkidle")
        page.get_by_role("button", name="Buy set").click()
        expect(page.get_by_text("confirmed")).to_be_visible(timeout=30_000)
        page.screenshot(path=str(OUT / "buy.png"), full_page=True)

        page.get_by_role("link", name="Portfolio").click()
        expect(page.get_by_role("heading", name="Portfolio")).to_be_visible(timeout=15_000)
        page.get_by_role("spinbutton").fill("1")
        page.get_by_role("button", name="Withdraw").click()
        expect(page.locator("text=/withdraw [1-9A-HJ-NP-Za-km-z]{20,}/")).to_be_visible(timeout=20_000)

        page.get_by_role("button", name="Disconnect").click()
        expect(page.get_by_text("on-chain session still live")).to_be_visible()
        expect(page.get_by_role("button", name="End session")).to_have_count(0)

        browser.close()
    print("PHASE6_E2E_OK", MARKET)
    return 0


if __name__ == "__main__":
    sys.exit(main())
