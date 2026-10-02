#!/usr/bin/env python3
"""iOS / Android shell: browse-in-wallet links when no injected provider."""

from __future__ import annotations

import sys
from urllib.parse import unquote

from playwright.sync_api import expect, sync_playwright

BASE = "http://127.0.0.1:3000"
IPHONE = (
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) "
    "AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1"
)
DESKTOP = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36"


def main() -> int:
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        phone = browser.new_page(
            viewport={"width": 390, "height": 844},
            user_agent=IPHONE,
            is_mobile=True,
            has_touch=True,
        )
        phone.goto(BASE + "/", wait_until="networkidle")
        expect(phone.get_by_role("link", name="Open in Phantom")).to_be_visible()
        expect(phone.get_by_role("link", name="Open in Solflare")).to_be_visible()
        href = phone.get_by_role("link", name="Open in Phantom").get_attribute("href") or ""
        if "phantom.app/ul/browse/" not in href or "127.0.0.1" not in unquote(href):
            print("FAIL phantom href", href)
            return 1
        expect(phone.get_by_role("button", name="Select Wallet")).to_have_count(0)
        expect(phone.locator('link[rel="manifest"]')).to_have_count(1)
        apple = phone.locator('link[rel="apple-touch-icon"]')
        if apple.count() == 0:
            # Next may emit apple-touch-icon via metadata
            html = phone.content()
            if "apple-touch-icon" not in html and "icon-180" not in html:
                print("FAIL missing apple icon")
                return 1
        phone.close()

        desk = browser.new_page(viewport={"width": 1400, "height": 900}, user_agent=DESKTOP)
        desk.goto(BASE + "/", wait_until="networkidle")
        expect(desk.get_by_role("button", name="Select Wallet")).to_be_visible()
        expect(desk.get_by_role("link", name="Open in Phantom")).to_have_count(0)
        desk.close()
        browser.close()
    print("OK wallet-shell playwright")
    return 0


if __name__ == "__main__":
    sys.exit(main())
