#!/usr/bin/env python3
"""FR-UI-48 lobby/market: English canonical display + optional locale translation.

Covers:
  1) en-US browser — English title/event by default (no translation badge required)
  2) zh-CN browser — Accept-Language serves zh-Hans copy + Show English toggle
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request

from playwright.sync_api import expect, sync_playwright

BASE = os.environ.get("WEB_URL", "http://127.0.0.1:3000")
API = os.environ.get("MARKET_API", "http://127.0.0.1:8080")

EN_TITLE = "US CPI YoY — PW i18n"
EN_EVENT = "US CPI YoY first print"
EN_DESC = "First official print. Revisions do not settle. Playwright i18n seed."
ZH_TITLE = "美国 CPI 同比 — PW"
ZH_EVENT = "美国 CPI 首次官方打印"
ZH_DESC = "首次官方打印。修订不结算。Playwright 种子。"

# Filled after seed (OPEN lock may keep earlier English title).
SEEDED_EN_TITLE = EN_TITLE


def api(method: str, path: str, body: dict | None = None, headers: dict | None = None) -> tuple[int, dict]:
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        API + path,
        data=data,
        method=method,
        headers={"content-type": "application/json", **(headers or {})},
    )
    try:
        with urllib.request.urlopen(req, timeout=8) as r:
            raw = r.read().decode()
            return r.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            return e.code, json.loads(raw) if raw else {}
        except json.JSONDecodeError:
            return e.code, {"raw": raw}


def seed_i18n_listing() -> str:
    global SEEDED_EN_TITLE
    st, health = api("GET", "/v1/health")
    if st != 200:
        raise RuntimeError(f"market-api down ({st}): {health}")
    st, page = api("GET", "/v1/markets?limit=5")
    if st != 200 or not page.get("items"):
        raise RuntimeError("need at least one indexed market for listing i18n UI test")
    market = page["items"][0]["market"]
    payload = {
        "market": market,
        "title": EN_TITLE,
        "tags": ["macro"],
        "event": EN_EVENT,
        "description": EN_DESC,
        # Canonical English; optional native locale is display-only.
        "source_locale": "en",
        "i18n": {
            "zh-Hans": {
                "title": ZH_TITLE,
                "event": ZH_EVENT,
                "description": ZH_DESC,
            }
        },
    }
    st, body = api("POST", "/v1/listings", payload)
    if st in (200, 409):
        if st == 409:
            st2, cur = api("GET", f"/v1/listings/{market}?locale=en")
            if st2 != 200:
                raise RuntimeError(f"seed listing failed {st} and cannot read {st2}")
            locked_title = cur.get("title_en") or cur.get("title") or EN_TITLE
            st, body = api(
                "POST",
                "/v1/listings",
                {
                    "market": market,
                    "title": locked_title,
                    "tags": cur.get("tags") or ["macro"],
                    "event": cur.get("event_en") or cur.get("event") or EN_EVENT,
                    "description": cur.get("description_en") or cur.get("description") or EN_DESC,
                    "topic": cur.get("topic") or "",
                    "tag": cur.get("tag") or "",
                    "source_locale": "en",
                    "i18n": payload["i18n"],
                },
            )
            if st != 200:
                raise RuntimeError(f"seed i18n on locked listing failed {st} {body}")
            SEEDED_EN_TITLE = locked_title
        else:
            SEEDED_EN_TITLE = EN_TITLE
    else:
        raise RuntimeError(f"seed listing failed {st} {body}")
    # Confirm English read path.
    st_en, en = api("GET", f"/v1/listings/{market}?locale=en")
    if st_en == 200:
        SEEDED_EN_TITLE = en.get("title_en") or en.get("title") or SEEDED_EN_TITLE
    return market


def assert_english_lobby(page) -> None:
    expect(page.get_by_role("heading", name="Live markets")).to_be_visible()
    expect(page.get_by_role("heading", name=SEEDED_EN_TITLE).first).to_be_visible(timeout=15_000)
    # Under en-US, Chinese translation titles must not appear as lobby headings.
    expect(page.get_by_role("heading", name=ZH_TITLE)).to_have_count(0)


def assert_chinese_lobby_then_english(page) -> None:
    expect(page.get_by_role("heading", name="Live markets")).to_be_visible()
    expect(page.get_by_role("heading", name=ZH_TITLE).first).to_be_visible(timeout=15_000)
    expect(page.get_by_text("Translation", exact=False).first).to_be_visible()
    page.get_by_role("button", name="Show English").first.click()
    expect(page.get_by_role("heading", name=SEEDED_EN_TITLE).first).to_be_visible()
    expect(page.get_by_role("button", name="Show translation").first).to_be_visible()


def main() -> int:
    try:
        market = seed_i18n_listing()
    except Exception as e:
        print("FAIL seed", e)
        return 1

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)

        # 1) English locale — canonical English is the primary product surface.
        en_ctx = browser.new_context(
            locale="en-US",
            extra_http_headers={"Accept-Language": "en-US,en;q=0.9"},
            viewport={"width": 1400, "height": 900},
        )
        en_page = en_ctx.new_page()
        en_page.goto(BASE + "/", wait_until="networkidle")
        assert_english_lobby(en_page)
        en_page.goto(BASE + f"/m/{market}", wait_until="networkidle")
        board_title = en_page.locator("h1.font-display")
        expect(board_title).to_be_visible(timeout=15_000)
        expect(board_title).to_contain_text(SEEDED_EN_TITLE[:12])
        expect(en_page.get_by_text("Translations", exact=True)).to_be_visible()
        expect(en_page.get_by_role("button", name="Add / edit")).to_be_visible()
        en_ctx.close()

        # 2) Chinese locale — optional translation + Show English.
        zh_ctx = browser.new_context(
            locale="zh-CN",
            extra_http_headers={"Accept-Language": "zh-CN,zh;q=0.9,en;q=0.5"},
            viewport={"width": 1400, "height": 900},
        )
        zh_page = zh_ctx.new_page()
        zh_page.goto(BASE + "/", wait_until="networkidle")
        assert_chinese_lobby_then_english(zh_page)
        zh_page.goto(BASE + f"/m/{market}", wait_until="networkidle")
        board_title = zh_page.locator("h1.font-display")
        expect(board_title).to_be_visible(timeout=15_000)
        # zh board may show translation; English toggle must reveal canonical title.
        if zh_page.get_by_role("button", name="Show English").count():
            zh_page.get_by_role("button", name="Show English").first.click()
            expect(zh_page.get_by_role("button", name="Show translation").first).to_be_visible()
            expect(board_title).to_contain_text(SEEDED_EN_TITLE[:12])
        else:
            expect(board_title).to_contain_text(SEEDED_EN_TITLE[:12])
        expect(zh_page.get_by_text("Translations", exact=True)).to_be_visible()
        zh_ctx.close()

        browser.close()
    print(f"OK listing-i18n playwright (en-US + zh-CN) market={market}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
