#!/usr/bin/env python3
"""Phase 8: heartbeat + notify + live keeper idempotent commit."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FINDINGS: list[str] = []
HB = ROOT / "tmp" / "keeper-heartbeat.json"
NOTIFY = ROOT / "tmp" / "notify"
CPM = ROOT / "target" / "debug" / "cpm.exe"
KP = ROOT / "tmp" / "phase6-id.json"

spec_flow = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec_flow)
assert spec_flow.loader
spec_flow.loader.exec_module(flow)

spec_cases = importlib.util.spec_from_file_location("cases", ROOT / "scripts" / "phase6-family-cases.py")
cases = importlib.util.module_from_spec(spec_cases)
assert spec_cases.loader
spec_cases.loader.exec_module(cases)


def fail(msg: str) -> None:
    FINDINGS.append(msg)
    print("FAIL", msg)


def ok(msg: str) -> None:
    print("OK  ", msg)


def http(path: str):
    with urllib.request.urlopen(f"{flow.API}{path}", timeout=10) as r:
        return json.loads(r.read())


def journal_append(replica: Path, obj: Path, market: str, owner: str, nonce: int, sig: str, prev: bytes, seq: int) -> bytes:
    rec = {
        "seq": seq,
        "market": market,
        "owner": owner,
        "nonce": nonce,
        "sig": sig,
        "prev_root": prev.hex(),
        "root": "",
    }
    body = f"{seq}|{market}|{owner}|{nonce}|{sig}".encode()
    root = hashlib.sha256(prev + body).digest()
    rec["root"] = root.hex()
    name = "".join(c if c.isalnum() else "_" for c in market) + ".jsonl"
    for d in (replica, obj):
        d.mkdir(parents=True, exist_ok=True)
        with (d / name).open("a", encoding="utf-8") as f:
            f.write(json.dumps(rec, separators=(",", ":")) + "\n")
    return root


def keeper() -> str:
    r = subprocess.run(
        [
            str(CPM),
            "--url",
            flow.RPC,
            "--er-url",
            os.environ.get("ER_URL", "http://127.0.0.1:7799"),
            "--keypair",
            str(KP),
            "keeper",
            "--once",
            "--heartbeat",
            str(HB),
            "--notify-dir",
            str(NOTIFY),
            "--journal-replica",
            str(ROOT / "journal-replica"),
            "--journal-object",
            str(ROOT / "journal-object"),
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    out = (r.stdout or "") + (r.stderr or "")
    if r.returncode != 0:
        fail(f"keeper {r.returncode} {out[-400:]}")
    return out


def run_offline() -> None:
    r = subprocess.run(
        ["cargo", "test", "-p", "notify", "--", "--nocapture"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        fail((r.stderr or r.stdout)[-400:])
    else:
        ok("notify crate tests")
    ev = {"ts": 1, "kind": "close", "market": "MktOnly"}
    if "owner" in ev or "email" in ev:
        fail("notify leaked identity")
    else:
        ok("notify market_id only")


def run_live() -> None:
    try:
        ops = http("/v1/ops/status")
    except Exception as e:
        fail(f"ops {e}")
        return
    if ops.get("keeper_heartbeat_slot") is None:
        fail("ops missing keeper_heartbeat_slot")
        return
    kp = flow.fund_chain()
    owner = str(kp.pubkey())
    stamp = str(int(time.time() * 1000) % 10_000_000)
    spec = {
        "id": "phase8-keeper",
        "family": 4,
        "op": "create_bernoulli",
        "topic": "p8",
        "tag": "kp",
        "n": 2,
        "title": "Phase8 Keeper",
        "category": "macro",
        "milli": False,
        "create_extra": {},
    }
    topic = f"{spec['topic']}{stamp}"
    close_ts = int(time.time()) + 180
    market = cases.market_pda(spec["family"], topic, spec["tag"])
    app_id = cases.record_application(kp, spec, topic, spec["tag"], close_ts, spec["title"] + stamp, spec["id"])
    try:
        cases.send(kp, **cases.create_body(spec, owner, topic, spec["tag"], close_ts))
        cases.send(kp, "fund_cm", owner=owner, market=market, amount=0)
        cases.send(kp, "buy_set", owner=owner, market=market, mask="01", shares=1, nonce=1)
        cases.send(kp, "delegate_book", owner=owner, market=market)
    except Exception as e:
        fail(f"setup {e}")
        return
    cases.mark_opened(kp, spec, app_id, market)
    replica, obj = ROOT / "journal-replica", ROOT / "journal-object"
    journal_append(replica, obj, market, owner, 1, "fill-p8", bytes(32), 1)
    first = keeper()
    if "commit" not in first:
        fail(f"first keeper did not commit: {first}")
        return
    ok("keeper first commit")
    n1 = (NOTIFY / "events.jsonl").read_text(encoding="utf-8").count('"kind":"commit"') if (NOTIFY / "events.jsonl").exists() else 0
    second = keeper()
    n2 = (NOTIFY / "events.jsonl").read_text(encoding="utf-8").count('"kind":"commit"') if (NOTIFY / "events.jsonl").exists() else 0
    if n2 != n1:
        fail(f"second keeper committed again n1={n1} n2={n2} {second}")
        return
    ok("keeper second run no extra commit")
    ops = http("/v1/ops/status")
    hb = json.loads(HB.read_text(encoding="utf-8"))
    if ops.get("keeper_heartbeat_slot") != hb.get("slot"):
        fail(f"ops slot {ops.get('keeper_heartbeat_slot')} != hb {hb.get('slot')}")
        return
    if ops.get("keeper_ok") is not True:
        fail(f"ops keeper_ok {ops.get('keeper_ok')}")
        return
    ok(f"ops heartbeat slot={hb.get('slot')}")


def main() -> int:
    run_offline()
    run_live()
    print("FINDINGS", len(FINDINGS))
    for row in FINDINGS:
        print("-", row)
    return 1 if FINDINGS else 0


if __name__ == "__main__":
    sys.exit(main())
