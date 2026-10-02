#!/usr/bin/env python3
"""Phase 6 UI: routes, wallet modal, SIWS copy, board ticket. Chromium headless."""

from pathlib import Path
import json
import os
import sys

from playwright.sync_api import expect, sync_playwright

BASE = "http://127.0.0.1:3000"
SEED = "Seed111111111111111111111111111111111111111"
SKEL = "Skel111111111111111111111111111111111111111"
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tmp" / "phase6-pw"
OUT.mkdir(parents=True, exist_ok=True)
ENV = ROOT / "tmp" / "phase6.env"


def env_market() -> str:
    m = os.environ.get("PHASE6_MARKET", "").strip()
    if m:
        return m
    if ENV.exists():
        for line in ENV.read_text(encoding="utf-8").splitlines():
            if line.startswith("PHASE6_MARKET="):
                return line.split("=", 1)[1].strip()
    return ""


def shot(page, name: str):
    page.screenshot(path=str(OUT / f"{name}.png"), full_page=True)


def main() -> int:
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": 1400, "height": 900})

        page.goto(BASE + "/", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Live markets")).to_be_visible()
        expect(page.get_by_role("heading", level=2).first).to_be_visible()
        expect(page.get_by_text("The row heading is the market name")).to_be_visible()
        expect(page.get_by_text("Fills are public on Solana")).to_be_visible()
        expect(page.get_by_text("does not promise on-chain anonymity")).to_be_visible()
        expect(page.get_by_text("Session revoke is not the same as disconnecting")).to_be_visible()
        expect(page.get_by_text("close ").first).to_be_visible()
        expect(page.get_by_text("C_max").first).to_be_visible()
        expect(page.get_by_text("payable").first).to_be_visible()
        expect(page.get_by_label("Search")).to_be_visible()
        expect(page.get_by_role("button", name="Search")).to_be_visible()
        expect(page.get_by_text("markets · page")).to_be_visible()
        page.goto(BASE + "/?limit=1", wait_until="networkidle")
        if page.get_by_role("link", name="Next").count():
            page.get_by_role("link", name="Next").click()
            page.wait_for_load_state("networkidle")
            expect(page.get_by_text("page 2 /")).to_be_visible()
        page.goto(BASE + "/", wait_until="networkidle")
        board_href = page.get_by_role("link", name="Market", exact=True).first.get_attribute("href")
        assert board_href and board_href.startswith("/m/")
        mid = board_href.split("/m/")[1]
        page.get_by_label("Search").fill(mid[:8])
        page.get_by_role("button", name="Search").click()
        page.wait_for_load_state("networkidle")
        expect(page.get_by_text(mid)).to_be_visible()
        page.goto(BASE + "/", wait_until="networkidle")
        shot(page, "01-lobby")

        page.get_by_role("button", name="Select Wallet").click()
        expect(page.get_by_text("Connect a wallet on Solana to continue")).to_be_visible()
        expect(page.get_by_text("Localnet keypair")).to_be_visible()
        expect(page.get_by_text("Phantom")).to_be_visible()
        expect(page.get_by_text("Solflare")).to_be_visible()
        shot(page, "02-wallet-modal")
        page.keyboard.press("Escape")

        expect(page.get_by_role("link", name="Create", exact=True)).to_be_visible()
        expect(page.get_by_role("link", name="Auctions", exact=True)).to_be_visible()
        expect(page.get_by_role("link", name="Ops", exact=True)).to_be_visible()
        expect(page.get_by_role("link", name="LP", exact=True)).to_be_visible()
        page.get_by_role("link", name="Portfolio").click()
        page.wait_for_load_state("networkidle")
        expect(page.get_by_role("heading", name="Portfolio")).to_be_visible()
        expect(page.get_by_text("cannot move unused margin")).to_be_visible()
        expect(page.get_by_text("Your tickets", exact=True)).to_be_visible()
        expect(page.get_by_text("Connect a wallet to see tickets")).to_be_visible()
        expect(page.get_by_text("Cash ticket")).to_be_visible()
        expect(page.get_by_text("Circle SPL USDC only")).to_be_visible()
        expect(page.get_by_text("Free to withdraw")).to_be_visible()
        expect(page.get_by_role("button", name="Deposit", exact=True)).to_be_visible()
        expect(page.get_by_role("button", name="Withdraw", exact=True)).to_be_visible()
        page.get_by_role("button", name="Deposit", exact=True).click()
        expect(page.get_by_text("connect + SIWS first")).to_be_visible()
        shot(page, "03-portfolio")

        page.get_by_role("link", name="Committee").click()
        page.wait_for_load_state("networkidle")
        expect(page.get_by_role("heading", name="Committee")).to_be_visible()
        expect(page.get_by_text("One protocol-wide roster").first).to_be_visible()
        expect(page.get_by_text("Submit event result")).to_be_visible()
        expect(page.get_by_label("Search markets")).to_be_visible()
        expect(page.get_by_text("Your role")).to_be_visible()
        action = page.get_by_role("button", name="Open window")
        if action.count() == 0:
            action = page.get_by_role("button", name="Submit result")
        if action.count():
            action.click()
            expect(page.get_by_text("connect a wallet first")).to_be_visible()
        else:
            expect(page.get_by_text("connect a wallet").first).to_be_visible()
        shot(page, "04-committee")

        mid = env_market() or mid
        page.goto(BASE + f"/m/{mid}", wait_until="networkidle")
        expect(page.get_by_text("Market card")).to_be_visible()
        expect(page.get_by_text("Trading close")).to_be_visible()
        expect(page.get_by_text("Payable")).to_be_visible()
        expect(page.get_by_text("trading-implied PDF")).to_be_visible()
        expect(page.get_by_text("Implied PDF from trading")).to_be_visible()
        expect(page.get_by_text("snapshot slot")).to_be_visible()
        expect(page.get_by_text("not computed in this browser")).to_be_visible()
        expect(page.get_by_text("undefined while L_max is 0")).to_be_visible()
        expect(page.get_by_text("selected")).to_be_visible()
        expect(page.get_by_text("distinct owners with q > 0")).to_be_visible()
        expect(page.get_by_text("sum of cost_paid")).to_be_visible()
        expect(page.get_by_text("locked+filled risk capital")).to_be_visible()
        expect(page.get_by_text("not baked into the quote")).to_be_visible()
        expect(page.get_by_role("button", name="Buy set")).to_be_visible()
        expect(page.get_by_role("button", name="Sell set")).to_be_visible()
        expect(page.get_by_text("Rules", exact=True)).to_be_visible()
        expect(page.get_by_text("p_S", exact=True)).to_be_visible()
        expect(page.get_by_text("C_S(q)", exact=True)).to_be_visible()
        expect(page.get_by_text("coverage", exact=True)).to_be_visible()
        expect(page.get_by_text("fee", exact=True)).to_be_visible()
        expect(page.get_by_text("Pre-bet ticket")).to_be_visible()
        expect(page.get_by_text("You pay now")).to_be_visible()
        expect(page.get_by_text("If S hits")).to_be_visible()
        expect(page.get_by_text("If S misses")).to_be_visible()
        expect(page.get_by_text("Net if hit")).to_be_visible()
        expect(page.get_by_text("Book EV", exact=True)).to_be_visible()
        page.get_by_role("button", name="Buy set").click()
        expect(page.get_by_text("connect a wallet").first).to_be_visible()
        shot(page, "05-board")

        page.get_by_role("link", name="Auction", exact=True).click()
        page.wait_for_load_state("networkidle")
        expect(page.get_by_text("Risk auction")).to_be_visible()
        expect(page.get_by_text("This page uses the connected wallet")).to_be_visible()
        expect(page.get_by_text("Layer stack")).to_be_visible()
        expect(page.get_by_text("If this layer is drawn")).to_be_visible()
        expect(page.get_by_role("button", name="Quote layer")).to_be_visible()
        shot(page, "06-auction")

        if page.get_by_text("Skellam").count():
            page.goto(BASE + f"/m/{SKEL}", wait_until="networkidle")
            expect(page.locator("[title^='score ']")).to_have_count(121)
            shot(page, "05b-skellam")

        page.goto(BASE + "/create", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Create prediction market")).to_be_visible()
        expect(page.get_by_text("Submit a market application by distribution family")).to_be_visible()
        expect(page.get_by_text("Identity ticket")).to_be_visible()
        expect(page.get_by_label("Market title")).to_be_visible()
        expect(page.get_by_label("Tags")).to_be_visible()
        expect(page.get_by_label("Trading event")).to_be_visible()
        expect(page.get_by_label("Description")).to_be_visible()
        expect(page.get_by_text("Prior ticket")).to_be_visible()
        expect(page.get_by_role("button", name="US CPI YoY")).to_be_visible()
        expect(page.get_by_text("FIRST_PRINT")).to_be_visible()
        expect(page.get_by_role("button", name="Submit for review")).to_be_visible()
        shot(page, "08-create")

        page.goto(BASE + "/auctions", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Auctions")).to_be_visible()
        expect(page.locator("main")).to_contain_text("Risk auctions for live markets")
        shot(page, "09-auctions")

        page.goto(BASE + "/ops", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Ops")).to_be_visible()
        expect(page.get_by_text("Read-only")).to_be_visible()
        expect(page.get_by_text("disabled")).to_be_visible()
        expect(page.get_by_text("Keeper close")).to_be_visible()
        shot(page, "10-ops")

        page.goto(BASE + "/lp", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Risk LP")).to_be_visible()
        expect(page.get_by_text("This page uses the connected wallet")).to_be_visible()
        shot(page, "11-lp")

        page.goto(BASE + f"/resolve/{mid}", wait_until="networkidle")
        expect(page.get_by_text("Resolution")).to_be_visible()
        expect(page.get_by_text("Market card")).to_be_visible()
        expect(page.get_by_text("no oracle settler")).to_be_visible()
        expect(page.get_by_text("A trading Session cannot open, submit, challenge, or vote")).to_be_visible()
        expect(page.get_by_text("Your role")).to_be_visible()
        if page.get_by_role("button", name="Open window").count() == 0:
            expect(page.get_by_text("Phase")).to_be_visible()
        else:
            expect(page.get_by_role("button", name="Open window")).to_be_visible()
        shot(page, "07-resolve")

        ch = page.request.post(
            BASE + "/api/siws/challenge",
            data=json.dumps({"address": "HaHjcAoa9wanJvWDugM4bFoBsQXyM5vg9gsSzZFp8qhn", "uri": BASE}),
            headers={"content-type": "application/json"},
        )
        assert ch.ok, ch.text()
        msg = ch.json()["message"]
        assert "cannot buy_set" in msg, msg
        assert "cannot buy_set, withdraw, or move USDC" in msg

        me = page.request.get(BASE + "/api/me")
        assert me.ok
        assert me.json()["ok"] is False

        verify = page.request.post(
            BASE + "/api/siws/verify",
            data=json.dumps({"address": "x", "message": "nope", "signature": "YQ=="}),
            headers={"content-type": "application/json"},
        )
        assert verify.status == 400 or verify.status == 401

        browser.close()

    print("PHASE6_PW_OK", OUT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
