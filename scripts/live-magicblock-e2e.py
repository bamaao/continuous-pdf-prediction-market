#!/usr/bin/env python3
"""Live MagicBlock E2E: every family + vault/session/ER/risk/resolve/settle/gateway.

Real txs on L1 :8899 and ER :7799. Failures are printed; never silent SKIP.
Does not restart :3000. Does not reset validators.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CPM = Path(os.environ.get("CPM", str(ROOT / "target" / "debug" / "cpm.exe")))
GW_BIN = Path(os.environ.get("GW_BIN", str(ROOT / "target" / "debug" / "trading-gateway.exe")))
KEYPAIR = Path(os.environ.get("KEYPAIR", str(ROOT / "tmp" / "live-id.json")))
SESS = Path(os.environ.get("SESS", str(ROOT / "tmp" / "e2e-session.json")))
L1 = os.environ.get("URL", "http://127.0.0.1:8899")
ER = os.environ.get("ER_URL", "http://127.0.0.1:7799")
GW = os.environ.get("GW", "http://127.0.0.1:8082")
DUMP = ROOT / "tmp" / "dump_acc.py"
USER_VAULT = os.environ.get("USER_VAULT", "8kSuaNf5FtrecG28AkFReDBPCy3NyV9EMfQU7iaRHPQJ")
SESSION_PDA = os.environ.get("SESSION_PDA", "")

rows: list[tuple[str, str, str]] = []


def log(msg: str) -> None:
    print(msg, flush=True)


def rec(step: str, status: str, detail: str = "") -> None:
    rows.append((step, status, detail[:240]))
    log(f"[{status}] {step}" + (f"  {detail}" if detail else ""))


def run(args: list[str], timeout: int = 180, allow_fail: bool = False) -> subprocess.CompletedProcess[str]:
    cmd = [str(CPM), "--url", L1, "--er-url", ER, "--keypair", str(KEYPAIR), *args]
    p = subprocess.run(cmd, cwd=str(ROOT), capture_output=True, text=True, timeout=timeout)
    if p.returncode != 0 and not allow_fail:
        err = (p.stderr or p.stdout or "").strip()
        raise RuntimeError(err[-800:] or f"exit {p.returncode}")
    return p


def out_ok(args: list[str], timeout: int = 180) -> str:
    p = run(args, timeout=timeout)
    text = ((p.stdout or "") + (p.stderr or "")).strip()
    log(text)
    return text


def try_step(name: str, args: list[str], timeout: int = 180, idempotent: bool = False) -> str | None:
    try:
        text = out_ok(args, timeout=timeout)
        rec(name, "PASS", first_sig(text))
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


def first_sig(text: str) -> str:
    m = re.search(r"\b([1-9A-HJ-NP-Za-km-z]{80,88})\b", text)
    return m.group(1) if m else text.splitlines()[-1][:120] if text else ""


def parse_create(text: str) -> tuple[str, int, int]:
    mkt = re.search(r"market=([1-9A-HJ-NP-Za-km-z]{32,44})", text)
    close = re.search(r"close_ts=(\d+)", text)
    chal = re.search(r"challenge_secs=(\d+)", text)
    if not mkt or not close:
        raise RuntimeError(f"parse create: {text}")
    return mkt.group(1), int(close.group(1)), int(chal.group(1) if chal else 8)


def mask_n(n: int) -> str:
    nbytes = (n + 7) // 8
    return "01" + ("00" * (nbytes - 1))


def dump(url: str, pk: str, kind: str = "vault") -> str:
    p = subprocess.run(
        [sys.executable, str(DUMP), url, pk, kind],
        capture_output=True,
        text=True,
        timeout=20,
    )
    return (p.stdout or "").strip() or (p.stderr or "").strip() or "ERR"


def rpc_ok(url: str) -> bool:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getHealth"}).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    try:
        raw = urllib.request.urlopen(req, timeout=8).read().decode()
        return "ok" in raw.lower() or '"result"' in raw
    except Exception:
        return False


def gw_health(url: str) -> bool:
    try:
        raw = urllib.request.urlopen(url.rstrip("/") + "/v1/health", timeout=5).read().decode()
        return '"ok":true' in raw.replace(" ", "") and '"holds_keys":false' in raw.replace(" ", "")
    except Exception:
        return False


def wait_close(close_ts: int) -> None:
    now = int(time.time())
    if close_ts > now:
        sl = close_ts - now + 2
        log(f"== wait close_ts={close_ts} ({sl}s) ==")
        time.sleep(sl)


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

    avail0 = dump(L1, USER_VAULT)
    rec("vault-available-start", "PASS" if avail0.isdigit() else "FAIL", f"L1={avail0}")

    try_step("faucet", ["faucet", "200000"])
    try_step("vault-init", ["vault-init"], timeout=40, idempotent=True)
    dep = try_step("deposit", ["deposit", "80000"])
    if dep:
        m = re.search(r"user=([1-9A-HJ-NP-Za-km-z]{32,44})", dep)
        if m:
            os.environ["USER_VAULT_PARSED"] = m.group(1)
    owner = os.environ.get("OWNER", "HaHjcAoa9wanJvWDugM4bFoBsQXyM5vg9gsSzZFp8qhn")
    try_step("committee-init", ["committee", "init", "--m", "1"], idempotent=True)
    try_step("committee-set", ["committee", "set", "--members", owner, "--m", "1"])

    if not SESS.exists():
        subprocess.run(
            [
                "wsl",
                "-e",
                "bash",
                "-lc",
                f"solana-keygen new --no-bip39-passphrase --silent -o /mnt/e/ai/continuous-pdf-predicion-market/tmp/{SESS.name}",
            ],
            check=False,
        )
        if not SESS.exists():
            rec("session-key", "FAIL", "could not write session keypair")
        else:
            rec("session-key", "PASS", str(SESS))
    else:
        rec("session-key", "PASS", "reuse")

    gw_proc = None
    listen = GW.split("://", 1)[-1]
    if not gw_health(GW) and GW_BIN.exists():
        env = os.environ.copy()
        env.update(
            {
                "RPC_URL": L1,
                "ER_RPC": ER,
                "LISTEN": listen,
                "RECEIPT_DIR": str(ROOT / "tmp" / "e2e-receipts"),
                "JOURNAL_REPLICA_DIR": str(ROOT / "tmp" / "e2e-journal-replica"),
                "JOURNAL_OBJECT_DIR": str(ROOT / "tmp" / "e2e-journal-object"),
            }
        )
        Path(env["RECEIPT_DIR"]).mkdir(parents=True, exist_ok=True)
        Path(env["JOURNAL_REPLICA_DIR"]).mkdir(parents=True, exist_ok=True)
        Path(env["JOURNAL_OBJECT_DIR"]).mkdir(parents=True, exist_ok=True)
        gw_proc = subprocess.Popen([str(GW_BIN)], cwd=str(ROOT), env=env)
        for _ in range(30):
            if gw_health(GW):
                break
            time.sleep(0.5)
    if gw_health(GW):
        rec("gateway-health", "PASS", GW)
    else:
        rec("gateway-health", "FAIL", GW)

    close_ts = int(time.time()) + 240
    chal = 6
    stamp = str(int(time.time()))[-6:]

    specs = [
        {
            "id": "gauss8",
            "family": 1,
            "create": [
                "market",
                "create-gaussian",
                f"e2e-g8-{stamp}",
                "tag",
                "--n",
                "8",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                str(chal),
            ],
            "n": 8,
            "mask": mask_n(8),
            "submit": ["resolve", "submit", None, "0", "--family", "1", "--kind", "1"],
            "payout": "mask",
            "risk": True,
            "session": True,
            "gateway": True,
        },
        {
            "id": "gauss256",
            "family": 1,
            "create": [
                "market",
                "create-gaussian",
                f"e2e-g256-{stamp}",
                "tag",
                "--n",
                "256",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                str(chal),
            ],
            "n": 256,
            "mask": mask_n(256),
            "submit": ["resolve", "submit", None, "0", "--family", "1", "--kind", "1"],
            "payout": "mask",
        },
        {
            "id": "logn",
            "family": 2,
            "create": [
                "market",
                "create-lognormal",
                f"e2e-ln-{stamp}",
                "tag",
                "--n",
                "8",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                str(chal),
            ],
            "n": 8,
            "mask": mask_n(8),
            "submit": ["resolve", "submit", None, "1", "--family", "2", "--kind", "1"],
            "payout": "mask",
        },
        {
            "id": "dirich",
            "family": 3,
            "create": [
                "market",
                "create-dirichlet",
                f"e2e-di-{stamp}",
                "--n",
                "3",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                str(chal),
            ],
            "n": 3,
            "mask": mask_n(3),
            "submit": ["resolve", "submit", None, "0", "--family", "3", "--kind", "2"],
            "payout": "mask",
        },
        {
            "id": "bern",
            "family": 4,
            "create": [
                "market",
                "create-bernoulli",
                f"e2e-be-{stamp}",
                "tag",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                str(chal),
            ],
            "n": 2,
            "mask": mask_n(2),
            "submit": ["resolve", "submit", None, "0", "--family", "4", "--kind", "4"],
            "payout": "mask",
        },
        {
            "id": "skellam",
            "family": 0,
            "create": [
                "market",
                "create-skellam",
                f"e2e-sk-{stamp}",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                str(chal),
            ],
            "n": 121,
            "mask": None,
            "skellam": True,
            "submit": ["resolve", "submit", None, "1", "--family", "0", "--kind", "0", "--b", "0"],
            "payout": "skellam",
        },
    ]

    markets: dict[str, dict] = {}
    for spec in specs:
        sid = spec["id"]
        text = try_step(f"{sid}/create", spec["create"], timeout=300)
        if not text:
            continue
        try:
            mkt, cts, ch = parse_create(text)
        except Exception as e:
            rec(f"{sid}/parse", "FAIL", str(e))
            continue
        markets[sid] = {"pk": mkt, "close_ts": cts, "chal": ch, **spec}
        try_step(f"{sid}/fund", ["settle", "fund-cm", mkt])
        if spec.get("skellam"):
            try_step(
                f"{sid}/quote",
                ["market", "quote", mkt, "--kind", "0", "--shares", "20"],
            )
        else:
            qmask = spec["mask"] or mask_n(int(spec["n"]))
            try_step(f"{sid}/quote", ["market", "quote", mkt, "--mask", qmask, "--shares", "20"])
        try_step(f"{sid}/info", ["market", "info", mkt])

    sess_text = try_step(
        "session-open",
        ["session", "open", "--authority", str(SESS), "--hours", "2", "--usdc", "30000"],
    )
    if not sess_text:
        sess_text = try_step(
            "session-renew",
            ["session", "renew", "--hours", "2", "--usdc", "30000"],
        )
    session_pda = SESSION_PDA
    if sess_text:
        m = re.search(r"session=([1-9A-HJ-NP-Za-km-z]{32,44})", sess_text)
        if m:
            session_pda = m.group(1)

    for sid, m in markets.items():
        pk = m["pk"]
        if m.get("skellam"):
            try_step(f"{sid}/l1-buy", ["trade", "buy-skellam", pk, "80", "--kind", "0"])
        else:
            try_step(f"{sid}/l1-buy", ["trade", "buy-set", pk, m["mask"], "80"])
        if m.get("risk"):
            try_step(f"{sid}/risk-open", ["risk", "open", pk])
            try_step(
                f"{sid}/risk-bid",
                ["risk", "bid", pk, "--layer", "1", "100", "5", "--profit-share-bps", "7000"],
            )

    for sid, m in markets.items():
        try_step(f"{sid}/delegate", ["market", "delegate", m["pk"]])

    for sid, m in markets.items():
        pk = m["pk"]
        if m.get("skellam"):
            try_step(f"{sid}/er-buy", ["trade", "buy-skellam", pk, "40", "--kind", "0"])
            try_step(f"{sid}/er-sell", ["trade", "sell-skellam", pk, "15", "--kind", "0"])
        else:
            try_step(f"{sid}/er-buy", ["trade", "buy-set", pk, m["mask"], "40"])
            try_step(f"{sid}/er-sell", ["trade", "sell-set", pk, m["mask"], "15"])

    g8 = markets.get("gauss8")
    if g8:
        try_step(
            "gauss8/session-er-buy",
            ["trade", "buy-set", g8["pk"], g8["mask"], "20", "--session", str(SESS)],
        )
        if gw_health(GW):
            try_step(
                "gauss8/gateway-er-buy",
                [
                    "trade",
                    "buy-set",
                    g8["pk"],
                    g8["mask"],
                    "10",
                    "--session",
                    str(SESS),
                    "--gateway",
                    GW,
                ],
            )
        rem_er = dump(ER, session_pda, "session") if session_pda else "no-pda"
        rem_l1 = dump(L1, session_pda, "session") if session_pda else "no-pda"
        rec(
            "session-remaining-er",
            "PASS" if rem_er.isdigit() else "FAIL",
            f"ER={rem_er} L1={rem_l1}",
        )

    avail_mid = dump(L1, USER_VAULT)
    rec("vault-available-mid-er", "PASS" if avail_mid.isdigit() else "FAIL", f"L1 stale-ok={avail_mid}")

    wait_close(close_ts)
    try_step(
        "keeper-once",
        [
            "keeper",
            "--once",
            "--journal-replica",
            str(ROOT / "tmp" / "e2e-journal-replica"),
            "--journal-object",
            str(ROOT / "tmp" / "e2e-journal-object"),
            "--heartbeat",
            str(ROOT / "tmp" / "e2e-keeper-heartbeat.json"),
            "--notify-dir",
            str(ROOT / "tmp" / "e2e-notify"),
        ],
    )

    for sid, m in markets.items():
        pk = m["pk"]
        mask = m.get("mask")
        close_args = ["market", "close", pk]
        if m.get("skellam"):
            close_args += ["--skellam-kind", "0"]
        elif mask:
            close_args += ["--mask", mask]
        closed = try_step(f"{sid}/close-sync", close_args, timeout=900, idempotent=True)
        if closed and "dump" in closed:
            rec(f"{sid}/grid-dump", "FAIL", "close used dump leftover; shard 0 must come home")
        homes = re.findall(r"home shard(\d+)=([1-9A-HJ-NP-Za-km-z]{32,44})", closed or "")
        if any(int(idx) == 0 for idx, _ in homes):
            rec(f"{sid}/shard0-home", "PASS", homes[0][1] if homes else "")
        else:
            rec(f"{sid}/shard0-home", "FAIL", "no shard0 printed after undelegate")
        try_step(f"{sid}/resolve-open", ["resolve", "open", pk], idempotent=True)
        sub = [x if x is not None else pk for x in m["submit"]]
        try_step(f"{sid}/submit", sub)

    time.sleep(chal + 2)

    for sid, m in markets.items():
        pk = m["pk"]
        try_step(f"{sid}/finalize", ["resolve", "finalize", pk])
        begin = ["settle", "begin", pk]
        if m.get("risk"):
            begin.append("--risk")
        try_step(f"{sid}/settle-begin", begin)
        if m["payout"] == "skellam":
            try_step(f"{sid}/payout", ["settle", "payout-skellam", pk, "--kind", "0"])
        else:
            try_step(f"{sid}/payout", ["settle", "payout", pk, m["mask"]])

    try_step("session-revoke", ["session", "revoke"])
    try_step("withdraw", ["withdraw", "1"])
    if SESS.exists():
        p = subprocess.run(
            [str(CPM), "--url", L1, "--keypair", str(SESS), "withdraw", "1"],
            cwd=str(ROOT),
            capture_output=True,
            text=True,
        )
        if p.returncode == 0:
            rec("withdraw-session-blocked", "FAIL", "session withdraw unexpectedly succeeded")
        else:
            rec("withdraw-session-blocked", "PASS", "rejected")

    avail1 = dump(L1, USER_VAULT)
    rec("vault-available-end", "PASS" if avail1.isdigit() else "FAIL", f"L1={avail1} start={avail0}")

    log("\n======== MATRIX ========")
    fails = 0
    for step, status, detail in rows:
        log(f"{status:5}  {step:28}  {detail}")
        if status == "FAIL":
            fails += 1
    log(f"======== {fails} FAIL / {len(rows)} steps ========")
    if gw_proc is not None:
        gw_proc.terminate()
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
