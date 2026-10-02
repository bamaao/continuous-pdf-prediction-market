#!/usr/bin/env python3
"""Sequential live cycle for Bernoulli + Skellam: trade on ER then undelegate immediately."""
from __future__ import annotations

import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CPM = ROOT / "target" / "debug" / "cpm.exe"
KEYPAIR = ROOT / "tmp" / "live-id.json"
L1 = "http://127.0.0.1:8899"
ER = "http://127.0.0.1:7799"
rows: list[tuple[str, str, str]] = []


def rec(step: str, status: str, detail: str = "") -> None:
    rows.append((step, status, detail[:200]))
    print(f"[{status}] {step}  {detail}", flush=True)


def cpm(args: list[str], timeout: int = 180) -> str:
    p = subprocess.run(
        [str(CPM), "--url", L1, "--er-url", ER, "--keypair", str(KEYPAIR), *args],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    text = ((p.stdout or "") + (p.stderr or "")).strip()
    print(text, flush=True)
    if p.returncode != 0:
        raise RuntimeError(text[-900:] or f"exit {p.returncode}")
    return text


def try_step(name: str, args: list[str], timeout: int = 180) -> str | None:
    try:
        text = cpm(args, timeout=timeout)
        rec(name, "PASS", text.splitlines()[-1][:180])
        return text
    except Exception as e:
        rec(name, "FAIL", str(e))
        return None


def parse_create(text: str) -> tuple[str, int, int]:
    mkt = re.search(r"market=([1-9A-HJ-NP-Za-km-z]{32,44})", text)
    close = re.search(r"close_ts=(\d+)", text)
    chal = re.search(r"challenge_secs=(\d+)", text)
    if not mkt or not close:
        raise RuntimeError(text)
    return mkt.group(1), int(close.group(1)), int(chal.group(1) if chal else 6)


def wait_close(ts: int) -> None:
    now = int(time.time())
    if ts > now:
        sl = ts - now + 2
        print(f"== wait {sl}s until {ts} ==", flush=True)
        time.sleep(sl)


def cycle_bern() -> None:
    stamp = str(int(time.time()))[-6:]
    created = try_step(
        "bern2/create",
        [
            "market",
            "create-bernoulli",
            f"e2e-be2-{stamp}",
            "tag",
            "--close-in",
            "90",
            "--challenge-secs",
            "6",
        ],
    )
    if not created:
        return
    pk, cts, chal = parse_create(created)
    try_step("bern2/fund", ["settle", "fund-cm", pk])
    try_step("bern2/l1-buy", ["trade", "buy-set", pk, "01", "50"])
    try_step("bern2/delegate", ["market", "delegate", pk])
    try_step("bern2/er-buy", ["trade", "buy-set", pk, "01", "20"])
    try_step("bern2/er-sell", ["trade", "sell-set", pk, "01", "8"])
    try_step("bern2/close-sync", ["market", "close", pk, "--mask", "01"], timeout=90)
    wait_close(cts)
    try_step("bern2/resolve-open", ["resolve", "open", pk])
    try_step("bern2/submit", ["resolve", "submit", pk, "0", "--family", "4", "--kind", "4"])
    time.sleep(chal + 2)
    try_step("bern2/finalize", ["resolve", "finalize", pk])
    try_step("bern2/settle-begin", ["settle", "begin", pk])
    try_step("bern2/payout", ["settle", "payout", pk, "01"])


def cycle_skellam() -> None:
    stamp = str(int(time.time()))[-6:]
    created = try_step(
        "sk2/create",
        [
            "market",
            "create-skellam",
            f"e2e-sk2-{stamp}",
            "--close-in",
            "90",
            "--challenge-secs",
            "6",
        ],
    )
    if not created:
        return
    pk, cts, chal = parse_create(created)
    try_step("sk2/fund", ["settle", "fund-cm", pk])
    try_step("sk2/quote", ["market", "quote", pk, "--mask", "01" + "00" * 15, "--shares", "20"])
    try_step("sk2/l1-buy", ["trade", "buy-skellam", pk, "50", "--kind", "0"])
    try_step("sk2/delegate", ["market", "delegate", pk])
    try_step("sk2/er-buy", ["trade", "buy-skellam", pk, "20", "--kind", "0"])
    try_step("sk2/er-sell", ["trade", "sell-skellam", pk, "8", "--kind", "0"])
    try_step("sk2/close-sync", ["market", "close", pk, "--skellam-kind", "0"], timeout=90)
    wait_close(cts)
    try_step("sk2/resolve-open", ["resolve", "open", pk])
    try_step("sk2/submit", ["resolve", "submit", pk, "1", "--family", "0", "--kind", "0", "--b", "0"])
    time.sleep(chal + 2)
    try_step("sk2/finalize", ["resolve", "finalize", pk])
    try_step("sk2/settle-begin", ["settle", "begin", pk])
    try_step("sk2/payout", ["settle", "payout-skellam", pk, "--kind", "0"])


def main() -> int:
    cycle_bern()
    cycle_skellam()
    print("\n======== FINISH ========")
    fails = 0
    for step, status, detail in rows:
        print(f"{status:5}  {step:22}  {detail}")
        if status == "FAIL":
            fails += 1
    print(f"======== {fails} FAIL / {len(rows)} steps ========")
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
