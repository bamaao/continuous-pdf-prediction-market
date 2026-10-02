#!/usr/bin/env python3
"""Retest the two live gaps: n=256 Delegate + Bernoulli/Skellam undelegate/settle.

Real txs on L1 :8899 and ER :7799. Failures are printed; never silent SKIP.
Does not restart :3000.
"""
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
CPM = Path(os.environ.get("CPM", str(ROOT / "target" / "debug" / "cpm.exe")))
KEYPAIR = Path(os.environ.get("KEYPAIR", str(ROOT / "tmp" / "live-id.json")))
L1 = os.environ.get("URL", "http://127.0.0.1:8899")
ER = os.environ.get("ER_URL", "http://127.0.0.1:7799")
OWNER = os.environ.get("OWNER", "HaHjcAoa9wanJvWDugM4bFoBsQXyM5vg9gsSzZFp8qhn")
DUMP = ROOT / "tmp" / "dump_acc.py"
OWNER_PY = ROOT / "tmp" / "owner.py"

rows: list[tuple[str, str, str]] = []


def log(msg: str) -> None:
    print(msg, flush=True)


def rec(step: str, status: str, detail: str = "") -> None:
    rows.append((step, status, detail[:240]))
    log(f"[{status}] {step}" + (f"  {detail}" if detail else ""))


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


def try_step(name: str, args: list[str], timeout: int = 180, idempotent: bool = False) -> str | None:
    try:
        text = cpm(args, timeout=timeout)
        rec(name, "PASS", text.splitlines()[-1][:180] if text else "")
        return text
    except Exception as e:
        msg = str(e)
        if idempotent and any(
            s in msg
            for s in (
                "already in use",
                "AlreadyInUse",
                "already initialized",
                "custom program error: 0x0",
            )
        ):
            rec(name, "PASS", "already exists")
            return msg
        rec(name, "FAIL", msg)
        return None


def parse_create(text: str) -> tuple[str, int, int]:
    mkt = re.search(r"market=([1-9A-HJ-NP-Za-km-z]{32,44})", text)
    close = re.search(r"close_ts=(\d+)", text)
    chal = re.search(r"challenge_secs=(\d+)", text)
    if not mkt or not close:
        raise RuntimeError(f"parse create: {text}")
    return mkt.group(1), int(close.group(1)), int(chal.group(1) if chal else 6)


def mask_n(n: int) -> str:
    """First cell only (sparse)."""
    nbytes = (n + 7) // 8
    return "01" + ("00" * (nbytes - 1))


def mask_all(n: int) -> str:
    """Every cell — n=1024 needs batched wide fill."""
    nbytes = (n + 7) // 8
    rem = n % 8
    if rem == 0:
        return "ff" * nbytes
    return "ff" * (nbytes - 1) + f"{(1 << rem) - 1:02x}"


def rpc_ok(url: str) -> bool:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getHealth"}).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        raw = urllib.request.urlopen(req, timeout=8).read().decode()
        return "ok" in raw.lower() or '"result"' in raw
    except Exception:
        return False


def owner_of(url: str, pk: str) -> str:
    p = subprocess.run(
        [sys.executable, str(OWNER_PY), url, pk],
        capture_output=True,
        text=True,
        timeout=20,
    )
    return (p.stdout or "").strip() or (p.stderr or "").strip() or "ERR"


def wait_close(ts: int) -> None:
    now = int(time.time())
    if ts > now:
        sl = ts - now + 2
        log(f"== wait {sl}s until close_ts={ts} ==")
        time.sleep(sl)


def cycle(
    sid: str,
    create: list[str],
    create_timeout: int,
    mask: str | None,
    skellam: bool,
    submit: list[str],
    payout: list[str],
) -> None:
    created = try_step(f"{sid}/create", create, timeout=create_timeout)
    if not created:
        return
    try:
        pk, cts, chal = parse_create(created)
    except Exception as e:
        rec(f"{sid}/parse", "FAIL", str(e))
        return
    try_step(f"{sid}/fund", ["settle", "fund-cm", pk])
    if skellam:
        try_step(f"{sid}/l1-buy", ["trade", "buy-skellam", pk, "50", "--kind", "0"])
    else:
        try_step(f"{sid}/l1-buy", ["trade", "buy-set", pk, mask or "01", "50"])
    dlg = try_step(f"{sid}/delegate", ["market", "delegate", pk], timeout=180)
    own = owner_of(L1, pk)
    rec(
        f"{sid}/owner-after-delegate",
        "PASS" if "DELeG" in own else "FAIL",
        own,
    )
    if not dlg:
        return
    if skellam:
        try_step(f"{sid}/er-buy", ["trade", "buy-skellam", pk, "20", "--kind", "0"])
        try_step(f"{sid}/er-sell", ["trade", "sell-skellam", pk, "8", "--kind", "0"])
        close_args = ["market", "close", pk, "--skellam-kind", "0"]
    else:
        try_step(f"{sid}/er-buy", ["trade", "buy-set", pk, mask or "01", "20"])
        try_step(f"{sid}/er-sell", ["trade", "sell-set", pk, mask or "01", "8"])
        close_args = ["market", "close", pk, "--mask", mask or "01"]
    closed = try_step(f"{sid}/close-sync", close_args, timeout=900, idempotent=True)
    own2 = owner_of(L1, pk)
    rec(
        f"{sid}/owner-after-undelegate",
        "PASS" if "Market1111" in own2 else "FAIL",
        own2,
    )
    if closed and "dump" in closed:
        rec(f"{sid}/grid-dump", "FAIL", "close used dump leftover; shard 0 must come home")
    homes = re.findall(r"home shard(\d+)=([1-9A-HJ-NP-Za-km-z]{32,44})", closed or "")
    core = [(idx, shard) for idx, shard in homes if int(idx) == 0]
    if core:
        own_g = owner_of(L1, core[0][1])
        rec(
            f"{sid}/shard0-home",
            "PASS" if "Market1111" in own_g else "FAIL",
            own_g,
        )
    elif sid in ("gauss1024", "gauss256", "skellam", "bern"):
        rec(f"{sid}/shard0-home", "FAIL", "no shard0 printed after undelegate")
    extra = [(idx, shard) for idx, shard in homes if int(idx) >= 1]
    if extra:
        idx, shard = extra[-1]
        own_s = owner_of(L1, shard)
        rec(
            f"{sid}/shard{idx}-home",
            "PASS" if "Market1111" in own_s else "FAIL",
            own_s,
        )
    elif homes:
        rec(f"{sid}/shard-home", "PASS", f"core printed {len(homes)}")
    elif sid in ("gauss256", "skellam"):
        rec(f"{sid}/shard-home", "FAIL", "no shard printed after undelegate")
    wait_close(cts)
    try_step(f"{sid}/resolve-open", ["resolve", "open", pk], idempotent=True)
    sub = [x if x is not None else pk for x in submit]
    try_step(f"{sid}/submit", sub)
    time.sleep(chal + 2)
    try_step(f"{sid}/finalize", ["resolve", "finalize", pk])
    try_step(f"{sid}/settle-begin", ["settle", "begin", pk])
    pay = [x if x is not None else pk for x in payout]
    try_step(f"{sid}/payout", pay)


def main() -> int:
    if not CPM.exists():
        rec("cli-bin", "FAIL", f"missing {CPM}")
        return 1
    if not rpc_ok(L1):
        rec("l1-health", "FAIL", L1)
        return 1
    rec("l1-health", "PASS", L1)
    if not rpc_ok(ER):
        rec("er-health", "FAIL", ER)
        return 1
    rec("er-health", "PASS", ER)

    try_step("faucet", ["faucet", "200000"])
    try_step("vault-init", ["vault-init"], timeout=40, idempotent=True)
    try_step("deposit", ["deposit", "80000"])
    try_step("committee-init", ["committee", "init", "--m", "1"], idempotent=True)
    try_step("committee-set", ["committee", "set", "--members", OWNER, "--m", "1"])

    stamp = str(int(time.time()))[-6:]
    only = os.environ.get("GAP_ONLY") or ""
    if only in ("", "gauss1024"):
        cycle(
            "gauss1024",
            [
                "market",
                "create-gaussian",
                f"gap-g1k-{stamp}",
                "tag",
                "--n",
                "1024",
                "--close-in",
                "480",
                "--challenge-secs",
                "6",
            ],
            420,
            mask_all(1024),
            False,
            ["resolve", "submit", None, "0", "--family", "1", "--kind", "1"],
            ["settle", "payout", None, mask_all(1024)],
        )
    if only in ("", "gauss256", "rest"):
        cycle(
            "gauss256",
            [
                "market",
                "create-gaussian",
                f"gap-g256-{stamp}",
                "tag",
                "--n",
                "256",
                "--close-in",
                "180",
                "--challenge-secs",
                "6",
            ],
            300,
            mask_all(256),
            False,
            ["resolve", "submit", None, "0", "--family", "1", "--kind", "1"],
            ["settle", "payout", None, mask_all(256)],
        )
    if only in ("", "bern", "rest"):
        cycle(
            "bern",
            [
                "market",
                "create-bernoulli",
                f"gap-be-{stamp}",
                "tag",
                "--close-in",
                "90",
                "--challenge-secs",
                "6",
            ],
            180,
            mask_n(2),
            False,
            ["resolve", "submit", None, "0", "--family", "4", "--kind", "4"],
            ["settle", "payout", None, mask_n(2)],
        )
    if only in ("", "skellam", "rest"):
        cycle(
            "skellam",
            [
                "market",
                "create-skellam",
                f"gap-sk-{stamp}",
                "--close-in",
                "90",
                "--challenge-secs",
                "6",
            ],
            180,
            None,
            True,
            ["resolve", "submit", None, "1", "--family", "0", "--kind", "0", "--b", "0"],
            ["settle", "payout-skellam", None, "--kind", "0"],
        )

    log("\n======== GAP RETEST ========")
    fails = 0
    for step, status, detail in rows:
        log(f"{status:5}  {step:32}  {detail}")
        if status == "FAIL":
            fails += 1
    log(f"======== {fails} FAIL / {len(rows)} steps ========")
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
