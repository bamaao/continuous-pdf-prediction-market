#!/usr/bin/env python3
"""Signed desk: deposit, then each family create → buy → submit x* → finalize → settle → claim."""

from __future__ import annotations

import importlib.util
import sys
import time
from pathlib import Path

from playwright.sync_api import TimeoutError as PwTimeout
from playwright.sync_api import expect
from solders.pubkey import Pubkey

ROOT = Path(__file__).resolve().parents[1]
BASE = "http://127.0.0.1:3000"
spec = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(flow)

FAMILIES = [
    {
        "id": 0,
        "name": "skellam",
        "pick": "Skellam Football",
        "create": "Create Skellam prediction market",
        "topic": "sk",
        "buy": "skellam",
        "x": ("Home", "1"),
        "xb": ("Away", "0"),
    },
    {
        "id": 1,
        "name": "gaussian",
        "pick": "Gaussian CPI",
        "create": "Create Gaussian prediction market",
        "topic": "ga",
        "n": "8",
        "buy": "cells",
        "x": ("Scalar x*", "2"),
    },
    {
        "id": 2,
        "name": "lognormal",
        "pick": "Lognormal Daily",
        "create": "Create Lognormal prediction market",
        "topic": "ln",
        "n": "8",
        "buy": "cells",
        "x": ("Scalar x*", "100000"),
    },
    {
        "id": 3,
        "name": "dirichlet",
        "pick": "Dirichlet Election",
        "create": "Create Dirichlet prediction market",
        "topic": "di",
        "buy": "cells",
        "x": ("Winning atom", "0"),
    },
    {
        "id": 4,
        "name": "bernoulli",
        "pick": "Bernoulli Binary",
        "create": "Create Bernoulli prediction market",
        "topic": "be",
        "buy": "cells",
        "yes": True,
    },
]

CLOSE_IN = 22
FINDINGS: list[str] = []


def fail(msg: str) -> None:
    FINDINGS.append(msg)
    flow.out("FAIL " + msg)


def wait_text(page, needle: str, timeout_ms=45_000) -> None:
    flow.wait_text(page, needle, timeout_ms=timeout_ms)


def open_market_href(page) -> str:
    link = page.get_by_role("link", name="Open market").first
    expect(link).to_be_visible()
    href = link.get_attribute("href") or ""
    return href.split("/m/")[-1]


def buy_on_board(page, spec: dict) -> None:
    page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
    if spec["buy"] == "skellam":
        for _ in range(24):
            if page.get_by_role("button", name="1X2 Home").count():
                break
            page.wait_for_timeout(500)
            page.reload(wait_until="networkidle")
        if page.get_by_role("button", name="1X2 Home").count() == 0:
            fail(f"{spec['name']} missing 1X2 Home")
            return
        page.get_by_role("button", name="1X2 Home").click()
        page.wait_for_timeout(600)
        btn = page.get_by_role("button", name="Buy line")
        if btn.count() == 0:
            fail(f"{spec['name']} Buy line missing")
            return
        btn.click()
    else:
        for _ in range(24):
            if page.locator("button[title^='cell ']").count() >= 1:
                break
            page.wait_for_timeout(500)
            page.reload(wait_until="networkidle")
        cells = page.locator("button[title^='cell ']")
        if cells.count() < 1:
            fail(f"{spec['name']} no PDF cells")
            return
        cells.nth(0).click()
        if cells.count() > 1:
            cells.nth(1).click()
        page.get_by_role("button", name="Buy set").click()
    try:
        wait_text(page, "confirmed ", timeout_ms=45_000)
        flow.out(f"{spec['name']} buy ok")
    except PwTimeout:
        fail(f"{spec['name']} buy did not confirm: {flow.note_or_err(page)}")


def settle_market(page, kp, spec: dict, market: str, close_at: float) -> None:
    remain = close_at - time.time() + 2
    if remain > 0:
        flow.out(f"{spec['name']} wait close {remain:.0f}s")
        page.wait_for_timeout(int(remain * 1000))
    page.goto(f"{BASE}/resolve/{market}", wait_until="networkidle")
    if page.get_by_role("button", name="Open window").count():
        page.get_by_role("button", name="Open window").click()
        try:
            wait_text(page, "resolve_open", timeout_ms=30_000)
        except PwTimeout:
            fail(f"{spec['name']} resolve_open: {flow.note_or_err(page)}")
            return
    for _ in range(40):
        if page.get_by_role("button", name="Submit result").count():
            break
        page.wait_for_timeout(500)
        if _ % 6 == 5:
            page.reload(wait_until="networkidle")
    if page.get_by_role("button", name="Submit result").count() == 0:
        fail(f"{spec['name']} Submit result not shown: {flow.note_or_err(page)}")
        return
    if spec.get("x"):
        page.get_by_label(spec["x"][0]).fill(spec["x"][1])
    if spec.get("xb"):
        page.get_by_label(spec["xb"][0]).fill(spec["xb"][1])
    if spec.get("yes"):
        page.get_by_label("YES").check()
    page.get_by_role("button", name="Submit result").click()
    try:
        wait_text(page, "submit_result", timeout_ms=30_000)
        flow.out(f"{spec['name']} submit_result ok")
    except PwTimeout:
        fail(f"{spec['name']} submit_result: {flow.note_or_err(page)}")
        return
    page.wait_for_timeout(9_000)
    fin = page.get_by_role("button", name="Finalize")
    for _ in range(24):
        if fin.count():
            break
        page.wait_for_timeout(500)
        if _ % 4 == 3:
            page.reload(wait_until="networkidle")
        fin = page.get_by_role("button", name="Finalize")
    if fin.count() == 0:
        fail(f"{spec['name']} Finalize not shown after challenge window: {flow.note_or_err(page)}")
        return
    fin.click()
    try:
        wait_text(page, "finalize", timeout_ms=30_000)
        flow.out(f"{spec['name']} finalize ok")
    except PwTimeout:
        fail(f"{spec['name']} finalize: {flow.note_or_err(page)}")
        return
    try:
        sig = flow.send_ixs(
            flow.Client(flow.RPC, commitment=flow.Confirmed),
            kp,
            [flow.compose("begin_settle", market=market, owner=str(kp.pubkey()))],
        )
        flow.out(f"{spec['name']} begin_settle {sig}")
    except Exception as e:
        fail(f"{spec['name']} begin_settle {e}")
        return
    page.goto(f"{BASE}/portfolio", wait_until="networkidle")
    for _ in range(20):
        if page.get_by_role("button", name="Claim", exact=True).count() or page.get_by_role("button", name="Refund", exact=True).count():
            break
        page.wait_for_timeout(500)
        page.reload(wait_until="networkidle")
    claim = page.get_by_role("button", name="Claim", exact=True)
    refund = page.get_by_role("button", name="Refund", exact=True)
    btn = claim if claim.count() else refund
    if btn.count() == 0:
        fail(f"{spec['name']} no Claim/Refund after settle")
        return
    btn.first.click()
    try:
        page.wait_for_function(
            """() => /payout|refund /.test(document.body.innerText)""",
            timeout=45_000,
        )
        flow.out(f"{spec['name']} claim ok")
    except PwTimeout:
        fail(f"{spec['name']} claim: {flow.note_or_err(page)}")


def run(kp) -> int:
    stamp = str(int(time.time()) % 10_000_000)
    secret = list(bytes(kp))
    from playwright.sync_api import sync_playwright

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": 1400, "height": 900})
        flow.connect_wallet(page, secret)
        page.get_by_role("link", name="Portfolio").click()
        page.wait_for_load_state("networkidle")
        page.locator("input[type='number']").first.fill("500")
        page.get_by_role("button", name="Deposit", exact=True).click()
        try:
            wait_text(page, "deposit ", timeout_ms=45_000)
            flow.out("deposit ok")
        except PwTimeout:
            fail("deposit: " + flow.note_or_err(page))
            browser.close()
            return 1

        for spec in FAMILIES:
            page.goto(BASE + "/create", wait_until="networkidle")
            page.get_by_role("button", name=spec["pick"], exact=False).click()
            page.get_by_label("Listing title").fill(f"{spec['name']} flow {stamp}")
            page.get_by_label("Topic / series (on-chain id)").fill(f"{spec['topic']}{stamp}")
            if spec.get("n"):
                page.get_by_label("n_grid / atoms").select_option(spec["n"])
            page.get_by_label("Close in seconds").fill(str(CLOSE_IN))
            close_at = time.time() + CLOSE_IN
            page.get_by_role("button", name=spec["create"]).click()
            needle = {
                0: "create_skellam",
                1: "create_gaussian",
                2: "create_lognormal",
                3: "create_dirichlet",
                4: "create_bernoulli",
            }[spec["id"]]
            try:
                wait_text(page, needle, timeout_ms=45_000)
            except PwTimeout:
                fail(f"{spec['name']} create: {flow.note_or_err(page)}")
                flow.shot(page, f"create-{spec['name']}")
                continue
            market = open_market_href(page)
            flow.out(f"{spec['name']} market {market}")
            page.get_by_role("link", name="Open market").first.click()
            page.wait_for_load_state("networkidle")
            buy_on_board(page, spec)
            flow.shot(page, f"buy-{spec['name']}")
            settle_market(page, kp, spec, market, close_at)
            flow.shot(page, f"settle-{spec['name']}")

        browser.close()
    flow.out("FINDINGS " + str(len(FINDINGS)))
    for row in FINDINGS:
        flow.out("- " + row)
    (flow.OUT / "families-findings.json").write_text(__import__("json").dumps(FINDINGS, indent=2), encoding="utf-8")
    return 1 if FINDINGS else 0


def main() -> int:
    kp = flow.fund_chain()
    return run(kp)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        flow.out("FAMILIES_FATAL " + str(e))
        raise
