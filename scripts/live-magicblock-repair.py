#!/usr/bin/env python3
"""Finish bern/skellam settle + gateway ER fill on the live MagicBlock validators."""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CPM = ROOT / "target" / "debug" / "cpm.exe"
GW_BIN = ROOT / "target" / "debug" / "trading-gateway.exe"
KEYPAIR = ROOT / "tmp" / "live-id.json"
SESS = ROOT / "tmp" / "e2e-session.json"
L1 = "http://127.0.0.1:8899"
ER = "http://127.0.0.1:7799"
GW = "http://127.0.0.1:8082"
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
    if p.returncode != 0:
        raise RuntimeError(text[-900:] or f"exit {p.returncode}")
    print(text, flush=True)
    return text


def try_step(name: str, args: list[str], timeout: int = 180) -> str | None:
    try:
        text = cpm(args, timeout=timeout)
        rec(name, "PASS", text.splitlines()[-1][:180])
        return text
    except Exception as e:
        rec(name, "FAIL", str(e))
        return None


def gw_health() -> bool:
    try:
        raw = urllib.request.urlopen(GW + "/v1/health", timeout=5).read().decode()
        return '"ok":true' in raw.replace(" ", "")
    except Exception:
        return False


def parse_create(text: str) -> str:
    m = re.search(r"market=([1-9A-HJ-NP-Za-km-z]{32,44})", text)
    if not m:
        raise RuntimeError(text)
    return m.group(1)


def main() -> int:
    bern = "C9w3Gt4i4TTHJhyWGNsgqqoqqoxUvXMkr8bCWrnnhkNR"
    skel = "BvWSNzNjEcmyvAJZ5YUiPepbP5LGz4F6uUUH99DBtPWz"

    try_step("skellam/quote-16", ["market", "quote", skel, "--mask", "01" + "00" * 15, "--shares", "20"])
    try_step("bern/undelegate", ["market", "undelegate", bern, "--mask", "01"], timeout=80)
    try_step("skellam/undelegate", ["market", "undelegate", skel, "--skellam-kind", "0"], timeout=80)
    try_step("bern/resolve-open", ["resolve", "open", bern])
    try_step("skellam/resolve-open", ["resolve", "open", skel])
    try_step("bern/submit", ["resolve", "submit", bern, "0", "--family", "4", "--kind", "4"])
    try_step("skellam/submit", ["resolve", "submit", skel, "1", "--family", "0", "--kind", "0", "--b", "0"])
    time.sleep(8)
    try_step("bern/finalize", ["resolve", "finalize", bern])
    try_step("skellam/finalize", ["resolve", "finalize", skel])
    try_step("bern/settle-begin", ["settle", "begin", bern])
    try_step("skellam/settle-begin", ["settle", "begin", skel])
    try_step("bern/payout", ["settle", "payout", bern, "01"])
    try_step("skellam/payout", ["settle", "payout-skellam", skel, "--kind", "0"])

    gw_proc = None
    if not gw_health() and GW_BIN.exists():
        env = os.environ.copy()
        env.update(
            {
                "RPC_URL": L1,
                "ER_RPC": ER,
                "LISTEN": "127.0.0.1:8082",
                "RECEIPT_DIR": str(ROOT / "tmp" / "e2e-receipts"),
                "JOURNAL_REPLICA_DIR": str(ROOT / "tmp" / "e2e-journal-replica"),
                "JOURNAL_OBJECT_DIR": str(ROOT / "tmp" / "e2e-journal-object"),
            }
        )
        Path(env["RECEIPT_DIR"]).mkdir(parents=True, exist_ok=True)
        gw_proc = subprocess.Popen([str(GW_BIN)], cwd=str(ROOT), env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(40):
            if gw_health():
                break
            time.sleep(0.25)
    rec("gateway-health", "PASS" if gw_health() else "FAIL", GW)

    try_step("session-open", ["session", "open", "--authority", str(SESS), "--hours", "2", "--usdc", "20000"])
    stamp = str(int(time.time()))[-6:]
    created = try_step(
        "gw/create",
        [
            "market",
            "create-gaussian",
            f"e2e-gw-{stamp}",
            "tag",
            "--n",
            "8",
            "--close-in",
            "180",
            "--challenge-secs",
            "6",
        ],
    )
    if created:
        pk = parse_create(created)
        try_step("gw/fund", ["settle", "fund-cm", pk])
        try_step("gw/l1-buy", ["trade", "buy-set", pk, "01", "40"])
        try_step("gw/delegate", ["market", "delegate", pk])
        if gw_health():
            try_step(
                "gw/gateway-er-buy",
                ["trade", "buy-set", pk, "01", "15", "--session", str(SESS), "--gateway", GW],
            )
        try_step("gw/close-sync", ["market", "close", pk, "--mask", "01"], timeout=80)

    print("\n======== REPAIR ========")
    fails = 0
    for step, status, detail in rows:
        print(f"{status:5}  {step:24}  {detail}")
        if status == "FAIL":
            fails += 1
    print(f"======== {fails} FAIL / {len(rows)} steps ========")
    if gw_proc is not None:
        gw_proc.terminate()
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
