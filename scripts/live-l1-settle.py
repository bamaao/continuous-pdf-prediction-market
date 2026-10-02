#!/usr/bin/env python3
"""L1-only resolve/settle for Bernoulli + Skellam (no Delegate). Plus finish the gateway gaussian."""
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


def cpm(args: list[str], timeout: int = 120) -> str:
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
        raise RuntimeError(text[-800:] or f"exit {p.returncode}")
    return text


def try_step(name: str, args: list[str]) -> str | None:
    try:
        text = cpm(args)
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
        print(f"== wait {sl}s ==", flush=True)
        time.sleep(sl)


def l1_cycle(sid: str, create: list[str], buy: list[str], submit: list[str], payout: list[str]) -> None:
    created = try_step(f"{sid}/create", create)
    if not created:
        return
    pk, cts, chal = parse_create(created)
    try_step(f"{sid}/fund", ["settle", "fund-cm", pk])
    try_step(f"{sid}/buy", [x if x != "MARKET" else pk for x in buy])
    try_step(f"{sid}/close", ["market", "close", pk])
    wait_close(cts)
    try_step(f"{sid}/resolve-open", ["resolve", "open", pk])
    try_step(f"{sid}/submit", [x if x != "MARKET" else pk for x in submit])
    time.sleep(chal + 2)
    try_step(f"{sid}/finalize", ["resolve", "finalize", pk])
    try_step(f"{sid}/settle-begin", ["settle", "begin", pk])
    try_step(f"{sid}/payout", [x if x != "MARKET" else pk for x in payout])


def main() -> int:
    stamp = str(int(time.time()))[-6:]
    l1_cycle(
        "bern-l1",
        ["market", "create-bernoulli", f"e2e-be-l1-{stamp}", "tag", "--close-in", "25", "--challenge-secs", "5"],
        ["trade", "buy-set", "MARKET", "01", "40"],
        ["resolve", "submit", "MARKET", "0", "--family", "4", "--kind", "4"],
        ["settle", "payout", "MARKET", "01"],
    )
    l1_cycle(
        "sk-l1",
        ["market", "create-skellam", f"e2e-sk-l1-{stamp}", "--close-in", "25", "--challenge-secs", "5"],
        ["trade", "buy-skellam", "MARKET", "40", "--kind", "0"],
        ["resolve", "submit", "MARKET", "1", "--family", "0", "--kind", "0", "--b", "0"],
        ["settle", "payout-skellam", "MARKET", "--kind", "0"],
    )
    gw = "H3hLVjjYEpLUC8J2bDqWtNCnHrYJHGKZrvz9rV7HD8ny"
    try_step("gw/resolve-open", ["resolve", "open", gw])
    try_step("gw/submit", ["resolve", "submit", gw, "0", "--family", "1", "--kind", "1"])
    time.sleep(8)
    try_step("gw/finalize", ["resolve", "finalize", gw])
    try_step("gw/settle-begin", ["settle", "begin", gw])
    try_step("gw/payout", ["settle", "payout", gw, "01"])

    print("\n======== L1 SETTLE ========")
    fails = 0
    for step, status, detail in rows:
        print(f"{status:5}  {step:22}  {detail}")
        if status == "FAIL":
            fails += 1
    print(f"======== {fails} FAIL / {len(rows)} steps ========")
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
