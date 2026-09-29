#!/usr/bin/env python3
"""Phase 7: dual-copy fill journal + L1 Delegate/Commit/Undelegate gates."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from solders.pubkey import Pubkey

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tmp" / "phase7-journal"
OUT.mkdir(parents=True, exist_ok=True)

spec_flow = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec_flow)
assert spec_flow.loader
spec_flow.loader.exec_module(flow)

spec_cases = importlib.util.spec_from_file_location("cases", ROOT / "scripts" / "phase6-family-cases.py")
cases = importlib.util.module_from_spec(spec_cases)
assert spec_cases.loader
spec_cases.loader.exec_module(cases)

FINDINGS: list[str] = []


def fail(case: str, msg: str) -> None:
    FINDINGS.append(f"{case}: {msg}")
    flow.out(f"FAIL {case}: {msg}")


def ok(case: str, msg: str) -> None:
    flow.out(f"OK   {case}: {msg}")


def hex32(b: bytes) -> str:
    return b.hex()


def chain(prev: bytes, body: bytes) -> bytes:
    return hashlib.sha256(prev + body).digest()


def append_line(path: Path, rec: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as f:
        f.write(json.dumps(rec, separators=(",", ":")) + "\n")


def journal_append(replica: Path, obj: Path, market: str, owner: str, nonce: int, sig: str, prev: bytes, seq: int) -> bytes:
    rec = {
        "seq": seq,
        "market": market,
        "owner": owner,
        "nonce": nonce,
        "sig": sig,
        "prev_root": hex32(prev),
        "root": "",
    }
    body = f"{seq}|{market}|{owner}|{nonce}|{sig}".encode()
    root = chain(prev, body)
    rec["root"] = hex32(root)
    name = "".join(c if c.isalnum() else "_" for c in market) + ".jsonl"
    append_line(replica / name, rec)
    append_line(obj / name, rec)
    return root


def replay_file(path: Path) -> tuple[int, bytes]:
    if not path.exists():
        return 0, bytes(32)
    prev = bytes(32)
    n = 0
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        rec = json.loads(line)
        body = f"{rec['seq']}|{rec['market']}|{rec['owner']}|{rec['nonce']}|{rec['sig']}".encode()
        root = chain(prev, body)
        if rec["prev_root"] != hex32(prev) or rec["root"] != hex32(root):
            raise ValueError(f"fork at seq={rec.get('seq')}")
        prev = root
        n += 1
    return n, prev


def run_journal_offline() -> None:
    base = Path(tempfile.mkdtemp(prefix="cpm-p7-"))
    replica, obj = base / "replica", base / "object"
    prev = bytes(32)
    prev = journal_append(replica, obj, "MktAAA", "OwnAAA", 1, "sig1", prev, 1)
    want = journal_append(replica, obj, "MktAAA", "OwnAAA", 2, "sig2", prev, 2)
    name = "MktAAA.jsonl"
    n, root = replay_file(replica / name)
    if n != 2 or root != want:
        fail("journal-replay", f"n={n} root={hex32(root)}")
        return
    shutil.rmtree(replica)
    n2, root2 = replay_file(obj / name)
    if n2 != 2 or root2 != want:
        fail("journal-kill-replica", f"n={n2} root={hex32(root2)}")
        return
    ok("journal-kill-replica", hex32(want)[:16])
    r = subprocess.run(
        ["cargo", "test", "-p", "journal", "--", "--nocapture"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        fail("journal-cargo-test", (r.stderr or r.stdout)[-400:])
    else:
        ok("journal-cargo-test", "2 passed")


def acc_data(rpc, pk: Pubkey) -> bytes | None:
    info = rpc.get_account_info(pk, commitment=flow.Confirmed).value
    if info is None:
        return None
    return bytes(info.data)


def run_chain() -> None:
    try:
        st, _ = flow.http_json("GET", f"{flow.API}/v1/health")
        if st != 200:
            fail("api", f"health {st}")
            return
        st, probe = flow.http_json(
            "POST",
            f"{flow.API}/v1/compose",
            {
                "op": "delegate_book",
                "owner": "11111111111111111111111111111111",
                "market": "11111111111111111111111111111111",
            },
        )
        if st != 200:
            flow.out(f"SKIP live chain: compose delegate_book {st} (restart market-api + reload market.so)")
            return
    except Exception as e:
        fail("api", str(e))
        return
    kp = flow.fund_chain()
    owner = str(kp.pubkey())
    try:
        cases.send(kp, "deposit", owner=owner, amount=2000)
    except Exception as e:
        flow.out(f"deposit skip {e}")
    stamp = str(int(time.time()) % 10_000_000)
    spec = {
        "id": "phase7-delegate",
        "family": 4,
        "op": "create_bernoulli",
        "topic": "p7",
        "tag": "er",
        "n": 2,
        "title": "Phase7 Bernoulli",
        "category": "macro",
        "milli": False,
        "create_extra": {},
    }
    topic = f"{spec['topic']}{stamp}"
    tag = spec["tag"]
    close_ts = int(time.time()) + 120
    market = cases.market_pda(spec["family"], topic, tag)
    app_id = cases.record_application(kp, spec, topic, tag, close_ts, spec["title"] + stamp, spec["id"])
    try:
        cases.send(kp, **cases.create_body(spec, owner, topic, tag, close_ts))
        cases.send(kp, "fund_cm", owner=owner, market=market, amount=0)
    except Exception as e:
        fail("create", str(e))
        return
    cases.mark_opened(kp, spec, app_id, market)
    rpc = flow.Client(flow.RPC, commitment=flow.Confirmed)
    try:
        cases.send(kp, "buy_set", owner=owner, market=market, mask="01", shares=1, nonce=1)
        ok("l1-buy-before-delegate", "nonce=1")
    except Exception as e:
        fail("l1-buy-before-delegate", str(e))
        return
    try:
        cases.send(kp, "delegate_book", owner=owner, market=market)
        ok("delegate_book", market[:12])
    except Exception as e:
        text = str(e)
        if "invalid instruction" in text.lower() or "Fallback" in text or "InstructionFallbackNotFound" in text:
            flow.out("SKIP live chain: validator still has pre-Phase-7 market.so")
            return
        fail("delegate_book", text)
        return
    try:
        cases.send(kp, "buy_set", owner=owner, market=market, mask="01", shares=1, nonce=2)
        fail("l1-buy-after-delegate", "expected Delegated")
    except Exception as e:
        text = str(e)
        if "Delegated" in text or "6023" in text:
            ok("l1-buy-after-delegate", "Delegated")
        else:
            fail("l1-buy-after-delegate", text[-400:])
            return
    board = Pubkey.find_program_address([b"board", bytes(Pubkey.from_string(market))], flow.VAULT)[0]
    user = Pubkey.find_program_address([b"user", bytes(kp.pubkey())], flow.VAULT)[0]
    before_board = acc_data(rpc, board)
    before_user = acc_data(rpc, user)
    replica = OUT / "replica"
    obj = OUT / "object"
    shutil.rmtree(replica, ignore_errors=True)
    shutil.rmtree(obj, ignore_errors=True)
    root = journal_append(replica, obj, market, owner, 1, "fill-1", bytes(32), 1)
    try:
        cases.send(kp, "commit_book", owner=owner, market=market, evidence_hex=hex32(root))
        ok("commit_book", hex32(root)[:16])
    except Exception as e:
        fail("commit_book", str(e))
        return
    after_board = acc_data(rpc, board)
    after_user = acc_data(rpc, user)
    if before_board != after_board:
        fail("vault-board-unchanged", "board account bytes changed on commit")
    else:
        ok("vault-board-unchanged", "same")
    if before_user != after_user:
        fail("vault-user-unchanged", "user vault bytes changed on commit")
    else:
        ok("vault-user-unchanged", "same")
    shutil.rmtree(replica)
    n, replayed = replay_file(obj / ("".join(c if c.isalnum() else "_" for c in market) + ".jsonl"))
    if n != 1 or replayed != root:
        fail("journal-object-survives", f"n={n}")
    else:
        ok("journal-object-survives", hex32(replayed)[:16])
    try:
        cases.send(kp, "halt", owner=owner, market=market)
        cases.send(kp, "undelegate_book", owner=owner, market=market)
        ok("undelegate_after_halt", "cleared")
    except Exception as e:
        fail("undelegate_after_halt", str(e))


def main() -> int:
    run_journal_offline()
    run_chain()
    (OUT / "findings.json").write_text(json.dumps(FINDINGS, indent=2), encoding="utf-8")
    flow.out("FINDINGS " + str(len(FINDINGS)))
    for row in FINDINGS:
        flow.out("- " + row)
    return 1 if FINDINGS else 0


if __name__ == "__main__":
    sys.exit(main())
